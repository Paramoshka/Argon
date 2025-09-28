use arc_swap::ArcSwap;
use prost::Message;
use rcgen::KeyIdMethod::Sha256;
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use tonic::transport::Channel;
use tracing::{info, warn};

mod argon_config {
    include!("argon.config.rs");
}

use crate::argon_config::{
    Snapshot, WatchRequest, config_discovery_client, config_discovery_client::ConfigDiscoveryClient,
};
use crate::certs;
use crate::snapshot::RouteTable;

const CERT_CA_NAME: &str = "ca.pem";

pub struct GrpcManager {
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
    ready: Arc<RwLock<bool>>,
    snapshot: Arc<RwLock<Snapshot>>,
    route_table: Arc<RwLock<Arc<RouteTable>>>,
    sni: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    ca_updated: Arc<Notify>,
    ca_pem: Arc<ArcSwap<Vec<u8>>>,
}

impl GrpcManager {
    pub fn start(
        controller_addr: String,
        node_id: String,
        ready: Arc<RwLock<bool>>,
        snapshot: Arc<RwLock<Snapshot>>,
        route_table: Arc<RwLock<Arc<RouteTable>>>,
        sni: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let cancel_child = cancel.clone();

        let ready_for_task = ready.clone();
        let snapshot_for_task = snapshot.clone();
        let route_table_for_task = route_table.clone();
        let sni_for_task = sni.clone();
        let ca_updated = Arc::new(Notify::new());
        let ca_pem: Arc<ArcSwap<Vec<u8>>> =
            Arc::new(ArcSwap::from_pointee(Vec::new()));
        let handle = tokio::spawn(async move {
            let mut backoff_ms: u64 = 500;
            let backoff_max: u64 = 10_000;

            loop {
                if cancel_child.is_cancelled() {
                    info!("gRPC manager: cancellation requested, exit loop");
                    break;
                }

                // Result<Channel, String>
                let connect_fut = async {
                    // TODO need tls config
                    let endpoint = Channel::from_shared(controller_addr.clone())
                        .map_err(|e| format!("invalid addr: {e}"))?;

                    let ch = endpoint
                        .connect()
                        .await
                        .map_err(|e| format!("connect error: {e}"))?;
                    Ok::<Channel, String>(ch)
                };

                let channel = match timeout(Duration::from_secs(10), connect_fut).await {
                    Ok(Ok(ch)) => {
                        info!("gRPC connected: {}", controller_addr);
                        backoff_ms = 500;
                        ch
                    }
                    Ok(Err(e)) => {
                        warn!("gRPC connect failed: {}", e);
                        sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms.saturating_mul(2)).min(backoff_max);
                        continue;
                    }
                    Err(_) => {
                        warn!("gRPC connect timeout");
                        sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms.saturating_mul(2)).min(backoff_max);
                        continue;
                    }
                };

                let mut client: ConfigDiscoveryClient<Channel> =
                    config_discovery_client::ConfigDiscoveryClient::new(channel);

                // open sream Watch
                let mut stream = match client
                    .watch(WatchRequest {
                        node_id: node_id.clone(),
                    })
                    .await
                {
                    Ok(resp) => {
                        info!("gRPC watch stream established");
                        {
                            let mut r = ready_for_task.write().await;
                            *r = true;
                        }
                        resp.into_inner()
                    }
                    Err(e) => {
                        warn!("watch RPC failed: {}", e);
                        sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms = (backoff_ms.saturating_mul(2)).min(backoff_max);
                        continue;
                    }
                };

                let mut got_first = false;

                // read stream Snapshots
                loop {
                    tokio::select! {
                        _ = cancel_child.cancelled() => {
                            info!("gRPC manager: cancellation while streaming");
                            return;
                        }
                        _ = cancel_child.cancelled() => {
                            info!("CA updated — forcing reconnect to apply new TLS");
                            break;
                        }

                        msg = stream.message() => {
                            match msg {
                                Ok(Some(snap)) => {
                                    // update shared snapshot
                                    {
                                        let mut slot = snapshot_for_task.write().await;
                                        *slot = snap.clone();
                                    }

                                    // update route_table
                                    let build_route_table = RouteTable::new(&snap);
                                    {
                                        let mut write_route_table = route_table_for_task.write().await;
                                        *write_route_table = Arc::new(build_route_table);
                                    }

                                    // update TLS list
                                    let certs = certs::certificates_from_snap(&snap);
                                    sni_for_task.store(Arc::new(certs));

                                    if !got_first {
                                        got_first = true;
                                        info!(
                                            "received first snapshot: version={}, routes={}, clusters={}",
                                            snap.version,
                                            snap.routes.len(),
                                            snap.clusters.len()
                                        );
                                    } else {
                                        info!(
                                            "snapshot update: version={}, routes={}, clusters={}",
                                            snap.version,
                                            snap.routes.len(),
                                            snap.clusters.len()
                                        );
                                    }
                                }
                                Ok(None) => {
                                    warn!("gRPC stream closed by server");
                                    break; // переподключение
                                }
                                Err(status) => {
                                    warn!("gRPC stream error: {}", status);
                                    break; // переподключение
                                }
                            }
                        }
                    }
                }

                // strict mode, set not ready when lost stream
                // {
                //     let mut r = ready.write().await;
                //     *r = false;
                // }

                sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(backoff_max);
                info!("reconnecting...");
            }
        });

        Self {
            cancel,
            handle,
            ready,
            snapshot,
            route_table,
            sni,
            ca_updated,
            ca_pem
        }
    }

    pub async fn shutdown(self) {
        self.cancel.cancel();
        let _ = self.handle.await;
        info!("gRPC manager stopped");
    }

    pub async fn is_ready(&self) -> bool {
        *self.ready.read().await
    }

    pub async fn latest_snapshot(&self) -> Snapshot {
        self.snapshot.read().await.clone()
    }

    async fn watch_for_certs(&self, path_certs_dir: PathBuf) {
        let cert_ca_path: PathBuf = path_certs_dir.clone().join(CERT_CA_NAME);

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!("cert watcher: stop requested");
                    break;
                }

                _ = sleep(Duration::from_secs(2)) => {
                    if let Ok(changed) = self.try_load_and_store_ca(&cert_ca_path).await {
                        if changed {
                            // let's wake up the gRPC loop so that it reconnects with the new TLS
                            self.ca_updated.notify_one();
                        }
                    }
                }
            }
        }
    }

    async fn try_load_and_store_ca(&self, path: &Path) -> Result<bool, ()> {
        match tokio::fs::read(path).await {
            Ok(new_bytes) if !new_bytes.is_empty() => {
                if !looks_like_pem(&new_bytes) {
                    tracing::warn!("CA file at {} is not a PEM, skip", path.display());
                    return Ok(false);
                }

                let current = self.ca_pem.load(); // Arc<Vec<u8>>
                if bytes_eq(&new_bytes, &current) {
                    self.ca_pem.store(Arc::new(new_bytes));
                    tracing::info!("CA reloaded from {}", path.display());
                    return Ok(true);
                }
                Ok(false)
            }
            Ok(_) => {
                tracing::warn!("CA file {} is empty, skip", path.display());
                Ok(false)
            }
            Err(e) => {
                tracing::warn!("failed to read CA file {}: {}", path.display(), e);
                Err(())
            }
        }
    }
}

fn looks_like_pem(bytes: &[u8]) -> bool {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    s.contains("-----BEGIN CERTIFICATE-----")
}

fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}
