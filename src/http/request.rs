use std::str::Lines;
use crate::http::response::Response;

pub struct Request {
    pub method: String,
    pub path: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn parse_request(data: &[u8]) -> Option<Request> {
        let raw_text = String::from_utf8_lossy(data);
        let mut lines: Lines = raw_text.lines();
        
        // Первая строка: "GET / HTTP/1.1"
        let request_line = lines.next()?;
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_string();
        let path = parts.next()?.to_string();
        let version = parts.next()?.to_string();
        
        let headers = Vec::new();
        let body = Vec::new();
        
        Some(Request{
            body,
            method,
            path,
            version,
            headers,
        })
    }
}