use anyhow::{Context, anyhow};
use arc_swap::ArcSwap;
use http::Uri;
use rustls::sign::CertifiedKey;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, RwLock};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use tonic::transport::{Channel, ClientTlsConfig, Identity};
use tracing::{info, warn};

mod argon_config {
    include!("argon.config.rs");
}

use crate::argon_config::{
    Snapshot, WatchRequest, config_discovery_client, config_discovery_client::ConfigDiscoveryClient,
};
use crate::certs;
use crate::snapshot::RouteTable;

const CERT_CA_NAME: &str = "ca.crt";
const CERT_NAME: &str = "tls.crt";
const CERT_KEY_NAME: &str = "tls.key";

pub struct GrpcManager {
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
    certs_watcher_handle: tokio::task::JoinHandle<()>,
    ready: Arc<RwLock<bool>>,
    snapshot: Arc<RwLock<Snapshot>>,
    route_table: Arc<RwLock<Arc<RouteTable>>>,
    sni: Arc<ArcSwap<HashMap<String, Arc<CertifiedKey>>>>,
    ca_updated: Arc<Notify>,
    ca_pem: Arc<ArcSwap<Vec<u8>>>,
    client_pem_updated: Arc<Notify>,
    client_pem: Arc<ArcSwap<Vec<u8>>>,
    client_key_pem: Arc<ArcSwap<Vec<u8>>>,
}

impl GrpcManager {
    pub fn start(
        controller_addr: String,
        node_id: String,
        certs_dir: PathBuf,
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
        let client_pem_updated = Arc::new(Notify::new());
        let ca_pem: Arc<ArcSwap<Vec<u8>>> = Arc::new(ArcSwap::from_pointee(Vec::new()));
        let client_pem: Arc<ArcSwap<Vec<u8>>> = Arc::new(ArcSwap::from_pointee(Vec::new()));
        let client_key_pem: Arc<ArcSwap<Vec<u8>>> = Arc::new(ArcSwap::from_pointee(Vec::new()));
        let ca_pem_for_grpc_loop = ca_pem.clone();
        let client_pem_for_grpc_loop = client_pem.clone();
        let client_key_pem_for_grpc_loop = client_key_pem.clone();
        let ca_updated_for_grpc_loop = ca_updated.clone();
        let client_pem_for_updated_grpc_loop = client_pem_updated.clone();

        let certs_watcher_handle = {
            let cancel_clone = cancel.clone();
            let ca_updated_clone = ca_updated.clone();
            let client_pem_updated_clone = client_pem_updated.clone();
            let ca_pem_clone = ca_pem.clone();
            let client_pem_clone = client_pem.clone();
            let client_key_pem_clone = client_key_pem.clone();

            tokio::spawn(async move {
                let cert_ca_path = certs_dir.join(CERT_CA_NAME);
                let client_pem_path = certs_dir.join(CERT_NAME);
                let client_key_pem_path = certs_dir.join(CERT_KEY_NAME);
                loop {
                    tokio::select! {
                        _ = cancel_clone.cancelled() => {
                            info!("Cancelled watch certs");
                            break;
                        }

                        _ = sleep(Duration::from_secs(2)) => {
                            if let Ok(changed) = try_load_and_store(&cert_ca_path, &ca_pem_clone, "CA").await {
                                if changed {
                                    ca_updated_clone.notify_one();
                                }
                            }

                            if let Ok(changed) = try_load_and_store(&client_pem_path, &client_pem_clone, "client certificate").await {
                                if changed {
                                    client_pem_updated_clone.notify_one();
                                }
                            }

                            if let Ok(changed) = try_load_and_store(&client_key_pem_path, &client_key_pem_clone, "client key").await {
                                if changed {
                                    client_pem_updated_clone.notify_one();
                                }
                            }
                        }
                    }
                }
            })
        };

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
                    let ca_cert_bytes = ca_pem_for_grpc_loop.load().clone();
                    let client_cert_bytes = client_pem_for_grpc_loop.load().clone();
                    let client_key_bytes = client_key_pem_for_grpc_loop.load().clone();

                    if ca_cert_bytes.is_empty() {
                        return Err(anyhow!("CA certificate is not available yet"));
                    }

                    if client_cert_bytes.is_empty() || client_key_bytes.is_empty() {
                        return Err(anyhow!("Client certificate/key is not available yet"));
                    }

                    let ca = tonic::transport::Certificate::from_pem(&*ca_cert_bytes);

                    let identity = Identity::from_pem(
                        client_cert_bytes.as_ref().clone(),
                        client_key_bytes.as_ref().clone(),
                    );

                    let uri = controller_addr
                        .parse::<Uri>()
                        .context("controller address is not a valid URI")?;
                    let host = uri
                        .host()
                        .ok_or_else(|| anyhow!("controller address is missing host component"))?
                        .to_string();

                    let tls_config = ClientTlsConfig::new()
                        .ca_certificate(ca)
                        .identity(identity)
                        .domain_name(host);

                    let endpoint = Channel::from_shared(controller_addr.clone())
                        .context("failed to create gRPC endpoint from controller address")?;

                    let ch = endpoint
                        .tls_config(tls_config)
                        .context("failed to apply TLS client configuration")?
                        .connect()
                        .await
                        .context("gRPC connect attempt failed")?;

                    Ok::<Channel, anyhow::Error>(ch)
                };

                let channel = match timeout(Duration::from_secs(10), connect_fut).await {
                    Ok(Ok(ch)) => {
                        info!("gRPC connected: {}", controller_addr);
                        backoff_ms = 500;
                        ch
                    }
                    Ok(Err(e)) => {
                        warn!("gRPC connect failed: {:?}", e);
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
                        _ = ca_updated_for_grpc_loop.notified() => {
                            info!("CA updated — forcing reconnect to apply new TLS");
                            break; // Выходим из внутреннего цикла, чтобы переподключиться
                        }
                        _ = client_pem_for_updated_grpc_loop.notified() => {
                            info!("Client certificate/key updated — forcing reconnect to apply new TLS identity");
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
            certs_watcher_handle,
            ready,
            snapshot,
            route_table,
            sni,
            ca_updated,
            ca_pem,
            client_pem_updated,
            client_pem,
            client_key_pem,
        }
    }

    pub async fn shutdown(self) {
        self.cancel.cancel();
        let _ = tokio::join!(self.handle, self.certs_watcher_handle);
        info!("gRPC manager stopped");
    }

    pub async fn is_ready(&self) -> bool {
        *self.ready.read().await
    }

    pub async fn latest_snapshot(&self) -> Snapshot {
        self.snapshot.read().await.clone()
    }
}

async fn try_load_and_store(
    path: &Path,
    target: &Arc<ArcSwap<Vec<u8>>>,
    description: &str,
) -> Result<bool, ()> {
    match tokio::fs::read(path).await {
        Ok(new_bytes) if !new_bytes.is_empty() => {
            let current = target.load();

            if !bytes_eq(&new_bytes, &current) {
                target.store(Arc::new(new_bytes));
                tracing::info!("{} reloaded from {}", description, path.display());
                return Ok(true);
            }
            Ok(false)
        }

        Ok(_) => {
            tracing::warn!("{} file {} is empty, skip", description, path.display());
            Ok(false)
        }
        Err(e) => {
            tracing::warn!(
                "failed to read {} file {}: {}",
                description,
                path.display(),
                e
            );
            Err(())
        }
    }
}

fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    a == b
}
