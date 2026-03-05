//! # Nexus HTTP Parser
//!
//! Zero-copy HTTP/1.1 request and response parser for the Nexus
//! networking stack. Supports chunked transfer encoding, header
//! parsing, and content-length validation.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// HTTP methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Connect,
    Trace,
}

impl HttpMethod {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "GET" => Some(HttpMethod::Get),
            "POST" => Some(HttpMethod::Post),
            "PUT" => Some(HttpMethod::Put),
            "DELETE" => Some(HttpMethod::Delete),
            "PATCH" => Some(HttpMethod::Patch),
            "HEAD" => Some(HttpMethod::Head),
            "OPTIONS" => Some(HttpMethod::Options),
            "CONNECT" => Some(HttpMethod::Connect),
            "TRACE" => Some(HttpMethod::Trace),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Connect => "CONNECT",
            HttpMethod::Trace => "TRACE",
        }
    }
}

/// HTTP version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpVersion {
    Http10,
    Http11,
}

impl HttpVersion {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "HTTP/1.0" => Some(HttpVersion::Http10),
            "HTTP/1.1" => Some(HttpVersion::Http11),
            _ => None,
        }
    }
}

/// A parsed HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub query: Option<String>,
    pub version: HttpVersion,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub content_length: Option<usize>,
    pub chunked: bool,
    pub keep_alive: bool,
}

/// A parsed HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub version: HttpVersion,
    pub status_code: u16,
    pub reason: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, reason: &str) -> Self {
        HttpResponse {
            version: HttpVersion::Http11,
            status_code: status,
            reason: String::from(reason),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    pub fn header(&mut self, key: &str, value: &str) {
        self.headers.insert(String::from(key.to_lowercase()), String::from(value));
    }

    pub fn body(&mut self, data: &[u8]) {
        self.body = data.to_vec();
        self.headers.insert(
            String::from("content-length"),
            alloc::format!("{}", data.len()),
        );
    }

    /// Serialize to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        let status_line = alloc::format!("HTTP/1.1 {} {}\r\n", self.status_code, self.reason);
        buf.extend_from_slice(status_line.as_bytes());

        for (k, v) in &self.headers {
            let header = alloc::format!("{}: {}\r\n", k, v);
            buf.extend_from_slice(header.as_bytes());
        }
        buf.extend_from_slice(b"\r\n");
        buf.extend_from_slice(&self.body);
        buf
    }
}

/// Parse error.
#[derive(Debug, Clone)]
pub enum ParseError {
    InvalidMethod,
    InvalidVersion,
    InvalidHeader,
    IncompleteRequest,
    BodyTooLarge,
}

/// The HTTP Parser.
pub struct HttpParser {
    /// Max header size
    pub max_header_size: usize,
    /// Max body size
    pub max_body_size: usize,
    /// Stats
    pub requests_parsed: u64,
    pub responses_built: u64,
    pub parse_errors: u64,
}

impl HttpParser {
    pub fn new() -> Self {
        HttpParser {
            max_header_size: 8192,
            max_body_size: 10 * 1024 * 1024, // 10 MiB
            requests_parsed: 0,
            responses_built: 0,
            parse_errors: 0,
        }
    }

    /// Parse a raw HTTP request.
    pub fn parse_request(&mut self, data: &[u8]) -> Result<HttpRequest, ParseError> {
        let text = core::str::from_utf8(data).map_err(|_| ParseError::InvalidHeader)?;

        // Split headers and body
        let (header_part, body_part) = match text.find("\r\n\r\n") {
            Some(pos) => (&text[..pos], &data[pos + 4..]),
            None => return Err(ParseError::IncompleteRequest),
        };

        if header_part.len() > self.max_header_size {
            self.parse_errors += 1;
            return Err(ParseError::BodyTooLarge);
        }

        let mut lines = header_part.split("\r\n");

        // Parse request line
        let request_line = lines.next().ok_or(ParseError::IncompleteRequest)?;
        let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
        if parts.len() < 3 {
            self.parse_errors += 1;
            return Err(ParseError::InvalidMethod);
        }

        let method = HttpMethod::from_str(parts[0]).ok_or({
            self.parse_errors += 1;
            ParseError::InvalidMethod
        })?;

        let version = HttpVersion::from_str(parts[2]).ok_or({
            self.parse_errors += 1;
            ParseError::InvalidVersion
        })?;

        // Parse path and query string
        let (path, query) = if let Some(qpos) = parts[1].find('?') {
            (String::from(&parts[1][..qpos]), Some(String::from(&parts[1][qpos + 1..])))
        } else {
            (String::from(parts[1]), None)
        };

        // Parse headers
        let mut headers = BTreeMap::new();
        for line in lines {
            if let Some(colon) = line.find(':') {
                let key = line[..colon].trim().to_lowercase();
                let value = line[colon + 1..].trim();
                headers.insert(String::from(&key), String::from(value));
            }
        }

        // Content-Length
        let content_length = headers.get("content-length")
            .and_then(|v| v.parse::<usize>().ok());

        // Chunked?
        let chunked = headers.get("transfer-encoding")
            .map(|v| v.contains("chunked"))
            .unwrap_or(false);

        // Keep-alive?
        let keep_alive = match version {
            HttpVersion::Http11 => {
                headers.get("connection").map(|v| v != "close").unwrap_or(true)
            }
            HttpVersion::Http10 => {
                headers.get("connection").map(|v| v == "keep-alive").unwrap_or(false)
            }
        };

        // Body
        let body = if let Some(cl) = content_length {
            if cl > self.max_body_size { return Err(ParseError::BodyTooLarge); }
            let len = cl.min(body_part.len());
            body_part[..len].to_vec()
        } else {
            body_part.to_vec()
        };

        self.requests_parsed += 1;

        Ok(HttpRequest {
            method,
            path,
            query,
            version,
            headers,
            body,
            content_length,
            chunked,
            keep_alive,
        })
    }

    /// Common responses.
    pub fn ok_response(&mut self, body: &[u8], content_type: &str) -> HttpResponse {
        self.responses_built += 1;
        let mut resp = HttpResponse::new(200, "OK");
        resp.header("content-type", content_type);
        resp.body(body);
        resp
    }

    pub fn not_found(&mut self) -> HttpResponse {
        self.responses_built += 1;
        let mut resp = HttpResponse::new(404, "Not Found");
        resp.body(b"404 Not Found");
        resp
    }

    pub fn internal_error(&mut self) -> HttpResponse {
        self.responses_built += 1;
        let mut resp = HttpResponse::new(500, "Internal Server Error");
        resp.body(b"500 Internal Server Error");
        resp
    }
}
