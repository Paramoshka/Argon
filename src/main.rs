use crate::config::ServerConfig;
use crate::server::Server;

mod config;
mod server;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let server_config_list = ServerConfig::new().expect("Could not create server config");
    for server_config in server_config_list {
        let server = Server::new(server_config).await?;
        server.run().await?;
    }
    
    Ok(())
}