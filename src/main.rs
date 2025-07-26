use std::collections::HashMap;
use crate::config::ServerConfig;
use crate::server::Server;

mod config;
mod server;
mod http;

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    let server_config_list = ServerConfig::new().expect("Could not create server config");
    let sort_config = sort_server_config(server_config_list);
    for (port, server_config) in sort_config {
        for (host, server_config) in server_config {
            println!("Server config found at {}:{}", server_config.host, server_config.port);
        }
    }
    // for server_config in server_config_list {
    //     tokio::spawn(async move {
    //         match Server::new(server_config).await {
    //             Ok(server) => {
    //                 if let Err(e) = server.run().await {
    //                     eprintln!("Server on port  failed: {}", e);
    //                 }
    //             }
    //             Err(e) => {
    //                 eprintln!("Failed to start server on port {}:", e);
    //             }
    //         }
    //     });
    // }
    futures::future::pending::<()>().await;
    Ok(())
}

// We are sorting ServerConfig, and removing duplicate ports in order to run only one Server instance at a time.
fn sort_server_config(server_config_list: Vec<ServerConfig>) -> HashMap<u16, HashMap<String, ServerConfig>> {
    let mut sorted_server_config: HashMap<u16, HashMap<String, ServerConfig>> = HashMap::new();

    for server_config in server_config_list {
        let port = server_config.port;
        let host = server_config.host.clone();

        sorted_server_config
            .entry(port)
            .or_insert_with(HashMap::new)
            .insert(host, server_config);
    }

    sorted_server_config
}