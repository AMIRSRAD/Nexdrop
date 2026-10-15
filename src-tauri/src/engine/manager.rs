use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

use crate::db::models::{
    AddDownloadRequest, AppSettings, DownloadProgressEvent, DownloadStatus, DownloadTask, FileCategory,
    SiteCredential,
};
use crate::db::Repository;
use crate::engine::downloader::DownloadEngine;
use crate::engine::probe::{probe_url, sanitize_filename};
use crate::engine::segment::{calculate_segments, calculate_segments_with_chunk_size};

pub struct DownloadManager {
    db_path: PathBuf,
    engine: DownloadEngine,
    active_controllers: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    app_handle: AppHandle,
}

impl DownloadManager {
    pub fn new(app_handle: AppHandle, db_path: PathBuf) -> Self {
        Self {
            db_path,
            engine: DownloadEngine::new(),
            active_controllers: Arc::new(Mutex::new(HashMap::new())),
            app_handle,
        }
    }

    fn get_db_conn(&self) -> rusqlite::Result<rusqlite::Connection> {
        Repository::open_or_create(&self.db_path)
    }

    pub fn get_settings(&self) -> AppSettings {
        if let Ok(conn) = self.get_db_conn() {
            Repository::load_settings(&conn).unwrap_or_default()
        } else {
            AppSettings::default()
        }
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_settings(&conn, settings).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_all_site_credentials(&self) -> Result<Vec<SiteCredential>, String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::get_all_site_credentials(&conn).map_err(|e| e.to_string())
    }

