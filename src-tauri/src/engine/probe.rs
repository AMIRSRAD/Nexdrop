use std::path::Path;
use reqwest::header::{HeaderMap, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, LAST_MODIFIED};
use url::Url;
use crate::db::models::FileCategory;

#[derive(Clone)]
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

/// Request details that some servers insist on before they'll even describe a file.
#[derive(Default, Clone)]
pub struct ProbeOptions {
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub headers: Vec<(reqwest::header::HeaderName, reqwest::header::HeaderValue)>,
}

impl ProbeOptions {
    fn apply(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let ua = self.user_agent.as_deref().unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0");
        req = req.header(reqwest::header::USER_AGENT, ua);
        if let Some(referer) = &self.referer {
            req = req.header(reqwest::header::REFERER, referer);
        }
        for (name, value) in &self.headers {
            req = req.header(name.clone(), value.clone());
        }
        req
    }
}

pub async fn probe_url(client: &reqwest::Client, url_str: &str) -> Result<ProbeResult, String> {
    probe_url_with(client, url_str, &ProbeOptions::default()).await
}

pub async fn probe_url_with(client: &reqwest::Client, url_str: &str, opts: &ProbeOptions) -> Result<ProbeResult, String> {
    let _parsed = Url::parse(url_str).map_err(|e| format!("Invalid URL: {e}"))?;

    // Attempt 1: HEAD request
    let head_res = opts.apply(client.head(url_str)).send().await;

    match head_res {
        Ok(res) if res.status().is_success() => {
            let final_url = res.url().to_string();
            let headers = res.headers().clone();
            parse_headers_into_probe(url_str, &final_url, &headers, None)
        }
        _ => {
            // Attempt 2: Fallback to GET with Range: bytes=0-0 to test range support & avoid downloading entire file
            let get_res = opts
                .apply(client.get(url_str))
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

/// Longest file name (in characters) we will create. Windows allows 255 per
/// path component, but leaving room for ` (1)` and the `.nexpart` suffixes.
const MAX_FILENAME_CHARS: usize = 180;

/// Characters that are invisible or reorder text. They are used to make a
/// file called `invoice\u{202E}fdp.exe` *display* as `invoiceexe.pdf`.
fn is_hidden_or_bidi(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// Windows device names that can't be used as file names, with or without an extension.
fn is_reserved_device_name(stem: &str) -> bool {
    let up = stem.trim_end().to_uppercase();
    matches!(up.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ["COM", "LPT"].iter().any(|p| {
            up.strip_prefix(p)
                .map(|d| d.chars().count() == 1 && d.chars().all(|c| matches!(c, '0'..='9' | '\u{00B9}' | '\u{00B2}' | '\u{00B3}')))
                .unwrap_or(false)
        })
}

/// Turns anything a server (or a user) might call a file into a name that is
/// safe to create on every platform: no path separators or traversal, no
/// control or direction-changing characters, no reserved Windows names, no
/// trailing dots/spaces (which Windows silently strips, changing the name),
/// and a bounded length.
pub fn sanitize_filename(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .filter(|c| !is_hidden_or_bidi(*c))
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect();

    // Prevent directory traversal
    while clean.contains("..") {
        clean = clean.replace("..", "_");
    }

    let mut clean = clean.trim_start().trim_end_matches(|c| c == ' ' || c == '.').to_string();

    // Over-long names: shorten the part before the extension, keep the extension.
    if clean.chars().count() > MAX_FILENAME_CHARS {
        let (stem, ext) = match clean.rfind('.') {
            Some(i) if clean.chars().count() - clean[..i].chars().count() <= 17 => (clean[..i].to_string(), clean[i..].to_string()),
            _ => (clean.clone(), String::new()),
        };
        let keep = MAX_FILENAME_CHARS.saturating_sub(ext.chars().count());
        let shortened: String = stem.chars().take(keep).collect();
        clean = format!("{}{}", shortened.trim_end_matches(|c| c == ' ' || c == '.'), ext);
    }

    if clean.is_empty() {
        return "download.bin".to_string();
    }

    let stem = clean.split('.').next().unwrap_or("");
    if is_reserved_device_name(stem) {
        clean = format!("_{clean}");
    }
    clean
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

/// Like [`resolve_unique_path`], but *claims* the name: an empty placeholder is
/// created atomically, so two downloads finishing at once (or another program)
/// can never end up with the same path. Rename the finished file over the
/// placeholder; remove the placeholder if the download is abandoned.
pub fn reserve_unique_path(dir: &Path, filename: &str) -> std::io::Result<std::path::PathBuf> {
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("download");
    let ext = path.extension().and_then(|s| s.to_str());

    for i in 0..10_000u32 {
        let candidate = if i == 0 {
            filename.to_string()
        } else {
            match ext {
                Some(e) => format!("{stem} ({i}).{e}"),
                None => format!("{stem} ({i})"),
            }
        };
        let target = dir.join(&candidate);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(_) => return Ok(target),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "no free file name"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_path_traversal_and_separators() {
        assert_eq!(sanitize_filename("../../Windows/system32/evil.dll"), "____Windows_system32_evil.dll");
        assert_eq!(sanitize_filename("a\\b/c:d*e?f\"g<h>i|j.txt"), "a_b_c_d_e_f_g_h_i_j.txt");
        assert!(!sanitize_filename("....//x").contains(".."));
    }

    #[test]
    fn removes_invisible_and_direction_override_characters() {
        let tricky = "invoice\u{202E}fdp.exe";
        let clean = sanitize_filename(tricky);
        assert_eq!(clean, "invoicefdp.exe");
        assert_eq!(sanitize_filename("a\u{0}b\u{7}c\u{200B}d.txt"), "abcd.txt");
    }

    #[test]
    fn avoids_reserved_windows_names() {
        for name in ["CON", "con.txt", "NUL.tar.gz", "Com1", "LPT9.log", "aux", "PRN.", "COM\u{00B9}.txt"] {
            let clean = sanitize_filename(name);
            assert!(clean.starts_with('_'), "{name} -> {clean}");
        }
        // Lookalikes that aren't reserved stay as they are.
        assert_eq!(sanitize_filename("console.txt"), "console.txt");
        assert_eq!(sanitize_filename("COM10.txt"), "COM10.txt");
        assert_eq!(sanitize_filename("Communism.pdf"), "Communism.pdf");
    }

    #[test]
    fn trims_trailing_dots_and_spaces() {
        assert_eq!(sanitize_filename("report.pdf. . "), "report.pdf");
        assert_eq!(sanitize_filename("   spaced.txt"), "spaced.txt");
    }

    #[test]
    fn empty_and_dot_names_get_a_default() {
        assert_eq!(sanitize_filename(""), "download.bin");
        assert_eq!(sanitize_filename("."), "download.bin");
        assert_eq!(sanitize_filename("  . "), "download.bin");
    }

    #[test]
    fn long_names_are_shortened_but_keep_their_extension() {
        let long = format!("{}.tar.gz", "x".repeat(400));
        let clean = sanitize_filename(&long);
        assert!(clean.chars().count() <= MAX_FILENAME_CHARS, "{}", clean.chars().count());
        assert!(clean.ends_with(".gz"));
        // Multi-byte characters are cut on character boundaries, not bytes.
        let wide = format!("{}.zip", "日".repeat(500));
        let clean = sanitize_filename(&wide);
        assert!(clean.chars().count() <= MAX_FILENAME_CHARS && clean.ends_with(".zip"));
    }

    #[test]
    fn normal_names_are_untouched() {
        for name in ["ubuntu-24.04-desktop-amd64.iso", "my file (1).pdf", "données-été.zip", ".gitignore"] {
            assert_eq!(sanitize_filename(name), name);
        }
    }

    #[test]
    fn reserving_a_unique_path_never_hands_out_the_same_name_twice() {
        let dir = std::env::temp_dir().join(format!("nexdrop_reserve_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let a = reserve_unique_path(&dir, "f.bin").unwrap();
        let b = reserve_unique_path(&dir, "f.bin").unwrap();
        let c = reserve_unique_path(&dir, "f.bin").unwrap();
        assert_eq!(a.file_name().unwrap(), "f.bin");
        assert_eq!(b.file_name().unwrap(), "f (1).bin");
        assert_eq!(c.file_name().unwrap(), "f (2).bin");
        assert!(a.exists() && b.exists() && c.exists(), "placeholders must exist");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
