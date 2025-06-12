use crate::config::ServerConfig;

mod config;
mod server;

fn main() {
    let server = ServerConfig::new().expect("Could not create server config");
    println!("{}", server[0].host)
}