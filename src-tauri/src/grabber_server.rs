//! Local bridge used by the browser extension (127.0.0.1:18888).
//!
//! Any web page can send requests to localhost, so the bridge only trusts
//! callers that a browser identifies as an *extension*: web pages cannot forge
//! the `Origin` header, and a page's own origin is never an extension origin.
//! Combined with a `Host` check (DNS-rebinding defence) and a JSON-only body
//! requirement, a malicious website cannot queue downloads, choose where files
//! are saved, or use Nexdrop to fetch pages from your network.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::db::models::AddDownloadRequest;
use crate::engine::DownloadManager;

const BRIDGE_ADDR: &str = "127.0.0.1:18888";
const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize, Serialize)]
pub struct GrabRequest {
    pub url: String,
    pub filename: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    // Accepted for compatibility but deliberately ignored: a caller must not
    // be able to pick the folder files are written to.
    #[serde(default)]
    pub destination_path: Option<String>,
    pub connections: Option<usize>,
    /// Optional hash the page published for the file (MD5 or SHA-256).
    #[serde(default)]
    pub expected_checksum: Option<String>,
    /// The browser's cookies for this address (a `Cookie` header value), so files
    /// behind a login download the same way they would in the browser.
    #[serde(default)]
    pub cookies: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PageParseRequest {
    pub url: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GrabbedLink {
    pub url: String,
    pub filename: String,
    pub category: String,
    pub extension: String,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct Reply {
    status: &'static str,
    body: String,
}

impl Reply {
    fn json(status: &'static str, value: serde_json::Value) -> Self {
        Self { status, body: value.to_string() }
    }
    fn error(status: &'static str, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": message }))
    }
}

pub struct GrabberServer;

impl GrabberServer {
    pub fn start(app_handle: AppHandle, manager: Arc<DownloadManager>) {
        tauri::async_runtime::spawn(async move {
            let listener = match TcpListener::bind(BRIDGE_ADDR).await {
                Ok(l) => {
                    log::info!("Browser bridge listening on http://{BRIDGE_ADDR}");
                    l
                }
                Err(e) => {
                    log::warn!("Failed to bind browser bridge on {BRIDGE_ADDR}: {e}");
                    return;
                }
            };

            loop {
                let Ok((socket, _)) = listener.accept().await else { continue };
                let app = app_handle.clone();
                let mgr = manager.clone();
                tauri::async_runtime::spawn(async move {
                    // A client that stalls must not hold a task forever.
                    let _ = tokio::time::timeout(Duration::from_secs(60), handle_connection(socket, app, mgr)).await;
                });
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, app: AppHandle, mgr: Arc<DownloadManager>) {
    let request = match tokio::time::timeout(IO_TIMEOUT, read_request(&mut socket)).await {
        Ok(Ok(req)) => req,
        Ok(Err(reply)) => {
            let _ = write_reply(&mut socket, reply, None).await;
            return;
        }
        Err(_) => return,
    };

    // DNS-rebinding defence: only our own loopback name is acceptable.
    let host_ok = request
        .headers
        .get("host")
        .map(|h| matches!(h.to_ascii_lowercase().as_str(), "127.0.0.1:18888" | "localhost:18888"))
        .unwrap_or(false);
    if !host_ok {
        let _ = write_reply(&mut socket, Reply::error("403 Forbidden", "Unexpected Host"), None).await;
        return;
    }

    let origin = request.headers.get("origin").cloned();
    let from_extension = origin.as_deref().map(is_extension_origin).unwrap_or(false);
    if origin.is_some() && !from_extension {
        // A web page (or anything else that isn't an extension) is talking to us.
        let _ = write_reply(&mut socket, Reply::error("403 Forbidden", "Origin not allowed"), None).await;
        return;
    }
    let allow_origin = if from_extension { origin.clone() } else { None };

    if request.method == "OPTIONS" {
        let reply = Reply { status: "204 No Content", body: String::new() };
        let _ = write_reply(&mut socket, reply, allow_origin.as_deref()).await;
        return;
    }

    let reply = route(&request, from_extension, &app, &mgr).await;
    let _ = write_reply(&mut socket, reply, allow_origin.as_deref()).await;
}

async fn route(req: &Request, from_extension: bool, app: &AppHandle, mgr: &Arc<DownloadManager>) -> Reply {
    match (req.method.as_str(), req.path.as_str()) {
        // Harmless read-only status, also used by the popup to show "connected".
        ("GET", "/health") | ("GET", "/status") => Reply::json(
            "200 OK",
            serde_json::json!({
                "name": "Nexdrop",
                "version": env!("CARGO_PKG_VERSION"),
                "status": "online",
                "grabber_active": true
            }),
        ),

        // The extension announcing itself (which browser, which version), so the app can tell
        // the user whether their browser is connected.
        ("POST", "/hello") => {
            if !from_extension {
                return Reply::error("403 Forbidden", "Only the Nexdrop browser extension may call this endpoint");
            }
            #[derive(Deserialize)]
            struct Hello {
                browser: String,
                #[serde(default)]
                version: String,
            }
            match serde_json::from_slice::<Hello>(&req.body) {
                Ok(h) => {
                    crate::browsers::note_hello(&h.browser, &h.version);
                    Reply::json("200 OK", serde_json::json!({ "success": true }))
                }
                Err(e) => Reply::error("400 Bad Request", &format!("Invalid JSON: {e}")),
            }
        }

        ("POST", "/download") | ("POST", "/parse-page") => {
            if !from_extension {
                return Reply::error("403 Forbidden", "Only the Nexdrop browser extension may call this endpoint");
            }
            let is_json = req
                .headers
                .get("content-type")
                .map(|c| c.to_ascii_lowercase().starts_with("application/json"))
                .unwrap_or(false);
            if !is_json {
                return Reply::error("415 Unsupported Media Type", "Expected application/json");
            }
            let body = String::from_utf8_lossy(&req.body);
            if req.path == "/download" {
                handle_download(&body, app, mgr).await
            } else {
                handle_parse_page(&body).await
            }
        }

        _ => Reply::error("404 Not Found", "Not Found"),
    }
}

async fn handle_download(body: &str, app: &AppHandle, mgr: &Arc<DownloadManager>) -> Reply {
    let grab = match serde_json::from_str::<GrabRequest>(body) {
        Ok(g) => g,
        Err(e) => return Reply::error("400 Bad Request", &format!("Invalid JSON: {e}")),
    };

    // Magnet links and .torrent files don't download straight away: the app asks
    // which files to take (and where to put them) before anything starts.
    let lower = grab.url.trim().to_lowercase();
    let torrent_file = url::Url::parse(grab.url.trim())
        .map(|u| matches!(u.scheme(), "http" | "https") && u.path().to_lowercase().ends_with(".torrent"))
        .unwrap_or(false);
    if lower.starts_with("magnet:?") || torrent_file {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.emit("torrent-requested", serde_json::json!({ "source": grab.url.trim() }));
        }
        return Reply::json("200 OK", serde_json::json!({ "success": true, "torrent": true }));
    }

    // Streaming video (HLS / DASH) has qualities to choose from, so it also goes through the app.
    if is_http_url(&grab.url) && crate::engine::stream::is_stream_url(&grab.url) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.emit(
                "stream-requested",
                serde_json::json!({ "url": grab.url.trim(), "referer": grab.referer, "user_agent": grab.user_agent }),
            );
        }
        return Reply::json("200 OK", serde_json::json!({ "success": true, "stream": true }));
    }

    if !is_http_url(&grab.url) {
        return Reply::error("400 Bad Request", "Only http and https links can be downloaded");
    }

    // Cookies belong to the page's login, never anywhere else: they travel with the
    // download as a private header that is only sent to the link's own host.
    let custom_headers: Option<HashMap<String, String>> = grab
        .cookies
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty() && c.len() <= 16 * 1024 && !c.contains(['\r', '\n']))
        .map(|c| HashMap::from([("Cookie".to_string(), c.to_string())]));

    // Like IDM: show the download window so the folder, name and queue can be chosen.
    if mgr.get_settings().browser_prompt {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
            let _ = window.emit(
                "download-requested",
                serde_json::json!({
                    "url": grab.url.trim(),
                    "filename": grab.filename,
                    "referer": grab.referer,
                    "user_agent": grab.user_agent,
                    "headers": custom_headers,
                    "expected_checksum": grab.expected_checksum,
                }),
            );
            return Reply::json("200 OK", serde_json::json!({ "success": true, "prompt": true }));
        }
    }

    let req = AddDownloadRequest {
        url: grab.url,
        filename: grab.filename,
        destination_path: None, // always the user's configured folder
        connections: grab.connections.map(|c| c.clamp(1, 32)),
        speed_limit: None,
        priority: Some(1),
        scheduled_time: None,
        user_agent: grab.user_agent,
        referer: grab.referer,
        custom_headers,
        expected_checksum: grab.expected_checksum,
        mirrors: None,
        min_chunk_size: None,
        proxy_mode: None,
        custom_proxy_url: None,
        queue_id: None,
        stream_variant: None,
    };

    match mgr.add_download(req).await {
        Ok(task) => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                let _ = window.emit("download-intercepted", &task);
            }
            Reply::json(
                "200 OK",
                serde_json::json!({ "success": true, "task_id": task.id, "filename": task.filename }),
            )
        }
        Err(e) => Reply::error("400 Bad Request", &e),
    }
}

