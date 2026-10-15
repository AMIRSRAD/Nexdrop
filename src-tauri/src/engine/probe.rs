use std::path::Path;
use reqwest::header::{HeaderMap, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED};
use url::Url;
use crate::db::models::FileCategory;

pub struct ProbeResult {
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub total_bytes: i64,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime_type: Option<String>,
    pub category: FileCategory,
}

pub async fn probe_url(client: &reqwest::Client, url_str: &str) -> Result<ProbeResult, String> {
    let _parsed = Url::parse(url_str).map_err(|e| format!("Invalid URL: {e}"))?;

    // Attempt 1: HEAD request
    let head_res = client
        .head(url_str)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0")
        .send()
        .await;

    match head_res {
        Ok(res) if res.status().is_success() => {
            let final_url = res.url().to_string();
            let headers = res.headers().clone();
            parse_headers_into_probe(url_str, &final_url, &headers, None)
        }
        _ => {
            // Attempt 2: Fallback to GET with Range: bytes=0-0 to test range support & avoid downloading entire file
            let get_res = client
                .get(url_str)
                .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0")
                .header(reqwest::header::RANGE, "bytes=0-0")
                .send()
                .await
                .map_err(|e| format!("Network request failed: {e}"))?;

            let final_url = get_res.url().to_string();
            let status = get_res.status();
            let headers = get_res.headers().clone();

            // Status 206 Partial Content proves Range request is supported!
            let range_supported = status == reqwest::StatusCode::PARTIAL_CONTENT;
            parse_headers_into_probe(url_str, &final_url, &headers, Some(range_supported))
        }
    }
}

fn parse_headers_into_probe(
    orig_url: &str,
    final_url: &str,
    headers: &HeaderMap,
    range_override: Option<bool>,
) -> Result<ProbeResult, String> {
    // 1. Detect total size
    let mut total_bytes: i64 = -1;
    if let Some(range_val) = headers.get(CONTENT_RANGE).and_then(|h| h.to_str().ok()) {
        // Example: bytes 0-0/10485760 or bytes 0-1000/1000
        if let Some(slash_idx) = range_val.rfind('/') {
            let total_str = &range_val[slash_idx + 1..].trim();
            if let Ok(size) = total_str.parse::<i64>() {
                total_bytes = size;
            }
        }
    }

    if total_bytes <= 0 {
        if let Some(cl) = headers.get(CONTENT_LENGTH).and_then(|h| h.to_str().ok()) {
            if let Ok(size) = cl.parse::<i64>() {
                total_bytes = size;
            }
        }
    }

    // 2. Detect range / resumable support
    let mut resumable = range_override.unwrap_or(false);
    if !resumable {
        if let Some(ar) = headers.get(ACCEPT_RANGES).and_then(|h| h.to_str().ok()) {
            if ar.to_lowercase().contains("bytes") {
                resumable = true;
            }
        }
    }

    let etag = headers.get(ETAG).and_then(|h| h.to_str().ok()).map(|s| s.to_string());
    let last_modified = headers.get(LAST_MODIFIED).and_then(|h| h.to_str().ok()).map(|s| s.to_string());
    let mime_type = headers.get(CONTENT_TYPE).and_then(|h| h.to_str().ok()).map(|s| {
        s.split(';').next().unwrap_or("").trim().to_string()
    });

    // 3. Extract filename
    let mut filename = None;
    if let Some(cd) = headers.get(CONTENT_DISPOSITION).and_then(|h| h.to_str().ok()) {
        filename = parse_content_disposition(cd);
    }

    if filename.is_none() {
        if let Ok(u) = Url::parse(final_url) {
            if let Some(seg) = u.path_segments().and_then(|mut s| s.next_back()) {
                let decoded = urlencoding_decode(seg);
                if !decoded.trim().is_empty() && decoded != "/" {
                    filename = Some(decoded);
                }
            }
        }
    }

    let raw_name = filename.unwrap_or_else(|| "download.bin".to_string());
    let clean_filename = sanitize_filename(&raw_name);
    let category = FileCategory::from_filename_or_mime(&clean_filename, mime_type.as_deref());

    Ok(ProbeResult {
        url: orig_url.to_string(),
        final_url: final_url.to_string(),
        filename: clean_filename,
        total_bytes,
        resumable,
        etag,
        last_modified,
        mime_type,
        category,
    })
}

fn parse_content_disposition(disposition: &str) -> Option<String> {
    // Check filename*=UTF-8''... (RFC 5987)
    if let Some(pos) = disposition.find("filename*=") {
        let part = &disposition[pos + 10..];
        let raw = part.split(';').next()?.trim();
        if let Some(idx) = raw.find("''") {
            let encoded = &raw[idx + 2..];
            let decoded = urlencoding_decode(encoded.trim_matches('"'));
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }

    // Fallback to standard filename="..."
    if let Some(pos) = disposition.find("filename=") {
        let part = &disposition[pos + 9..];
        let val = part.split(';').next()?.trim().trim_matches('"').trim_matches('\'');
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }

    None
}

fn urlencoding_decode(s: &str) -> String {
    url::form_urlencoded::parse(s.as_bytes())
        .map(|(k, _)| k.into_owned())
        .collect::<Vec<String>>()
        .join("")
}

pub fn sanitize_filename(name: &str) -> String {
    let mut clean = name
        .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .to_string();

    // Prevent directory traversal
    while clean.contains("..") {
        clean = clean.replace("..", "_");
    }

    if clean.is_empty() || clean == "." {
        "download.bin".to_string()
    } else {
        clean
    }
}

pub fn resolve_unique_path(dir: &Path, filename: &str) -> std::path::PathBuf {
    let target = dir.join(filename);
    if !target.exists() {
        return target;
    }

    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());

    for i in 1..10000 {
        let new_name = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let p = dir.join(&new_name);
        if !p.exists() {
            return p;
        }
    }

    target
}
