use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::db::models::{
    AddDownloadRequest, AppSettings, DownloadProgressEvent, DownloadStatus, DownloadTask, FileCategory,
    SiteCredential,
};
use crate::db::Repository;
use crate::engine::downloader::DownloadEngine;
use crate::engine::probe::{probe_url, sanitize_filename, ProbeResult};
use crate::engine::segment::{calculate_segments, calculate_segments_with_chunk_size};

pub struct DownloadManager {
    db_path: PathBuf,
    engine: DownloadEngine,
    /// Pause switches for runs that are in flight. An entry stays until its
    /// run has fully ended (not merely until it was asked to pause), so a
    /// task can never have two engines writing to the same files.
    active_controllers: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    /// Serialises queue scheduling so concurrent callers can't start a task twice.
    queue_lock: AsyncMutex<()>,
    /// Recent probe results, so adding a link the dialog just probed costs no second round trip.
    probe_cache: Mutex<HashMap<String, (Instant, ProbeResult)>>,
    app_handle: AppHandle,
}

impl DownloadManager {
    pub fn new(app_handle: AppHandle, db_path: PathBuf) -> Self {
        Self {
            db_path,
            engine: DownloadEngine::new(),
            active_controllers: Arc::new(Mutex::new(HashMap::new())),
            queue_lock: AsyncMutex::new(()),
            probe_cache: Mutex::new(HashMap::new()),
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
        self.engine.set_global_speed_limit(settings.global_speed_limit);
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

    /// Applies settings that take effect while the app runs (call once at startup).
    pub fn apply_runtime_settings(&self) {
        self.engine.set_global_speed_limit(self.get_settings().global_speed_limit);
    }

    /// Probes a link with the right proxy, reusing a result from the last minute if we have one.
    pub async fn probe(
        &self,
        url: &str,
        proxy_mode: Option<&str>,
        custom_proxy: Option<&str>,
    ) -> Result<ProbeResult, String> {
        const TTL: Duration = Duration::from_secs(60);
        let key = format!("{url}|{}|{}", proxy_mode.unwrap_or(""), custom_proxy.unwrap_or(""));
        {
            let mut cache = self.probe_cache.lock();
            cache.retain(|_, (at, _)| at.elapsed() < TTL);
            if let Some((_, hit)) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let client = self.engine.clients().probe(proxy_mode, custom_proxy);
        let result = probe_url(&client, url).await?;
        self.probe_cache.lock().insert(key, (Instant::now(), result.clone()));
        Ok(result)
    }

    /// Probe for the "new download" dialog, using the proxy from settings.
    pub async fn probe_with_settings(&self, url: &str) -> Result<ProbeResult, String> {
        let settings = self.get_settings();
        self.probe(url, Some(&settings.proxy_mode), settings.custom_proxy_url.as_deref()).await
    }

    pub async fn add_download(self: &Arc<Self>, req: AddDownloadRequest) -> Result<DownloadTask, String> {
        let settings = self.get_settings();
        let proxy_mode_to_use = req.proxy_mode.as_deref().unwrap_or(&settings.proxy_mode);
        let custom_proxy_to_use = req.custom_proxy_url.as_deref().or(settings.custom_proxy_url.as_deref());
        let probe = self
            .probe(&req.url, Some(proxy_mode_to_use), custom_proxy_to_use)
            .await?;

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
            proxy_mode: Some(proxy_mode_to_use.to_string()),
            custom_proxy_url: custom_proxy_to_use.map(str::to_string),
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

    /// Called once at startup: anything still marked as downloading was cut off
    /// by a crash or quit. Put it back in the queue (or on hold) so it can be
    /// resumed instead of sitting in a state nothing will ever act on.
    pub fn recover_interrupted(&self) {
        let settings = self.get_settings();
        let target = if settings.auto_resume_on_launch {
            DownloadStatus::Queued
        } else {
            DownloadStatus::Paused
        };
        if let Ok(conn) = self.get_db_conn() {
            let _ = Repository::reset_interrupted(&conn, target, chrono::Utc::now().timestamp());
        }
    }

    pub async fn process_queue(self: &Arc<Self>) {
        // Only one scheduling pass at a time.
        let _guard = self.queue_lock.lock().await;

        let settings = self.get_settings();
        let active_count = self.active_controllers.lock().len();

        if active_count >= settings.max_active_downloads {
            return;
        }

        let slots_available = settings.max_active_downloads - active_count;
        let all = match self.get_db_conn().and_then(|conn| Repository::get_all_downloads(&conn)) {
            Ok(tasks) => tasks,
            Err(_) => return,
        };

        // Filter queued tasks, sorted by priority DESC, created_at ASC
        let mut queued: Vec<DownloadTask> = all
            .into_iter()
            .filter(|t| t.status == DownloadStatus::Queued)
            .collect();

        queued.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.created_at.cmp(&b.created_at)));

        let mut started = 0;
        for task in queued {
            if started >= slots_available {
                break;
            }
            if self.start_download(task) {
                started += 1;
            }
        }
    }

    /// Starts a run for `task`. Returns `false` if it was already running
    /// (e.g. paused and resumed again before the previous run finished
    /// stopping); that run's final queue pass will pick it up afterwards.
    fn start_download(self: &Arc<Self>, mut task: DownloadTask) -> bool {
        let (pause_tx, pause_rx) = watch::channel(false);
        {
            let mut active = self.active_controllers.lock();
            if active.contains_key(&task.id) {
                return false;
            }
            active.insert(task.id.clone(), pause_tx);
        }

        task.status = DownloadStatus::Downloading;
        if let Ok(conn) = self.get_db_conn() {
            let now = chrono::Utc::now().timestamp();
            let _ = Repository::update_status(&conn, &task.id, DownloadStatus::Downloading, None, None, now);
        }

        let manager = Arc::clone(self);
        let task_id = task.id.clone();
        let engine = self.engine.clone();

        tauri::async_runtime::spawn(async move {
            let app_handle = manager.app_handle.clone();
            let db_path = manager.db_path.clone();

            // Progress goes to the UI on every tick, but to the database only
            // every couple of seconds and off the async threads.
            let clock = Instant::now();
            let last_db_write = Arc::new(AtomicU64::new(0));
            let progress_app = app_handle.clone();
            let progress_db = db_path.clone();

            let res = engine
                .run_download(task, pause_rx, move |event: DownloadProgressEvent| {
                    let _ = progress_app.emit("download-progress", &event);
                    if event.status != DownloadStatus::Downloading {
                        return;
                    }
                    let now_ms = clock.elapsed().as_millis() as u64;
                    if now_ms.saturating_sub(last_db_write.load(Ordering::Relaxed)) < 2000 {
                        return;
                    }
                    last_db_write.store(now_ms, Ordering::Relaxed);
                    let db = progress_db.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Ok(conn) = Repository::open_or_create(&db) {
                            let now = chrono::Utc::now().timestamp();
                            let _ = Repository::update_progress(
                                &conn,
                                &event.id,
                                event.downloaded_bytes,
                                event.speed_bps,
                                event.eta_seconds,
                                now,
                            );
                            let _ = Repository::replace_segments(&conn, &event.id, &event.segments);
                        }
                    });
                })
                .await;

            match res {
                // Stopped on request. The pause (or delete) already updated the
                // task's status; just remember how far we got. This is NOT a
                // completed download, so no hashing, scanning or "done" event.
                Ok(task) if task.status == DownloadStatus::Paused => {
                    if let Ok(conn) = Repository::open_or_create(&db_path) {
                        let _ = Repository::update_downloaded_bytes(&conn, &task_id, task.downloaded_bytes);
                        let _ = Repository::replace_segments(&conn, &task_id, &task.segments);
                    }
                }
                Ok(mut final_task) => {
                    let final_path = Path::new(&final_task.destination_path).join(&final_task.filename);
                    let settings = Repository::open_or_create(&db_path)
                        .ok()
                        .and_then(|c| Repository::load_settings(&c).ok())
                        .unwrap_or_default();

                    // Hashing and scanning are slow and blocking: keep them off the async threads.
                    let hash_path = final_path.clone();
                    let hashes = tauri::async_runtime::spawn_blocking(move || {
                        crate::engine::compute_checksums(&hash_path)
                    })
                    .await;
                    if let Ok(Ok((sha256, md5))) = hashes {
                        final_task.checksum_sha256 = Some(sha256);
                        final_task.checksum_md5 = Some(md5);
                    }

                    if settings.scan_with_antivirus {
                        let scan_path = final_path.clone();
                        let av_path = settings.antivirus_path.clone();
                        let scan = tauri::async_runtime::spawn_blocking(move || {
                            scan_file_with_antivirus(&scan_path, av_path.as_deref())
                        })
                        .await
                        .unwrap_or_else(|_| "skipped".to_string());
                        final_task.antivirus_status = Some(scan);
                    }

                    if let Ok(conn) = Repository::open_or_create(&db_path) {
                        let _ = Repository::save_download(&conn, &final_task);
                    }
                    let _ = app_handle.emit("download-completed", &final_task);
                }
                Err(err) => {
                    if let Ok(conn) = Repository::open_or_create(&db_path) {
                        let now = chrono::Utc::now().timestamp();
                        let _ = Repository::update_status(&conn, &task_id, DownloadStatus::Failed, Some(&err), None, now);
                    }
                    let _ = app_handle.emit("download-failed", serde_json::json!({
                        "id": task_id,
                        "error": err,
                    }));
                }
            }

            // The run is over: free its slot, then let the next queued download start.
            manager.active_controllers.lock().remove(&task_id);
            let _ = app_handle.emit("download-status-changed", serde_json::json!({ "id": task_id }));
            manager.process_queue().await;
        });

        true
    }