async fn handle_parse_page(body: &str) -> Reply {
    let req = match serde_json::from_str::<PageParseRequest>(body) {
        Ok(r) => r,
        Err(e) => return Reply::error("400 Bad Request", &format!("Invalid JSON: {e}")),
    };
    if !is_http_url(&req.url) {
        return Reply::error("400 Bad Request", "Only http and https pages can be scanned");
    }
    match extract_links_from_page(&req.url).await {
        Ok(links) => Reply::json("200 OK", serde_json::json!({ "success": true, "links": links })),
        Err(e) => Reply::error("500 Internal Server Error", &e),
    }
}

fn is_extension_origin(origin: &str) -> bool {
    origin.starts_with("chrome-extension://") || origin.starts_with("moz-extension://")
}

fn is_http_url(s: &str) -> bool {
    url::Url::parse(s).map(|u| matches!(u.scheme(), "http" | "https")).unwrap_or(false)
}

/// Reads one HTTP request with hard limits on header and body size.
async fn read_request(socket: &mut TcpStream) -> Result<Request, Reply> {
    let mut data: Vec<u8> = Vec::with_capacity(2048);
    let mut buf = [0u8; 2048];

    let header_end = loop {
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        if data.len() > MAX_HEADER_BYTES {
            return Err(Reply::error("431 Request Header Fields Too Large", "Headers too large"));
        }
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return Err(Reply::error("400 Bad Request", "Incomplete request")),
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
    };

    let head = String::from_utf8_lossy(&data[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/").to_string();

    let mut headers = HashMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let content_length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Err(Reply::error("413 Payload Too Large", "Body too large"));
    }

    let mut body = data[header_end..].to_vec();
    while body.len() < content_length {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return Err(Reply::error("400 Bad Request", "Incomplete body")),
            Ok(n) => body.extend_from_slice(&buf[..n]),
        }
    }
    body.truncate(content_length);

    Ok(Request { method, path, headers, body })
}

