use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use tonic::transport::Channel;

mod argon_config {
    include!("argon.config.rs");
}

use crate::argon_config::{
    config_discovery_client,
    config_discovery_client::ConfigDiscoveryClient,
    Snapshot,
    WatchRequest,
};
use crate::snapshot::RouteTable;

pub struct GrpcManager {
    cancel: CancellationToken,
    handle: tokio::task::JoinHandle<()>,
    ready: Arc<RwLock<bool>>,
    snapshot: Arc<RwLock<Snapshot>>,
    route_table: Arc<RwLock<Arc<RouteTable>>>,
}

impl GrpcManager {
    pub fn start(
        controller_addr: String,
        node_id: String,
        ready: Arc<RwLock<bool>>,
        snapshot: Arc<RwLock<Snapshot>>,
        route_table: Arc<RwLock<Arc<RouteTable>>>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let cancel_child = cancel.clone();

        let ready_for_task = ready.clone();
        let snapshot_for_task = snapshot.clone();
        let route_table_for_task = route_table.clone();

        let handle = tokio::spawn(async move {
            let mut backoff_ms: u64 = 500;
            let backoff_max: u64 = 10_000;

            loop {
                if cancel_child.is_cancelled() {
                    info!("gRPC manager: cancellation requested, exit loop");
                    break;
                }

                // отдельный async-блок, возвращающий Result<Channel, String>
                let connect_fut = async {
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

                // открываем стрим Watch
                let mut stream = match client.watch(WatchRequest {
                    node_id: node_id.clone(),
                }).await {
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

                // читаем поток снапшотов
                loop {
                    if cancel_child.is_cancelled() {
                        info!("gRPC manager: cancellation while streaming");
                        return;
                    }

                    match stream.message().await {
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

                            if !got_first {
                                got_first = true;
                                info!(
                                    "received first snapshot: version={}, routes={}, clusters={}",
                                    snap.version, snap.routes.len(), snap.clusters.len()
                                );
                            } else {
                                info!(
                                    "snapshot update: version={}, routes={}, clusters={}",
                                    snap.version, snap.routes.len(), snap.clusters.len()
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

                // строгий режим — гасить ready при потере стрима:
                // {
                //     let mut r = ready.write().await;
                //     *r = false;
                // }

                sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(backoff_max);
                info!("reconnecting...");
            }
        });

        Self { cancel, handle, ready, snapshot,  route_table}
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
}