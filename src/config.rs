use std::fs;
use std::path::Path;

pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ServerConfig {
    pub fn new() -> Result<Vec<ServerConfig>, std::io::Error> {
        let mut arr_server_config: Vec<ServerConfig> = Vec::new();
        let path = Path::new("./argon.conf");

        // Читаем файл, возвращая ошибку в случае неудачи
        let raw = fs::read_to_string(path)?;

        // Разделяем файл по строкам "server {"
        for block in raw.split("server {").skip(1) {
            let block = block.split_once('}').map(|(b, _)| b).unwrap_or("").trim();
            if block.is_empty() {
                continue;
            }

            let host = Self::parse_directive(block, "server_name")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server name"))?;

            let port_str = Self::parse_directive(block, "listen")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server port"))?;

            let port: u16 = port_str.trim()
                .parse()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Port is not a valid number"))?;

            arr_server_config.push(ServerConfig { host, port });
        }


        // Возвращаем пустой Vec, если нет серверов, или заполненный
        Ok(arr_server_config)
    }

    // Простая функция для извлечения значения директивы
    fn parse_directive(block: &str, directive: &str) -> Option<String> {
        block.lines()
            .map(str::trim)
            .find_map(|line| {
                let mut parts = line.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(key), Some(value)) if key == directive => {
                        Some(value.trim_end_matches(';').to_string())
                    }
                    _ => None,
                }
            })
    }


}

