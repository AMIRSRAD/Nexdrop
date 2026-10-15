use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tauri::{AppHandle, Emitter, Manager};
use serde::{Deserialize, Serialize};
use crate::engine::DownloadManager;
use crate::db::models::AddDownloadRequest;

#[derive(Debug, Deserialize, Serialize)]
pub struct GrabRequest {
    pub url: String,
    pub filename: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub destination_path: Option<String>,
    pub connections: Option<usize>,
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

pub struct GrabberServer;

impl GrabberServer {
    pub fn start(app_handle: AppHandle, manager: Arc<DownloadManager>) {
        tauri::async_runtime::spawn(async move {
            let listener = match TcpListener::bind("127.0.0.1:18888").await {
                Ok(l) => {
                    log::info!("Browser Link Grabber server listening on http://127.0.0.1:18888");
                    l
                }
                Err(e) => {
                    log::warn!("Failed to bind Browser Link Grabber on port 18888: {}", e);
                    return;
                }
            };

            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let app_clone = app_handle.clone();
                    let mgr_clone = manager.clone();

                    tauri::async_runtime::spawn(async move {
                        let mut raw_data = Vec::with_capacity(4096);
                        let mut temp_buf = [0u8; 2048];

                        // Read headers until end-of-headers delimiter
                        loop {
                            let n = match socket.read(&mut temp_buf).await {
                                Ok(n) if n > 0 => n,
                                _ => break,
                            };
                            raw_data.extend_from_slice(&temp_buf[..n]);
                            if raw_data.windows(4).any(|w| w == b"\r\n\r\n") || raw_data.windows(2).any(|w| w == b"\n\n") {
                                break;
                            }
                        }

                        if raw_data.is_empty() {
                            return;
                        }

                        let header_end = raw_data
                            .windows(4)
                            .position(|w| w == b"\r\n\r\n")
                            .map(|p| p + 4)
                            .or_else(|| raw_data.windows(2).position(|w| w == b"\n\n").map(|p| p + 2))
                            .unwrap_or(raw_data.len());

                        let header_str = String::from_utf8_lossy(&raw_data[..header_end]);
                        let first_line = header_str.lines().next().unwrap_or("");
                        let parts: Vec<&str> = first_line.split_whitespace().collect();
                        let method = parts.get(0).copied().unwrap_or("GET").to_string();
                        let path = parts.get(1).copied().unwrap_or("/").to_string();

                        // Handle CORS preflight
                        if method == "OPTIONS" {
                            let response = "HTTP/1.1 204 No Content\r\n\
                                            Access-Control-Allow-Origin: *\r\n\
                                            Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                                            Access-Control-Allow-Headers: Content-Type, Authorization, X-Requested-With\r\n\
                                            Access-Control-Max-Age: 86400\r\n\
                                            Content-Length: 0\r\n\r\n";
                            let _ = socket.write_all(response.as_bytes()).await;
                            return;
                        }

                        let content_length: usize = header_str
                            .lines()
                            .find(|l| l.to_lowercase().starts_with("content-length:"))
                            .and_then(|l| l.split(':').nth(1))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);

                        let mut body_bytes = raw_data[header_end..].to_vec();
                        while body_bytes.len() < content_length {
                            let n = match socket.read(&mut temp_buf).await {
                                Ok(n) if n > 0 => n,
                                _ => break,
                            };
                            body_bytes.extend_from_slice(&temp_buf[..n]);
                        }

                        let body = String::from_utf8_lossy(&body_bytes).to_string();

                        let (status_code, response_body) = if path == "/health" || path == "/status" {
                            (
                                "200 OK",
                                serde_json::json!({
                                    "name": "Nexdrop",
                                    "version": "0.1.0",
                                    "status": "online",
                                    "grabber_active": true
                                })
                                .to_string(),
                            )
                        } else if path == "/download" && method == "POST" {
                            match serde_json::from_str::<GrabRequest>(&body) {
                                Ok(grab) => {
                                    let req = AddDownloadRequest {
                                        url: grab.url.clone(),
                                        filename: grab.filename,
                                        destination_path: grab.destination_path,
                                        connections: grab.connections,
                                        speed_limit: None,
                                        priority: Some(1),
                                        scheduled_time: None,
                                        user_agent: grab.user_agent,
                                        referer: grab.referer,
                                        custom_headers: None,
                                        expected_checksum: None,
                                        mirrors: None,
                                        min_chunk_size: None,
                                        proxy_mode: None,
                                        custom_proxy_url: None,
                                    };

                                    match mgr_clone.add_download(req).await {
                                        Ok(task) => {
                                            // Bring main window to focus
                                            if let Some(window) = app_clone.get_webview_window("main") {
                                                let _ = window.show();
                                                let _ = window.unminimize();
                                                let _ = window.set_focus();
                                                let _ = window.emit("download-intercepted", &task);
                                            }

                                            (
                                                "200 OK",
                                                serde_json::json!({
                                                    "success": true,
                                                    "task_id": task.id,
                                                    "filename": task.filename
                                                })
                                                .to_string(),
                                            )
                                        }
                                        Err(e) => (
                                            "400 Bad Request",
                                            serde_json::json!({ "error": e }).to_string(),
                                        ),
                                    }
                                }
                                Err(e) => (
                                    "400 Bad Request",
                                    serde_json::json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
                                ),
                            }
                        } else if path == "/parse-page" && method == "POST" {
                            match serde_json::from_str::<PageParseRequest>(&body) {
                                Ok(req) => match extract_links_from_page(&req.url).await {
                                    Ok(links) => (
                                        "200 OK",
                                        serde_json::json!({ "success": true, "links": links }).to_string(),
                                    ),
                                    Err(e) => (
                                        "500 Internal Error",
                                        serde_json::json!({ "error": e }).to_string(),
                                    ),
                                },
                                Err(e) => (
                                    "400 Bad Request",
                                    serde_json::json!({ "error": format!("Invalid JSON: {}", e) }).to_string(),
                                ),
                            }
                        } else {
                            (
                                "404 Not Found",
                                serde_json::json!({ "error": "Not Found" }).to_string(),
                            )
                        };

                        let response = format!(
                            "HTTP/1.1 {}\r\n\
                            Content-Type: application/json\r\n\
                            Access-Control-Allow-Origin: *\r\n\
                            Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                            Access-Control-Allow-Headers: Content-Type, Authorization\r\n\
                            Content-Length: {}\r\n\r\n{}",
                            status_code,
                            response_body.len(),
                            response_body
                        );

                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            }
        });
    }
}


pub async fn extract_links_from_page(target_url: &str) -> Result<Vec<GrabbedLink>, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    let html = client
        .get(target_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch web page: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Failed to read page text: {}", e))?;

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
