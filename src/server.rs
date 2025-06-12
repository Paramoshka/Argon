use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpSocket};
use crate::config::ServerConfig;

pub struct Server {
    tcp_listener: TcpListener,
    server_config: ServerConfig
}

impl Server {
    pub async fn new(server_config: ServerConfig) -> Result<Self, std::io::Error> {
        match TcpListener::bind(("0.0.0.0", server_config.port)).await {
            Ok(listener) => {
                println!("✅ Server listening on 0.0.0.0:{}", server_config.port);
                Ok(Server {
                    tcp_listener: listener,
                    server_config,
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
            tokio::task::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    let n = match tcp_stream.read(&mut buf).await {
                        Ok(0) => return,
                        Ok(n) => {
                            println!("Read {} bytes", n);
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