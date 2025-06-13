use std::io::Write;
use std::ptr::write;

pub struct Response {
    pub method: String,
    pub status_code: u16,
    pub reason_phrase: String,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn to_bytes(&self) -> Result<Vec<u8>, std::io::Error> {
        let mut buf = Vec::<u8>::new();
        write!(&mut buf, "HTTP/1.1 {} {}\r\n", self.status_code, self.reason_phrase)?;
        for (k, v) in self.headers.iter() {
            write!(&mut buf, "{}: {}\r\n", k, v)?;
        }
        write!(&mut buf, "Content-Length: {}\r\n", self.body.len())?;
        write!(&mut buf, "\r\n")?;
        Ok(buf)
    }

}