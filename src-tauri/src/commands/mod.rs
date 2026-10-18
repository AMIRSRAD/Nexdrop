use std::sync::Arc;
use tauri::State;

use std::collections::HashMap;

use crate::db::models::{
    AddDownloadRequest, AddTorrentRequest, AppSettings, DownloadTask, SiteCredential, TorrentProbeInfo, UrlProbeResult,
};
use crate::engine::torrent::TorrentSource;
use crate::engine::auth::{base64, is_forbidden_header};
use crate::engine::manager::DownloadManager;

pub type AppState = Arc<DownloadManager>;

#[tauri::command]
pub async fn probe_url_cmd(state: State<'_, AppState>, url: String) -> Result<UrlProbeResult, String> {
    let probe = state.probe_with_settings(&url).await?;
    let settings = state.get_settings();

    Ok(UrlProbeResult {
        url: probe.url,
        filename: probe.filename,
        total_bytes: probe.total_bytes,
        resumable: probe.resumable,
        mime_type: probe.mime_type,
        category: probe.category,
        default_destination: settings.default_destination,
    })
}

#[tauri::command]
pub async fn add_download_cmd(
    state: State<'_, AppState>,
    req: AddDownloadRequest,
) -> Result<DownloadTask, String> {
    state.add_download(req).await
}

#[tauri::command]
pub async fn batch_add_downloads_cmd(
    state: State<'_, AppState>,
    reqs: Vec<AddDownloadRequest>,
) -> Result<Vec<DownloadTask>, String> {
    let mut tasks = Vec::new();
    for req in reqs {
        match state.add_download(req).await {
            Ok(task) => tasks.push(task),
            Err(e) => log::error!("Failed to add batch item: {e}"),
        }
    }
    Ok(tasks)
}

#[tauri::command]
pub async fn pause_download_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.pause_download(&id)
}

#[tauri::command]
pub async fn resume_download_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.resume_download(&id).await
}

#[tauri::command]
pub async fn retry_download_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.retry_download(&id).await
}

#[tauri::command]
pub async fn cancel_download_cmd(
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    state.cancel_download(&id, delete_file).await
}

#[tauri::command]
pub async fn delete_download_cmd(
    state: State<'_, AppState>,
    id: String,
    delete_file: bool,
) -> Result<(), String> {
    state.delete_download(&id, delete_file).await
}

#[tauri::command]
pub async fn get_all_downloads_cmd(
    state: State<'_, AppState>,
) -> Result<Vec<DownloadTask>, String> {
    state.get_all_downloads()
}

#[tauri::command]
pub async fn get_download_cmd(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<DownloadTask>, String> {
    state.get_download(&id)
}

#[tauri::command]
pub async fn get_settings_cmd(state: State<'_, AppState>) -> Result<AppSettings, String> {
    Ok(state.get_settings())
}

#[tauri::command]
pub async fn update_settings_cmd(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<(), String> {
    state.save_settings(&settings)
}

#[tauri::command]
pub async fn open_file_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.open_file(&id)
}

#[tauri::command]
pub async fn reveal_file_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.reveal_file(&id)
}

#[tauri::command]
pub async fn get_default_download_dir_cmd() -> Result<String, String> {
    dirs::download_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "Could not determine Downloads directory".to_string())
}

#[tauri::command]
pub async fn verify_checksum_cmd(
    state: State<'_, AppState>,
    id: String,
    algorithm: String,
    expected: Option<String>,
) -> Result<crate::db::models::ChecksumResult, String> {
    let task = state
        .get_download(&id)?
        .ok_or_else(|| "Download task not found".to_string())?;

    let path = std::path::Path::new(&task.destination_path).join(&task.filename);
    if !path.exists() {
        return Err("Downloaded file does not exist on disk".to_string());
    }

    crate::engine::verify_file_checksum(&path, &algorithm, expected.as_deref())
}

