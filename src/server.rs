use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpSocket};
use crate::config::ServerConfig;
use crate::http::{Handle, Request};

pub struct Server {
    tcp_listener: TcpListener,
    server_config: Arc<ServerConfig>
}

impl Server {
    pub async fn new(server_config: ServerConfig) -> Result<Self, std::io::Error> {
        match TcpListener::bind(("0.0.0.0", server_config.port)).await {
            Ok(listener) => {
                println!("✅ Server listening on 0.0.0.0:{}", server_config.port);
                Ok(Server {
                    tcp_listener: listener,
                    server_config: Arc::new(server_config),
                })
            },
            Err(e) => {
                eprintln!("❌ Failed to bind to port {}: {}", server_config.port, e);
                Err(e)
            }
        }
    }
    
    pub async fn run(&self) -> Result<(), std::io::Error> {
        loop {
            let (mut tcp_stream, addr) = self.tcp_listener.accept().await?;
            let config = Arc::clone(&self.server_config);
            // like as goroutine
            tokio::task::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    let n = match tcp_stream.read(&mut buf).await {
                        Ok(0) => return,
                        Ok(n) => {
                            if let Some(req) = Request::parse_request(&buf[..n]) {
                                if let Err(e) = Handle::handle_request(&config, &req).await {
                                    eprintln!("<UNK> Failed to handle request: {}", e);
                                }
                            }
                        },
                        Err(e) => {
                            println!("failed to read from socket; err = {:?}", e);
                            return ;
                        }
                    };
                }
            });
        }
    }
}