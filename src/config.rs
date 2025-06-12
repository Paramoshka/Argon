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
        for block in raw.split("server {") {
            // Пропускаем пустые блоки или строки до первого "server"
            let block = block.trim();
            if block.is_empty() || !block.contains('}') {
                continue;
            }

            // Извлекаем host и port
            let host = ServerConfig::parse_directive(block, "server_name")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server name"))?;
            let port_str = ServerConfig::parse_directive(block, "listen")
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Error parsing server port"))?;

            // Парсим port в u16
            let port: u16 = port_str.trim()
                .parse()
                .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Port is not a valid number"))?;

            // Создаем структуру и добавляем в массив
            let server = ServerConfig {
                host,
                port,
            };
            arr_server_config.push(server);
        }

        // Возвращаем пустой Vec, если нет серверов, или заполненный
        Ok(arr_server_config)
    }

    // Простая функция для извлечения значения директивы
    fn parse_directive(block: &str, directive: &str) -> Option<String> {
        for line in block.lines() {
            let line = line.trim();
            if line.starts_with(directive) {
                // Извлекаем значение после директивы, убирая пробелы и точку с запятой
                return Some(
                    line[directive.len()..]
                        .trim()
                        .trim_end_matches(';')
                        .to_string(),
                );
            }
        }
        None
    }
}

