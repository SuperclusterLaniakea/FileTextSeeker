/// HTTP Server - allows searching and accessing files via web browser

use std::sync::{Arc, RwLock};
use std::path::Path;
use tiny_http::{Server, Response, Header};
use crate::file_seeker::engine::Engine;
use crate::file_seeker::types::{SearchOptions, SortSpec, SortField, SortOrder};

/// HTTP server for remote search
pub struct HttpServer {
    engine: Arc<Engine>,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    running: bool,
    download_enabled: bool,
}

impl HttpServer {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            port: 8080,
            username: None,
            password: None,
            running: false,
            download_enabled: true,
        }
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_auth(mut self, username: String, password: String) -> Self {
        self.username = Some(username);
        self.password = Some(password);
        self
    }

    pub fn with_download(mut self, enabled: bool) -> Self {
        self.download_enabled = enabled;
        self
    }

    /// Start the HTTP server (blocks until stopped)
    pub fn start(&mut self) -> Result<(), String> {
        let addr = format!("0.0.0.0:{}", self.port);
        let server = Server::http(&addr)
            .map_err(|e| format!("Failed to start HTTP server: {}", e))?;

        self.running = true;
        println!("HTTP server started on http://0.0.0.0:{}", self.port);

        for request in server.incoming_requests() {
            if !self.running {
                break;
            }
            self.handle_request(request);
        }

        Ok(())
    }

    pub fn stop(&mut self) {
        self.running = false;
    }

    fn handle_request(&self, request: tiny_http::Request) {
        let url = request.url().to_string();
        let method = request.method().as_str().to_string();

        // Parse query from URL
        let (path, query) = if let Some(pos) = url.find('?') {
            (&url[..pos], &url[pos + 1..])
        } else {
            (&url[..], "")
        };

        // Check auth
        if !self.check_auth(&request) {
            let response = Response::from_string("401 Unauthorized")
                .with_status_code(401);
            let _ = request.respond(response);
            return;
        }

        match (method.as_str(), path) {
            ("GET", "/") | ("GET", "/index.html") => {
                self.serve_search_page(request, query);
            }
            ("GET", "/search") => {
                self.handle_search_api(request, query);
            }
            ("GET", path) if path.starts_with("/download/") && self.download_enabled => {
                self.serve_file(request, &path[10..]);
            }
            _ => {
                let response = Response::from_string("Not Found")
                    .with_status_code(404);
                let _ = request.respond(response);
            }
        }
    }

    fn check_auth(&self, request: &tiny_http::Request) -> bool {
        if self.username.is_none() {
            return true;
        }

        let auth_header = request.headers().iter()
            .find(|h| h.field.as_str().to_ascii_lowercase() == "authorization");

        if let Some(header) = auth_header {
            let value = header.value.as_str();
            if value.starts_with("Basic ") {
                let decoded = base64_decode(&value[6..]);
                let parts: Vec<&str> = decoded.split(':').collect();
                if parts.len() == 2 {
                    return parts[0] == self.username.as_deref().unwrap_or("")
                        && parts[1] == self.password.as_deref().unwrap_or("");
                }
            }
        }

        false
    }

    fn serve_search_page(&self, request: tiny_http::Request, query: &str) {
        let search_query = query.split('&')
            .find(|p| p.starts_with("q="))
            .map(|p| url_decode(&p[2..]))
            .unwrap_or_default();

        let results_html = if !search_query.is_empty() {
            let mut options = SearchOptions::default();
            options.max_results = 100;
            let results = self.engine.search(&search_query, &options)
                .unwrap_or_default();
            Self::format_results_html(&results)
        } else {
            String::new()
        };

        let html = format!(
            r#"<!DOCTYPE html>
<html><head><title>文件检索助手- HTTP Server</title>
<meta charset="UTF-8">
<style>
body {{ font-family: Arial, sans-serif; margin: 20px; background: #f5f5f5; }}
.container {{ max-width: 900px; margin: 0 auto; background: white; padding: 20px; border-radius: 8px; box-shadow: 0 2px 4px rgba(0,0,0,0.1); }}
h1 {{ color: #333; font-size: 24px; }}
.search-form {{ margin: 20px 0; display: flex; }}
.search-form input[type=text] {{ flex: 1; padding: 10px; font-size: 16px; border: 1px solid #ddd; border-radius: 4px 0 0 4px; }}
.search-form input[type=submit] {{ padding: 10px 20px; background: #0078d4; color: white; border: none; border-radius: 0 4px 4px 0; cursor: pointer; font-size: 16px; }}
.search-form input[type=submit]:hover {{ background: #106ebe; }}
table {{ width: 100%; border-collapse: collapse; }}
th, td {{ padding: 8px 12px; text-align: left; border-bottom: 1px solid #ddd; }}
th {{ background: #f0f0f0; }}
tr:hover {{ background: #f9f9f9; }}
a {{ color: #0078d4; text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
.info {{ color: #666; margin-bottom: 10px; }}
</style></head><body>
<div class="container">
<h1>文件检索助手/h1>
<form class="search-form" method="get" action="/">
<input type="text" name="q" placeholder="Search..." value="{}" autofocus>
<input type="submit" value="Search">
</form>
<div class="info">Total indexed: {} files, {} folders</div>
{}
</div></body></html>"#,
            search_query,
            self.engine.total_file_count(),
            self.engine.total_folder_count(),
            results_html
        );

        let response = Response::from_string(html)
            .with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap());
        let _ = request.respond(response);
    }

    fn handle_search_api(&self, request: tiny_http::Request, query: &str) {
        let search_query = query.split('&')
            .find(|p| p.starts_with("q="))
            .map(|p| url_decode(&p[2..]))
            .unwrap_or_default();

        let mut options = SearchOptions::default();
        options.max_results = 100;
        let results = self.engine.search(&search_query, &options)
            .unwrap_or_default();

        // Build JSON response
        let json = serde_json::to_string_pretty(&results.iter().map(|e| {
            serde_json::json!({
                "name": e.file_name,
                "path": e.full_path.to_string_lossy(),
                "size": e.size,
                "is_directory": e.is_directory,
            })
        }).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".to_string());

        let response = Response::from_string(json)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
        let _ = request.respond(response);
    }

    fn serve_file(&self, request: tiny_http::Request, file_path: &str) {
        let path = Path::new(file_path);
        if !path.exists() || !path.is_file() {
            let response = Response::from_string("File not found")
                .with_status_code(404);
            let _ = request.respond(response);
            return;
        }

        match std::fs::read(path) {
            Ok(data) => {
                let response = Response::from_data(data);
                let _ = request.respond(response);
            }
            Err(_) => {
                let response = Response::from_string("Error reading file")
                    .with_status_code(500);
                let _ = request.respond(response);
            }
        }
    }

    fn format_results_html(results: &[crate::file_seeker::types::FileEntry]) -> String {
        if results.is_empty() {
            return "<p>No results found.</p>".to_string();
        }

        let mut html = format!("<p>Found {} results.</p>\n<table>\n<tr><th>Name</th><th>Path</th><th>Size</th><th>Date Modified</th></tr>\n", results.len());

        for entry in results.iter().take(100) {
            let icon = if entry.is_directory { "馃搧" } else { "馃搫" };
            let size_str = crate::file_seeker::engine::indexer::format_size(entry.size);
            let date_str = entry.date_modified
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();

            html.push_str(&format!(
                "<tr><td>{} {}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                icon,
                html_escape(&entry.file_name),
                html_escape(&entry.parent_path.to_string_lossy()),
                size_str,
                date_str
            ));
        }

        html.push_str("</table>\n");
        html
    }
}

fn base64_decode(input: &str) -> String {
    // Simple base64 decode (UTF-8 subset)
    use std::str;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes: Vec<u8> = input.bytes()
        .filter(|&c| c != b'\n' && c != b'\r' && c != b'=')
        .map(|c| {
            CHARS.iter().position(|&x| x == c).unwrap_or(0) as u8
        })
        .collect();

    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        out.push((chunk[0] << 2) | (chunk[1] >> 4));
        if chunk.len() > 2 {
            out.push((chunk[1] << 4) | (chunk[2] >> 2));
        }
        if chunk.len() > 3 {
            out.push((chunk[2] << 6) | chunk[3]);
        }
    }

    String::from_utf8_lossy(&out).to_string()
}

fn url_decode(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.bytes();
    while let Some(c) = chars.next() {
        match c {
            b'+' => result.push(' '),
            b'%' => {
                let hi = chars.next().and_then(|c| hex_val(c));
                let lo = chars.next().and_then(|c| hex_val(c));
                if let (Some(h), Some(l)) = (hi, lo) {
                    result.push((h << 4 | l) as char);
                }
            }
            _ => result.push(c as char),
        }
    }
    result
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