    pub fn pause_download(&self, id: &str) -> Result<(), String> {
        {
            let active = self.active_controllers.lock();
            if let Some(tx) = active.get(id) {
                let _ = tx.send(true);
            }
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

    pub async fn resume_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
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

    pub async fn retry_download(self: &Arc<Self>, id: &str) -> Result<(), String> {
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

    /// Waits (briefly) for a stopping run to release its files.
    async fn wait_until_idle(&self, id: &str) {
        for _ in 0..50 {
            if !self.active_controllers.lock().contains_key(id) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn cancel_download(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let _ = self.pause_download(id);
        self.wait_until_idle(id).await;

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        if let Ok(Some(task)) = Repository::get_download(&conn, id) {
            if delete_file {
                remove_partial_files(&task);
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

    pub async fn delete_download(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let _ = self.pause_download(id);
        self.wait_until_idle(id).await;

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        if let Ok(Some(task)) = Repository::get_download(&conn, id) {
            if delete_file {
                let _ = std::fs::remove_file(Path::new(&task.destination_path).join(&task.filename));
                remove_partial_files(&task);
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
            crate::process::command("explorer")
                .arg("/select,")
                .arg(file_path)
                .spawn()
                .map_err(|e| format!("Failed to reveal file: {e}"))?;
        }

        #[cfg(target_os = "macos")]
        {
            crate::process::command("open")
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

/// Removes everything a partial download leaves behind: the preallocated file,
/// its progress map, and the per-segment files older versions used.
fn remove_partial_files(task: &DownloadTask) {
    let dir = Path::new(&task.destination_path);
    for suffix in ["nexpart", "nexpart.map", "nexpart.map.tmp", "nexmerge"] {
        let _ = std::fs::remove_file(dir.join(format!("{}.{}", task.filename, suffix)));
    }
    for i in 0..32 {
        let _ = std::fs::remove_file(dir.join(format!("{}.nexpart_seg_{}", task.filename, i)));
    }
}

/// Returns "clean", "threat_detected" or "skipped". Anything that isn't a
/// definite verdict (no scanner installed, scanner failed to run, unexpected
/// exit code) is "skipped" so we never claim a file was checked when it wasn't.
fn scan_file_with_antivirus(file_path: &Path, custom_av_path: Option<&str>) -> String {
    #[cfg(target_os = "windows")]
    {
        let av_path = if let Some(p) = custom_av_path.filter(|p| !p.trim().is_empty()) {
            PathBuf::from(p)
        } else {
            let p1 = PathBuf::from(r"C:\Program Files\Windows Defender\MpCmdRun.exe");
            if p1.exists() {
                p1
            } else {
                PathBuf::from(r"C:\ProgramData\Microsoft\Windows Defender\Platform\MpCmdRun.exe")
            }
        };

        if !av_path.exists() {
            return "skipped".to_string();
        }

        let res = crate::process::command(&av_path)
            .args(["-Scan", "-ScanType", "3", "-File"])
            .arg(file_path)
            .output();

        match res {
            Ok(output) => match output.status.code() {
                Some(0) => "clean".to_string(),
                Some(2) => "threat_detected".to_string(),
                _ => "skipped".to_string(),
            },
            Err(_) => "skipped".to_string(),
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = file_path;
        let _ = custom_av_path;
        "skipped".to_string()
    }
}
