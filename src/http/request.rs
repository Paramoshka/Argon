
use std::collections::HashMap;

pub struct Request {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn parse_request(data: &[u8]) -> Option<Request> {
        let raw_text = String::from_utf8_lossy(data);

        // Divide headers and body
        let parts: Vec<&str> = raw_text.split("\r\n\r\n").collect();
        let header_part = parts.get(0)?;
        let body_part = parts.get(1).unwrap_or(&"");

        let mut lines = header_part.lines();

        // first line: "GET / HTTP/1.1"
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();
        let version = parts.next()?.to_string();

        // Headers
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((key, value)) = line.split_once(":") {
                headers.insert(
                    key.trim().to_string(),
                    value.trim().to_string(),
                );
            }
        }

        // Body
        let body = data
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| &data[i + 4..])
            .unwrap_or(&[])
            .to_vec();

        Some(Request {
            method,
            path,
            version,
            headers,
            body,
        })
    }
}