async fn write_reply(socket: &mut TcpStream, reply: Reply, allow_origin: Option<&str>) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\n", reply.status);
    if let Some(origin) = allow_origin {
        head.push_str(&format!(
            "Access-Control-Allow-Origin: {origin}\r\n\
             Vary: Origin\r\n\
             Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
             Access-Control-Allow-Headers: Content-Type\r\n\
             Access-Control-Max-Age: 600\r\n"
        ));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", reply.body.len()));
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(reply.body.as_bytes()).await?;
    socket.flush().await
}

// ─────────────────────────── page link extraction ───────────────────────────

pub async fn extract_links_from_page(target_url: &str) -> Result<Vec<GrabbedLink>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let mut res = client
        .get(target_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch web page: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("The page returned HTTP {}", res.status()));
    }
    // A direct link to a big file isn't a page to scan; don't pull it into memory.
    if let Some(ct) = res.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        let ct = ct.to_ascii_lowercase();
        if !(ct.starts_with("text/") || ct.contains("html") || ct.contains("xml") || ct.contains("json")) {
            return Err("That link points to a file, not a web page".to_string());
        }
    }

    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| format!("Failed to read page: {e}"))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_PAGE_BYTES {
            return Err("The page is too large to scan".to_string());
        }
    }
    let html = String::from_utf8_lossy(&bytes);

    let base_url = url::Url::parse(target_url).map_err(|e| e.to_string())?;
    let mut links = Vec::new();
    let mut seen_urls = std::collections::HashSet::new();

    // Scan for href="..." and src="..."
    for token in html.split(&['"', '\''][..]) {
        let trimmed = token.trim();
        if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || (trimmed.starts_with('/') && trimmed.len() > 1 && !trimmed.starts_with("//"))
            || (trimmed.contains('.') && !trimmed.contains(' ') && trimmed.len() > 3)
        {
            if let Ok(resolved) = base_url.join(trimmed) {
                let full_url = resolved.to_string();
                if full_url.starts_with("http") && seen_urls.insert(full_url.clone()) {
                    let path = resolved.path();
                    let extension = std::path::Path::new(path)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    if is_downloadable_extension(&extension) {
                        let filename = resolved
                            .path_segments()
                            .and_then(|mut segs| segs.next_back())
                            .unwrap_or("download.bin")
                            .to_string();

                        let category = categorize_extension(&extension);

                        links.push(GrabbedLink {
                            url: full_url,
                            filename: if filename.is_empty() { "file.bin".to_string() } else { filename },
                            category,
                            extension,
                        });
                    }
                }
            }
        }
    }

    Ok(links)
}

