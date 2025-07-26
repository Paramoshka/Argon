use std::collections::HashMap;
use crate::config::ServerConfig;
use crate::http::Request;

pub struct Handle{}

use tokio::io::AsyncWriteExt;

impl Handle {
    pub async fn handle_request(
        stream: &mut tokio::net::TcpStream,
        server_config: &HashMap<String, ServerConfig>,
        request: &Request,
    ) -> Result<(), std::io::Error> {
        // parse host
        let host = match request.headers.get("Host") {
            Some(h) => h.split(':').next().unwrap_or(h),
            None => {
                stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n").await?;
                return Ok(());
            }
        };

        let sc = match server_config.get(host) {
            Some(cfg) => cfg,
            None => {
                stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await?;
                return Ok(());
            }
        };

        let path = &request.path;
        if let Some(location) = sc.paths.get(path) {
            println!("Proxy to {}", location.proxy_pass);
        } else {
            stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await?;
            return Ok(());
        }

        Ok(())
    }
}

