use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use crate::config::ServerConfig;
use crate::http::{Handle, Request};

pub struct Server {
    tcp_listener: TcpListener,
    server_config: Arc<HashMap<String, ServerConfig>>
}

impl Server {
    // Get Array ServerConfig with the same port
    pub async fn new(port: u16, server_config: HashMap<String, ServerConfig>) -> Result<Self, std::io::Error> {
        match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(listener) => {
                // print virtual servers
                for server in &server_config {
                    println!("Server listening on port {}", server.1.host);
                }
                println!("✅ Server listening on 0.0.0.0:{}", port);
                Ok(Server {
                    tcp_listener: listener,
                    server_config: Arc::new(server_config),
                })
            },
            Err(e) => {
                eprintln!("❌ Failed to bind to port {}: {}", port, e);
                Err(e)
            }
        }
    }

    pub async fn run(&self) -> Result<(), std::io::Error> {
        loop {
            let (mut tcp_stream, addr) = self.tcp_listener.accept().await?;
            let config = Arc::clone(&self.server_config);

            tokio::task::spawn(async move {
                let mut buf = [0u8; 1024];
                match tcp_stream.read(&mut buf).await {
                    Ok(0) => return,
                    Ok(n) => {
                        if let Some(req) = Request::parse_request(&buf[..n]) {
                            if let Err(e) = Handle::handle_request(&mut tcp_stream, &config, &req).await {
                                eprintln!("<UNK> Failed to handle request: {}", e);
                            }
                        } else {
                            let _ = tcp_stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await;
                        }
                    }
                    Err(e) => {
                        eprintln!("failed to read from socket: {:?}", e);
                    }
                }
            });
        }
    }

}