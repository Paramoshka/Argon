use std::arch::x86_64::_mm256_hsub_epi16;
use crate::config::ServerConfig;
use crate::http::Request;

pub struct Handle{}

impl Handle{
    pub async fn handle_request(server_config: &ServerConfig, request: &Request) -> Result<(), std::io::Error> {
        if let Some(host) = request.headers.get("Host") {
            let host = host.to_string();
            println!("Host: {}", host);
        }
        
    Ok(())
    }
}