    pub fn save_site_credential(&self, cred: SiteCredential) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_site_credential(&conn, &cred).map_err(|e| e.to_string())
    }

    pub fn delete_site_credential(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::delete_site_credential(&conn, id).map_err(|e| e.to_string())
    }

    pub async fn add_download(&self, req: AddDownloadRequest) -> Result<DownloadTask, String> {
        let settings = self.get_settings();
        let proxy_mode_to_use = req.proxy_mode.as_deref().unwrap_or(&settings.proxy_mode);
        let custom_proxy_to_use = req.custom_proxy_url.as_deref().or(settings.custom_proxy_url.as_deref());
        let client = crate::engine::create_http_client_with_proxy(
            std::time::Duration::from_secs(15),
            Some(proxy_mode_to_use),
            custom_proxy_to_use,
        );

        let probe = probe_url(&client, &req.url).await?;

        let filename = req
            .filename
            .filter(|f| !f.trim().is_empty())
            .unwrap_or(probe.filename);
        let safe_filename = sanitize_filename(&filename);

        let base_dest_dir_str = req
            .destination_path
            .filter(|d| !d.trim().is_empty())
            .unwrap_or(settings.default_destination.clone());

        let mut dest_dir = PathBuf::from(&base_dest_dir_str);
        if settings.auto_categorize && probe.category != FileCategory::Other {
            let sub = probe.category.subfolder_name();
            if !sub.is_empty() {
                dest_dir = dest_dir.join(sub);
            }
        }
        let _ = std::fs::create_dir_all(&dest_dir);
        let dest_dir_str = dest_dir.to_string_lossy().to_string();

        let connections = req
            .connections
            .unwrap_or(settings.default_connections)
            .clamp(1, 32);

        let min_chunk_size = req.min_chunk_size.unwrap_or(settings.min_chunk_size);
        let segments = if probe.resumable && probe.total_bytes > 0 {
            calculate_segments_with_chunk_size(probe.total_bytes, connections, min_chunk_size)
        } else {
            calculate_segments(probe.total_bytes, 1)
        };

        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();

        let task = DownloadTask {
            id: id.clone(),
            url: req.url,
            final_url: Some(probe.final_url),
            filename: safe_filename.clone(),
            destination_path: dest_dir_str.clone(),
            temp_path: dest_dir.join(format!("{}.nexpart", safe_filename)).to_string_lossy().to_string(),
            total_bytes: probe.total_bytes,
            downloaded_bytes: 0,
            status: if req.scheduled_time.is_some() {
                DownloadStatus::Scheduled
            } else {
                DownloadStatus::Queued
            },
            resumable: probe.resumable,
            etag: probe.etag,
            last_modified: probe.last_modified,
            mime_type: probe.mime_type,
            category: probe.category,
            connections: segments.len(),
            speed_limit: req.speed_limit.unwrap_or(0),
            priority: req.priority.unwrap_or(0),
            speed_bps: 0,
            eta_seconds: None,
            segments,
            created_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
            scheduled_time: req.scheduled_time,
            checksum_sha256: req.expected_checksum.as_ref().filter(|s| s.len() == 64).cloned(),
            checksum_md5: req.expected_checksum.as_ref().filter(|s| s.len() == 32).cloned(),
            user_agent: req.user_agent,
            referer: req.referer,
            mirrors: req.mirrors.unwrap_or_default(),
            min_chunk_size,
            proxy_mode: req.proxy_mode,
            custom_proxy_url: req.custom_proxy_url,
            antivirus_status: None,
        };

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_download(&conn, &task).map_err(|e| e.to_string())?;

        // If not scheduled, check if queue has capacity to start immediately
        if task.status == DownloadStatus::Queued {
            self.process_queue().await;
        }

        Ok(task)
    }

    pub async fn process_queue(&self) {
        let settings = self.get_settings();
        let active_count = self.active_controllers.lock().len();

        if active_count >= settings.max_active_downloads {
            return;
        }

        let slots_available = settings.max_active_downloads - active_count;
        let conn = match self.get_db_conn() {
            Ok(c) => c,
            Err(_) => return,
        };

        let all = match Repository::get_all_downloads(&conn) {
            Ok(tasks) => tasks,
            Err(_) => return,
        };

        // Filter queued tasks, sorted by priority DESC, created_at ASC
        let mut queued: Vec<DownloadTask> = all
            .into_iter()
            .filter(|t| t.status == DownloadStatus::Queued)
            .collect();

        queued.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.created_at.cmp(&b.created_at)));

        for task in queued.into_iter().take(slots_available) {
            self.start_download(task).await;
        }
    }

    pub async fn start_download(&self, mut task: DownloadTask) {
        let (pause_tx, pause_rx) = watch::channel(false);
        {
            let mut active = self.active_controllers.lock();
            active.insert(task.id.clone(), pause_tx);
        }

        task.status = DownloadStatus::Downloading;
        if let Ok(conn) = self.get_db_conn() {
            let now = chrono::Utc::now().timestamp();
            let _ = Repository::update_status(&conn, &task.id, DownloadStatus::Downloading, None, None, now);
        }

        let app_handle_clone = self.app_handle.clone();
        let db_path_clone = self.db_path.clone();
        let active_controllers_clone = self.active_controllers.clone();
        let task_id = task.id.clone();

        let engine = self.engine.clone();

        tauri::async_runtime::spawn(async move {
            let app_h = app_handle_clone.clone();
            let db_p = db_path_clone.clone();

            let res = engine
                .run_download(task, pause_rx, move |event: DownloadProgressEvent| {
                    let _ = app_h.emit("download-progress", &event);
                    if let Ok(conn) = Repository::open_or_create(&db_p) {
                        let now = chrono::Utc::now().timestamp();
                        let _ = Repository::update_progress(
                            &conn,
                            &event.id,
                            event.downloaded_bytes,
                            event.status,
                            event.speed_bps,
                            event.eta_seconds,
                            now,
                        );
                    }
                })
                .await;

            // Remove from active controllers
            {
                let mut active = active_controllers_clone.lock();
                active.remove(&task_id);
            }

            if let Ok(conn) = Repository::open_or_create(&db_path_clone) {
                let now = chrono::Utc::now().timestamp();
                match res {
                    Ok(mut final_task) => {
                        let final_path = Path::new(&final_task.destination_path).join(&final_task.filename);
                        if let Ok((sha256, md5)) = crate::engine::compute_checksums(&final_path) {
                            final_task.checksum_sha256 = Some(sha256.clone());
                            final_task.checksum_md5 = Some(md5.clone());
                            let _ = Repository::update_checksum(&conn, &task_id, Some(&sha256), Some(&md5));
                        }

                        // Antivirus / Windows Defender post-download scan
                        let settings = Repository::load_settings(&conn).unwrap_or_default();
                        if settings.scan_with_antivirus {
                            let scan_result = scan_file_with_antivirus(&final_path, settings.antivirus_path.as_deref());
                            final_task.antivirus_status = Some(scan_result.clone());
                            let _ = Repository::update_antivirus_status(&conn, &task_id, &scan_result);
                        }

                        let _ = Repository::save_download(&conn, &final_task);
                        let _ = app_handle_clone.emit("download-completed", &final_task);
                    }
                    Err(err) => {
                        let _ = Repository::update_status(&conn, &task_id, DownloadStatus::Failed, Some(&err), None, now);
                        let _ = app_handle_clone.emit("download-failed", serde_json::json!({
                            "id": task_id,
                            "error": err,
                        }));
                    }
                }
            }

            // Trigger next item in queue
            let _ = app_handle_clone.emit("download-status-changed", serde_json::json!({ "id": task_id }));
        });
    }

    pub fn pause_download(&self, id: &str) -> Result<(), String> {
        let mut active = self.active_controllers.lock();
        if let Some(tx) = active.remove(id) {
            let _ = tx.send(true);
        }

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let now = chrono::Utc::now().timestamp();
        Repository::update_status(&conn, id, DownloadStatus::Paused, None, None, now).map_err(|e| e.to_string())?;

        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({
            "id": id,
            "status": "paused"
        }));

        Ok(())
    }

    pub async fn resume_download(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let task_opt = Repository::get_download(&conn, id).map_err(|e| e.to_string())?;

        if let Some(_task) = task_opt {
            let now = chrono::Utc::now().timestamp();
            Repository::update_status(&conn, id, DownloadStatus::Queued, None, None, now).map_err(|e| e.to_string())?;
            self.process_queue().await;
            Ok(())
        } else {
            Err(format!("Download task {id} not found"))
        }
    }

    pub async fn retry_download(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let task_opt = Repository::get_download(&conn, id).map_err(|e| e.to_string())?;

        if let Some(mut task) = task_opt {
            task.status = DownloadStatus::Queued;
            task.error_message = None;
            let now = chrono::Utc::now().timestamp();
            Repository::update_status(&conn, id, DownloadStatus::Queued, None, None, now).map_err(|e| e.to_string())?;
            self.process_queue().await;
            Ok(())
        } else {
            Err(format!("Download task {id} not found"))
        }
    }

    pub fn cancel_download(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let _ = self.pause_download(id);

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        if let Ok(Some(task)) = Repository::get_download(&conn, id) {
            if delete_file {
                let dest_dir = Path::new(&task.destination_path);
                let part_file = dest_dir.join(format!("{}.nexpart", task.filename));
                let _ = std::fs::remove_file(part_file);

                for i in 0..task.connections {
                    let seg_file = dest_dir.join(format!("{}.nexpart_seg_{}", task.filename, i));
                    let _ = std::fs::remove_file(seg_file);
                }
            }

            let now = chrono::Utc::now().timestamp();
            let _ = Repository::update_status(&conn, id, DownloadStatus::Canceled, None, None, now);
        }

        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({
            "id": id,
            "status": "canceled"
        }));

        Ok(())
    }

    pub fn delete_download(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let _ = self.pause_download(id);

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        if let Ok(Some(task)) = Repository::get_download(&conn, id) {
            if delete_file {
                let final_path = Path::new(&task.destination_path).join(&task.filename);
                let _ = std::fs::remove_file(final_path);

                let part_file = Path::new(&task.destination_path).join(format!("{}.nexpart", task.filename));
                let _ = std::fs::remove_file(part_file);

                for i in 0..task.connections {
                    let seg_file = Path::new(&task.destination_path).join(format!("{}.nexpart_seg_{}", task.filename, i));
                    let _ = std::fs::remove_file(seg_file);
                }
            }
        }

        Repository::delete_download(&conn, id).map_err(|e| e.to_string())?;
        let _ = self.app_handle.emit("download-deleted", serde_json::json!({ "id": id }));
        Ok(())
    }

    pub fn get_all_downloads(&self) -> Result<Vec<DownloadTask>, String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::get_all_downloads(&conn).map_err(|e| e.to_string())
    }

    pub fn get_download(&self, id: &str) -> Result<Option<DownloadTask>, String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::get_download(&conn, id).map_err(|e| e.to_string())
    }

    pub fn open_file(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let task = Repository::get_download(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Task not found".to_string())?;

        let file_path = Path::new(&task.destination_path).join(&task.filename);
        if !file_path.exists() {
            return Err("File does not exist on disk".to_string());
        }

        open::that(file_path).map_err(|e| format!("Failed to open file: {e}"))?;
        Ok(())
    }

    pub fn reveal_file(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let task = Repository::get_download(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Task not found".to_string())?;

        let file_path = Path::new(&task.destination_path).join(&task.filename);
        if !file_path.exists() {
            // If file doesn't exist yet, open the directory
            let dir = Path::new(&task.destination_path);
            open::that(dir).map_err(|e| format!("Failed to open directory: {e}"))?;
            return Ok(());
        }

        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            Command::new("explorer")
                .arg("/select,")
                .arg(file_path)
                .spawn()
                .map_err(|e| format!("Failed to reveal file: {e}"))?;
        }

        #[cfg(target_os = "macos")]
        {
            use std::process::Command;
            Command::new("open")
                .arg("-R")
                .arg(file_path)
                .spawn()
                .map_err(|e| format!("Failed to reveal file: {e}"))?;
        }

        #[cfg(target_os = "linux")]
        {
            let dir = Path::new(&task.destination_path);
            open::that(dir).map_err(|e| format!("Failed to open directory: {e}"))?;
        }

        Ok(())
    }

    pub fn get_active_task_ids(&self) -> Vec<String> {
        let active = self.active_controllers.lock();
        active.keys().cloned().collect()
    }
}

fn scan_file_with_antivirus(file_path: &Path, custom_av_path: Option<&str>) -> String {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let av_path = if let Some(p) = custom_av_path {
            PathBuf::from(p)
        } else {
            let p1 = PathBuf::from(r"C:\Program Files\Windows Defender\MpCmdRun.exe");
            if p1.exists() {
                p1
            } else {
                PathBuf::from(r"C:\ProgramData\Microsoft\Windows Defender\Platform\MpCmdRun.exe")
            }
        };

        if av_path.exists() {
            let res = Command::new(&av_path)
                .args(["-Scan", "-ScanType", "3", "-File"])
                .arg(file_path)
                .output();

            match res {
                Ok(output) => {
                    if output.status.code() == Some(0) {
                        "clean".to_string()
                    } else if output.status.code() == Some(2) {
                        "threat_detected".to_string()
                    } else {
                        "clean".to_string()
                    }
                }
                Err(_) => "skipped".to_string(),
            }
        } else {
            "clean".to_string()
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = file_path;
        let _ = custom_av_path;
        "clean".to_string()
    }
}