#[tauri::command]
pub async fn calculate_file_hash_cmd(
    file_path: String,
    algorithm: String,
) -> Result<crate::db::models::ChecksumResult, String> {
    let path = std::path::Path::new(&file_path);
    if !path.exists() {
        return Err("File does not exist on disk".to_string());
    }
    crate::engine::verify_file_checksum(path, &algorithm, None)
}

#[tauri::command]
pub async fn get_curl_command_cmd(
    state: State<'_, AppState>,
    id: String,
) -> Result<String, String> {
    let task = state
        .get_download(&id)?
        .ok_or_else(|| "Download not found".to_string())?;

    let mut curl = format!("curl -C - -L -o \"{}\"", task.filename);
    if let Some(ref ua) = task.user_agent {
        curl.push_str(&format!(" -A \"{}\"", ua));
    }
    if let Some(ref ref_url) = task.referer {
        curl.push_str(&format!(" -e \"{}\"", ref_url));
    }
    curl.push_str(&format!(" \"{}\"", task.url));
    Ok(curl)
}

#[tauri::command]
pub async fn export_downloads_cmd(state: State<'_, AppState>) -> Result<String, String> {
    let all = state.get_all_downloads()?;
    serde_json::to_string_pretty(&all).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_site_credentials_cmd(
    state: State<'_, AppState>,
) -> Result<Vec<SiteCredential>, String> {
    state.get_all_site_credentials()
}

#[tauri::command]
pub async fn save_site_credential_cmd(
    state: State<'_, AppState>,
    cred: SiteCredential,
) -> Result<(), String> {
    state.save_site_credential(cred)
}

#[tauri::command]
pub async fn delete_site_credential_cmd(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.delete_site_credential(&id)
}

#[tauri::command]
pub async fn export_checksum_file_cmd(
    state: State<'_, AppState>,
    id: String,
    algorithm: String,
) -> Result<String, String> {
    let task = state
        .get_download(&id)?
        .ok_or_else(|| "Download task not found".to_string())?;

    let file_path = std::path::Path::new(&task.destination_path).join(&task.filename);
    if !file_path.exists() {
        return Err("Downloaded file does not exist on disk".to_string());
    }

    let (sha256, md5) = crate::engine::compute_checksums(&file_path)
        .map_err(|e| format!("Failed to compute checksums: {e}"))?;

    let algo_lower = algorithm.to_lowercase();
    let (hash, ext) = if algo_lower == "md5" {
        (md5, "md5")
    } else {
        (sha256, "sha256")
    };

    let target_checksum_path = std::path::Path::new(&task.destination_path).join(format!("{}.{}", task.filename, ext));
    let content = format!("{} *{}\n", hash, task.filename);
    std::fs::write(&target_checksum_path, content)
        .map_err(|e| format!("Failed to write checksum file: {e}"))?;

    Ok(target_checksum_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn parse_curl_command_cmd(curl: String) -> Result<AddDownloadRequest, String> {
    let trimmed = curl.trim();
    if !trimmed.starts_with("curl") {
        return Err("Command must start with 'curl'".to_string());
    }

    let mut url: Option<String> = None;
    let mut filename: Option<String> = None;
    let mut user_agent: Option<String> = None;
    let mut referer: Option<String> = None;
    let mut custom_proxy_url: Option<String> = None;
    let mut proxy_mode: Option<String> = None;
    let mut headers: HashMap<String, String> = HashMap::new();

    let tokens = tokenize_command(trimmed);
    let mut iter = tokens.into_iter().peekable();
    let _ = iter.next(); // Skip 'curl'

    while let Some(token) = iter.next() {
        match token.as_str() {
            "-H" | "--header" => {
                if let Some(header_val) = iter.next() {
                    let mut parts = header_val.splitn(2, ':');
                    if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                        let key = k.trim().to_lowercase();
                        let val = v.trim().to_string();
                        if key == "user-agent" {
                            user_agent = Some(val);
                        } else if key == "referer" {
                            referer = Some(val);
                        } else if !is_forbidden_header(&key) && !val.is_empty() {
                            // Cookie, Authorization, custom tokens: exactly what makes
                            // "Copy as cURL" downloads work for logged-in sites.
                            headers.insert(k.trim().to_string(), val);
                        }
                    }
                }
            }
            "-b" | "--cookie" => {
                // A cookie string (name=value; ...). A bare file name is a cookie jar, which we can't read.
                if let Some(cookie) = iter.next().filter(|c| c.contains('=')) {
                    headers.insert("Cookie".to_string(), cookie);
                }
            }
            "-u" | "--user" => {
                if let Some(creds) = iter.next().filter(|c| c.contains(':')) {
                    headers.insert("Authorization".to_string(), format!("Basic {}", base64(creds.as_bytes())));
                }
            }
            "-A" | "--user-agent" => {
                user_agent = iter.next();
            }
            "-e" | "--referer" => {
                referer = iter.next();
            }
            "-o" | "--output" => {
                filename = iter.next();
            }
            "-x" | "--proxy" => {
                custom_proxy_url = iter.next();
                proxy_mode = Some("custom".to_string());
            }
            "--url" => {
                url = iter.next();
            }
            arg => {
                if (arg.starts_with("http://") || arg.starts_with("https://")) && url.is_none() {
                    url = Some(arg.to_string());
                }
            }
        }
    }

    let final_url = url.ok_or_else(|| "Could not find a valid HTTP/HTTPS URL in the curl command".to_string())?;

    Ok(AddDownloadRequest {
        url: final_url,
        filename,
        destination_path: None,
        connections: None,
        speed_limit: None,
        priority: None,
        scheduled_time: None,
        user_agent,
        referer,
        custom_headers: if headers.is_empty() { None } else { Some(headers) },
        expected_checksum: None,
        mirrors: None,
        min_chunk_size: None,
        proxy_mode,
        custom_proxy_url,
        queue_id: None,
    })
}

fn tokenize_command(cmd: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;

    for c in cmd.chars() {
        if escape {
            current.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if c == '\'' && !in_double {
            in_single = !in_single;
            continue;
        }
        if c == '"' && !in_single {
            in_double = !in_double;
            continue;
        }
        if c.is_whitespace() && !in_single && !in_double {
            if !current.is_empty() {
                tokens.push(current);
                current = String::new();
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[tauri::command]
pub async fn grab_page_links_cmd(url: String) -> Result<Vec<crate::grabber_server::GrabbedLink>, String> {
    crate::grabber_server::extract_links_from_page(&url).await
}

/// Looks inside a torrent (magnet link, link to a `.torrent`, or the file's bytes) without downloading it.
#[tauri::command]
pub async fn probe_torrent_cmd(
    state: State<'_, AppState>,
    url: Option<String>,
    bytes: Option<Vec<u8>>,
) -> Result<TorrentProbeInfo, String> {
    let source = match (bytes, url) {
        (Some(bytes), _) if !bytes.is_empty() => TorrentSource::Bytes(bytes),
        (_, Some(url)) if !url.trim().is_empty() => TorrentSource::Url(url.trim().to_string()),
        _ => return Err("Give a magnet link or choose a .torrent file".to_string()),
    };
    state.probe_torrent(source).await
}

#[tauri::command]
pub async fn add_torrent_cmd(state: State<'_, AppState>, req: AddTorrentRequest) -> Result<DownloadTask, String> {
    state.add_torrent(req).await
}

#[tauri::command]
pub async fn stop_seeding_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.stop_seeding(&id).await
}

#[tauri::command]
pub async fn start_seeding_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.start_seeding(&id).await
}

/// Cancels a pending "sleep/shut down when finished" countdown.
#[tauri::command]
pub async fn cancel_completion_action_cmd(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state.cancel_pending_power_action())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn curl_import_keeps_cookies_and_auth_but_not_engine_headers() {
        let cmd = r#"curl 'https://files.example.com/a.zip' -H 'Cookie: sid=abc' -H 'Authorization: Bearer tok' -H 'Range: bytes=0-10' -H 'Accept-Encoding: gzip' -H 'User-Agent: UA/1' -H 'Referer: https://example.com/' -H 'X-Custom: yes' --compressed"#;
        let req = parse_curl_command_cmd(cmd.to_string()).await.unwrap();
        let h = req.custom_headers.unwrap();
        assert_eq!(h.get("Cookie").map(String::as_str), Some("sid=abc"));
        assert_eq!(h.get("Authorization").map(String::as_str), Some("Bearer tok"));
        assert_eq!(h.get("X-Custom").map(String::as_str), Some("yes"));
        assert!(!h.contains_key("Range") && !h.contains_key("Accept-Encoding"), "{h:?}");
        assert_eq!(req.user_agent.as_deref(), Some("UA/1"));
        assert_eq!(req.referer.as_deref(), Some("https://example.com/"));
        assert_eq!(req.url, "https://files.example.com/a.zip");
    }

    #[tokio::test]
    async fn curl_cookie_and_basic_auth_flags_are_understood() {
        let req = parse_curl_command_cmd("curl -b 'a=1; b=2' -u user:pass https://x.test/f".to_string()).await.unwrap();
        let h = req.custom_headers.unwrap();
        assert_eq!(h["Cookie"], "a=1; b=2");
        assert_eq!(h["Authorization"], "Basic dXNlcjpwYXNz");
        // A cookie *file* can't be read, so it's ignored rather than sent as a cookie.
        let req = parse_curl_command_cmd("curl -b cookies.txt https://x.test/f".to_string()).await.unwrap();
        assert!(req.custom_headers.is_none());
    }
}

/// Quits Nexdrop for real (the window's close button only hides it to the tray).
#[tauri::command]
pub async fn quit_app_cmd(app: tauri::AppHandle) -> Result<(), String> {
    crate::quit_app(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_autostart_cmd(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_autostart_cmd(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|e| e.to_string())?;
    } else {
        launcher.disable().map_err(|e| e.to_string())?;
    }
    launcher.is_enabled().map_err(|e| e.to_string())
}

/// Where the bundled browser extension lives on this machine.
fn extension_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let mut candidates = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("extension"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("extension"));
        }
    }
    // Development checkout
    candidates.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("extensions").join("nexdrop-browser-extension"));
    candidates.into_iter().find(|p| p.join("manifest.json").exists()).map(|p| p.canonicalize().unwrap_or(p))
}

#[tauri::command]
pub async fn get_extension_dir_cmd(app: tauri::AppHandle) -> Result<String, String> {
    extension_dir(&app)
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .ok_or_else(|| "The browser extension folder was not found".to_string())
}

#[tauri::command]
pub async fn open_extension_dir_cmd(app: tauri::AppHandle) -> Result<(), String> {
    let dir = extension_dir(&app).ok_or_else(|| "The browser extension folder was not found".to_string())?;
    open::that(dir).map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[tauri::command]
pub async fn get_app_info_cmd(app: tauri::AppHandle) -> Result<AppInfo, String> {
    Ok(AppInfo { name: app.package_info().name.clone(), version: app.package_info().version.to_string() })
}

// ───────────────────────────── queues ─────────────────────────────

#[tauri::command]
pub async fn get_queues_cmd(state: State<'_, AppState>) -> Result<Vec<crate::db::models::QueueInfo>, String> {
    Ok(state.queue_infos())
}

#[tauri::command]
pub async fn save_queue_cmd(state: State<'_, AppState>, queue: crate::db::models::DownloadQueue) -> Result<crate::db::models::DownloadQueue, String> {
    state.save_queue(queue)
}

#[tauri::command]
pub async fn delete_queue_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.delete_queue(&id)
}

#[tauri::command]
pub async fn move_to_queue_cmd(state: State<'_, AppState>, ids: Vec<String>, queue_id: String) -> Result<(), String> {
    state.move_to_queue(ids, &queue_id).await
}

#[tauri::command]
pub async fn start_queue_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.start_queue(&id).await
}

#[tauri::command]
pub async fn stop_queue_cmd(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.stop_queue(&id).await
}
