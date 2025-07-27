use std::collections::HashMap;
use std::fs;
use std::path::Path;
use crate::config::helpers::parse_directive;
use crate::config::location::Location;
use crate::config::server::ServerBlock;

pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub paths: HashMap<String, Location>,
}

impl ServerConfig {
    pub(crate) fn new() -> Result<Vec<ServerConfig>, std::io::Error> {
        let mut arr_server_config: Vec<ServerConfig> = Vec::new();
        let path = Path::new("./argon.conf");
        let raw = fs::read_to_string(path)?;
        let raw_blocks = ServerBlock::extract_server_blocks(&raw);
        for raw_block in raw_blocks {
            
            let host = parse_directive(&raw_block, "server_name")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server name"))?;

            let port_str = parse_directive(&raw_block, "listen")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server port"))?;

            let port: u16 = port_str.trim()
                .parse()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Port is not a valid number"))?;

            let raw_locations = Location::extract_location_blocks(&raw_block);
            
            let mut locations: HashMap<String, Location> = HashMap::new();
            
            if raw_locations.len() > 0 {
                for raw_location in raw_locations {

                    let path = Location::extract_location_path(&raw_location)
                        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing location"))?;

                    let prox_pass = parse_directive(&raw_location, "proxy_pass")
                        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing proxy password"))?;
                    
                    locations.insert(path, Location{proxy_pass: prox_pass});
                }
            }

            
            arr_server_config.push(ServerConfig{host, port, paths: locations});
            
           
        }
        Ok(arr_server_config)
    }
}