fn is_downloadable_extension(ext: &str) -> bool {
    matches!(
        ext,
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "dmg" |
        "exe" | "msi" | "apk" | "deb" | "rpm" | "bin" |
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "ts" | "m3u8" |
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" |
        "pdf" | "epub" | "docx" | "xlsx" | "pptx" | "csv" |
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "ico"
    )
}

fn categorize_extension(ext: &str) -> String {
    match ext {
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "iso" | "dmg" => "compressed".to_string(),
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "ts" | "m3u8" => "video".to_string(),
        "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" => "music".to_string(),
        "exe" | "msi" | "apk" | "deb" | "rpm" => "programs".to_string(),
        "pdf" | "epub" | "docx" | "xlsx" | "pptx" | "csv" => "documents".to_string(),
        _ => "other".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_extension_origins_are_trusted() {
        assert!(is_extension_origin("chrome-extension://abcdefghijklmnop"));
        assert!(is_extension_origin("moz-extension://1234-5678"));
        assert!(!is_extension_origin("https://evil.example"));
        assert!(!is_extension_origin("http://127.0.0.1:18888"));
        assert!(!is_extension_origin("null"));
    }

    #[test]
    fn rejects_non_http_links() {
        assert!(is_http_url("https://example.com/file.zip"));
        assert!(!is_http_url("file:///C:/Windows/system.ini"));
        assert!(!is_http_url("javascript:alert(1)"));
        assert!(!is_http_url("not a url"));
    }
}
