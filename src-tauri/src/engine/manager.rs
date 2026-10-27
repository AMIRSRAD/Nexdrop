use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex as AsyncMutex};
use uuid::Uuid;

use crate::db::models::{
    AddDownloadRequest, AddTorrentRequest, AppSettings, DownloadProgressEvent, DownloadQueue, DownloadStatus, DownloadTask,
    FileCategory, QueueInfo, SiteCredential, MAIN_QUEUE_ID,
};
use crate::engine::queues;
use crate::engine::speed::RateLimiter;
use crate::db::Repository;
use crate::engine::downloader::DownloadEngine;
use crate::engine::auth::{build_header_list, headers_for_credential, is_forbidden_header};
use crate::engine::checksum::{check_expected, normalize_expected_checksum};
use crate::engine::probe::{probe_url_with, sanitize_filename, ProbeOptions, ProbeResult};
use crate::engine::security;
use crate::engine::stream::{self, Fetch};
use crate::engine::torrent::{TorrentEngine, TorrentLive, TorrentSettings, TorrentSource};
use crate::engine::segment::{calculate_segments, calculate_segments_with_chunk_size};

/// What the tray shows.
#[derive(Default, Clone, Copy, Debug)]
pub struct StatusSummary {
    pub downloading: u32,
    pub queued: u32,
    pub paused: u32,
    pub speed_bps: u64,
}

/// Live state of one queue (never saved).
#[derive(Default)]
struct QueueRuntime {
    /// "Start now" (or a start-time trigger) opened the queue until it empties.
    forced_open: bool,
    was_open: Option<bool>,
    last_checked: Option<chrono::DateTime<chrono::Local>>,
}

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
    /// Downloads that finished since the queue was last idle ("when all finish" actions).
    completed_in_batch: AtomicUsize,
    /// Downloads the scheduler paused when its window closed; resumed when it reopens.
    scheduler_paused: Mutex<HashSet<String>>,
    /// Cancel flag of a pending sleep/shutdown countdown, if one is running.
    pending_power: Mutex<Option<Arc<AtomicBool>>>,
    /// What each queue is doing right now (forced open, last evaluation).
    queue_runtime: Mutex<HashMap<String, QueueRuntime>>,
    /// Speed cap shared by every download of a queue.
    queue_limiters: Mutex<HashMap<String, Arc<RateLimiter>>>,
    /// Downloads finished per queue since it was last empty ("when finished" actions).
    queue_batches: Mutex<HashMap<String, usize>>,
    /// Automatic retries used so far per download.
    retries_used: Mutex<HashMap<String, u32>>,
    /// BitTorrent session (created on first use).
    torrents: TorrentEngine,
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
            completed_in_batch: AtomicUsize::new(0),
            scheduler_paused: Mutex::new(HashSet::new()),
            pending_power: Mutex::new(None),
            queue_runtime: Mutex::new(HashMap::new()),
            queue_limiters: Mutex::new(HashMap::new()),
            queue_batches: Mutex::new(HashMap::new()),
            retries_used: Mutex::new(HashMap::new()),
            torrents: TorrentEngine::new(),
            app_handle,
        }
    }

    fn get_db_conn(&self) -> rusqlite::Result<rusqlite::Connection> {
        Repository::open_or_create(&self.db_path)
    }

    /// Addresses of the network adapters downloads should use (empty = let Windows choose).
    pub fn local_addresses(&self) -> Vec<std::net::IpAddr> {
        let settings = self.get_settings();
        if settings.network_mode == "selected" && !settings.network_adapters.is_empty() {
            crate::network::addresses_for(&settings.network_adapters)
        } else {
            Vec::new()
        }
    }

    pub fn get_settings(&self) -> AppSettings {
        if let Ok(conn) = self.get_db_conn() {
            Repository::load_settings(&conn).unwrap_or_default()
        } else {
            AppSettings::default()
        }
    }

    /// One-time upgrades of saved settings when a default changes.
    /// Version 2: "Sort downloads into folders by type" became the default.
    pub fn migrate_settings(&self) {
        let Ok(conn) = self.get_db_conn() else { return };
        let Ok(mut settings) = Repository::load_settings(&conn) else { return };
        if settings.settings_version >= crate::db::models::CURRENT_SETTINGS_VERSION {
            return;
        }
        if settings.settings_version < 2 {
            settings.auto_categorize = true;
        }
        settings.settings_version = crate::db::models::CURRENT_SETTINGS_VERSION;
        let _ = Repository::save_settings(&conn, &settings);
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_settings(&conn, settings).map_err(|e| e.to_string())?;
        self.apply_runtime_settings();
        Ok(())
    }

    /// Logins for the UI. Secrets never leave the backend: the settings page only
    /// needs to know a login exists, so passwords and tokens are masked.
    pub fn get_all_site_credentials(&self) -> Result<Vec<SiteCredential>, String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        let mut list = Repository::get_all_site_credentials(&conn).map_err(|e| e.to_string())?;
        for cred in &mut list {
            if cred.password.as_deref().map(|p| !p.is_empty()).unwrap_or(false) {
                cred.password = Some("••••••••".to_string());
            }
            if cred.token_or_cookie.as_deref().map(|t| !t.is_empty()).unwrap_or(false) {
                cred.token_or_cookie = Some("••••••••".to_string());
            }
        }
        Ok(list)
    }

    pub fn save_site_credential(&self, cred: SiteCredential) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_site_credential(&conn, &cred).map_err(|e| e.to_string())
    }

    /// Encrypts logins that older versions stored as plaintext (run once at startup).
    pub fn protect_stored_secrets(&self) {
        if let Ok(conn) = self.get_db_conn() {
            if let Ok(n) = Repository::protect_existing_secrets(&conn) {
                if n > 0 {
                    log::info!("Encrypted {n} stored site login(s)");
                }
            }
        }
    }

    /// Headers from the user's saved login for this link's site, if any.
    fn vault_headers(&self, url: &str) -> Vec<(String, String)> {
        let Some(host) = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_lowercase)) else {
            return Vec::new();
        };
        self.get_db_conn()
            .ok()
            .and_then(|conn| Repository::get_site_credential_for_domain(&conn, &host).ok().flatten())
            .map(|cred| headers_for_credential(url, &cred))
            .unwrap_or_default()
    }

    pub fn delete_site_credential(&self, id: &str) -> Result<(), String> {
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::delete_site_credential(&conn, id).map_err(|e| e.to_string())
    }

    /// Applies settings that take effect while the app runs. Called at startup,
    /// after settings change, and when the scheduler window opens or closes.
    pub fn apply_runtime_settings(&self) {
        let settings = self.get_settings();
        let limit = settings.global_speed_limit;
        self.engine.set_global_speed_limit(limit);
        self.torrents.apply_limits(settings.torrent_upload_limit, limit);
    }

    fn torrent_settings(&self) -> TorrentSettings {
        let settings = self.get_settings();
        let limit = settings.global_speed_limit;
        TorrentSettings {
            upload_limit: settings.torrent_upload_limit,
            download_limit: limit,
            dht: settings.torrent_dht,
            upnp: settings.torrent_upnp,
            seed_after_download: settings.torrent_seed_after_download,
            default_dir: PathBuf::from(&settings.default_destination),
        }
    }

    /// Re-applies settings that depend on the schedule. (The queues own the
    /// schedule now; this stays for callers that only need the global cap refreshed.)
    pub fn set_scheduler_window(&self, _open: bool) {
        self.apply_runtime_settings();
    }

    /// Probes a link with the right proxy and request details, reusing a result
    /// from the last minute if we have one.
    pub async fn probe(
        &self,
        url: &str,
        proxy_mode: Option<&str>,
        custom_proxy: Option<&str>,
        opts: &ProbeOptions,
    ) -> Result<ProbeResult, String> {
        const TTL: Duration = Duration::from_secs(60);
        // Headers can hold secrets: key the cache on a hash of them, never the values.
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        opts.user_agent.hash(&mut hasher);
        opts.referer.hash(&mut hasher);
        for (name, value) in &opts.headers {
            name.as_str().hash(&mut hasher);
            value.as_bytes().hash(&mut hasher);
        }
        let key = format!("{url}|{}|{}|{:x}", proxy_mode.unwrap_or(""), custom_proxy.unwrap_or(""), hasher.finish());
        {
            let mut cache = self.probe_cache.lock();
            cache.retain(|_, (at, _)| at.elapsed() < TTL);
            if let Some((_, hit)) = cache.get(&key) {
                return Ok(hit.clone());
            }
        }
        let client = self.engine.clients().probe(proxy_mode, custom_proxy);
        let result = probe_url_with(&client, url, opts).await?;
        self.probe_cache.lock().insert(key, (Instant::now(), result.clone()));
        Ok(result)
    }

    /// Probe for the "new download" dialog, using the proxy from settings. A link
    /// that came from the browser can bring its referer, user agent and cookies,
    /// so files behind a login can be inspected too.
    pub async fn probe_with_settings(
        &self,
        url: &str,
        referer: Option<String>,
        user_agent: Option<String>,
        headers: HashMap<String, String>,
    ) -> Result<ProbeResult, String> {
        let settings = self.get_settings();
        let headers: HashMap<String, String> = headers.into_iter().filter(|(k, _)| !is_forbidden_header(k)).collect();
        let opts = ProbeOptions {
            headers: build_header_list(&headers, &self.vault_headers(url)),
            referer,
            user_agent,
        };
        self.probe(url, Some(&settings.proxy_mode), settings.custom_proxy_url.as_deref(), &opts).await
    }

    pub async fn add_download(self: &Arc<Self>, req: AddDownloadRequest) -> Result<DownloadTask, String> {
        // HLS / DASH links are downloaded segment by segment, not as one file.
        if req.stream_variant.is_some() || stream::is_stream_url(&req.url) {
            return self.add_stream(req).await;
        }
        let settings = self.get_settings();

        let parsed = url::Url::parse(req.url.trim()).map_err(|e| format!("Invalid link: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Only http and https links can be downloaded".to_string());
        }
        let expected_checksum = match normalize_expected_checksum(req.expected_checksum.as_deref().unwrap_or("")) {
            Ok(Some((_, hash))) => Some(hash),
            Ok(None) => None,
            Err(e) => return Err(e),
        };
        let custom_headers: HashMap<String, String> = req
            .custom_headers
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty() && !is_forbidden_header(k))
            .collect();

        let proxy_mode_to_use = req.proxy_mode.as_deref().unwrap_or(&settings.proxy_mode);
        let custom_proxy_to_use = req.custom_proxy_url.as_deref().or(settings.custom_proxy_url.as_deref());
        let probe_opts = ProbeOptions {
            user_agent: req.user_agent.clone(),
            referer: req.referer.clone(),
            headers: build_header_list(&custom_headers, &self.vault_headers(&req.url)),
        };
        let probe = self
            .probe(&req.url, Some(proxy_mode_to_use), custom_proxy_to_use, &probe_opts)
            .await?;
        // A link without a telltale extension can still turn out to be a playlist.
        if probe.mime_type.as_deref().map(stream::is_stream_mime).unwrap_or(false) {
            return self.add_stream(req).await;
        }

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

        let queue_id = self.valid_queue_id(req.queue_id.as_deref());
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
            } else if req.hold == Some(true) {
                DownloadStatus::Paused
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
            checksum_sha256: None,
            checksum_md5: None,
            user_agent: req.user_agent,
            referer: req.referer,
            mirrors: req.mirrors.unwrap_or_default(),
            min_chunk_size,
            proxy_mode: Some(proxy_mode_to_use.to_string()),
            custom_proxy_url: custom_proxy_to_use.map(str::to_string),
            antivirus_status: None,
            expected_checksum,
            custom_headers,
            kind: "http".to_string(),
            info_hash: None,
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: vec![],
            queue_id,
        };

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_download(&conn, &task).map_err(|e| e.to_string())?;

        // If not scheduled, check if queue has capacity to start immediately
        if task.status == DownloadStatus::Queued {
            self.process_queue().await;
        }

        Ok(task)
    }

    // ───────────────────────────── streaming video ─────────────────────────────

    fn stream_fetch(&self, url: &str, referer: Option<String>, user_agent: Option<String>) -> Fetch {
        let settings = self.get_settings();
        let client = self.engine.clients().download(Some(&settings.proxy_mode), settings.custom_proxy_url.as_deref());
        let headers = build_header_list(&HashMap::new(), &self.vault_headers(url));
        Fetch::new(client, headers, url, referer, user_agent, Duration::from_millis(500))
    }

    /// Looks inside an HLS / DASH link without downloading anything.
    pub async fn probe_stream(&self, url: &str, referer: Option<String>, user_agent: Option<String>) -> Result<stream::StreamProbe, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("Invalid link: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Only http and https links can be downloaded".to_string());
        }
        stream::probe(&self.stream_fetch(url, referer, user_agent), url).await
    }

    /// Adds an HLS / DASH stream: reads its playlist now (so problems show up
    /// at once), then queues it like any other download.
    async fn add_stream(self: &Arc<Self>, req: AddDownloadRequest) -> Result<DownloadTask, String> {
        let settings = self.get_settings();
        let url = req.url.trim().to_string();
        let parsed = url::Url::parse(&url).map_err(|e| format!("Invalid link: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("Only http and https links can be downloaded".to_string());
        }
        let custom_headers: HashMap<String, String> = req
            .custom_headers
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty() && !is_forbidden_header(k))
            .collect();

        let proxy_mode = req.proxy_mode.clone().unwrap_or_else(|| settings.proxy_mode.clone());
        let custom_proxy = req.custom_proxy_url.clone().or_else(|| settings.custom_proxy_url.clone());
        let client = self.engine.clients().download(Some(&proxy_mode), custom_proxy.as_deref());
        let headers = build_header_list(&custom_headers, &self.vault_headers(&url));
        let fetch = Fetch::new(client, headers, &url, req.referer.clone(), req.user_agent.clone(), Duration::from_millis(500));

        let resolved = stream::resolve(&fetch, &url, req.stream_variant.as_deref()).await?;
        let extension = resolved.extension();
        let stem = req
            .filename
            .as_deref()
            .filter(|f| !f.trim().is_empty())
            .map(|f| Path::new(f).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| f.to_string()))
            .unwrap_or_else(|| resolved.title.clone());
        let filename = sanitize_filename(&format!("{stem}.{extension}"));

        let mut dest_dir = PathBuf::from(
            req.destination_path.clone().filter(|d| !d.trim().is_empty()).unwrap_or_else(|| settings.default_destination.clone()),
        );
        if settings.auto_categorize {
            let sub = FileCategory::Videos.subfolder_name();
            if !sub.is_empty() {
                dest_dir = dest_dir.join(sub);
            }
        }
        let _ = std::fs::create_dir_all(&dest_dir);

        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        // The chosen quality travels in `etag` (unused by streams otherwise): when the
        // download starts again later, the playlist is read afresh and this picks the same quality.
        let variant = resolved.variant.clone();
        let task = DownloadTask {
            id,
            url: url.clone(),
            final_url: None,
            filename: filename.clone(),
            destination_path: dest_dir.to_string_lossy().to_string(),
            temp_path: String::new(),
            total_bytes: resolved.estimated_bytes() as i64,
            downloaded_bytes: 0,
            status: if req.scheduled_time.is_some() {
                DownloadStatus::Scheduled
            } else if req.hold == Some(true) {
                DownloadStatus::Paused
            } else {
                DownloadStatus::Queued
            },
            resumable: true,
            etag: variant,
            last_modified: None,
            mime_type: Some(if extension == "ts" { "video/mp2t" } else { "video/mp4" }.to_string()),
            category: FileCategory::Videos,
            // Segments are small, so speed is limited by round trips, not bandwidth: many at once.
            connections: 16,
            speed_limit: req.speed_limit.unwrap_or(0),
            priority: req.priority.unwrap_or(0),
            speed_bps: 0,
            eta_seconds: None,
            segments: vec![],
            created_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
            scheduled_time: req.scheduled_time,
            checksum_sha256: None,
            checksum_md5: None,
            user_agent: req.user_agent,
            referer: req.referer,
            mirrors: vec![],
            min_chunk_size: 1024 * 1024,
            proxy_mode: Some(proxy_mode),
            custom_proxy_url: custom_proxy,
            antivirus_status: None,
            expected_checksum: None,
            custom_headers,
            kind: "stream".to_string(),
            info_hash: None,
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: vec![],
            queue_id: self.valid_queue_id(req.queue_id.as_deref()),
        };
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_download(&conn, &task).map_err(|e| e.to_string())?;
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

    /// Startup: recover interrupted downloads and resume them. Queues that are
    /// outside their schedule keep their downloads waiting.
    pub async fn startup_resume(self: &Arc<Self>) {
        self.migrate_settings();
        self.ensure_queues();
        self.apply_runtime_settings();

        self.recover_interrupted();
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move { manager.resume_seeding_after_start().await });
        if !self.get_settings().auto_resume_on_launch {
            return;
        }
        self.process_queue().await;
    }

    // ───────────────────────────── torrents ─────────────────────────────

    /// Resolves a torrent's contents (files, sizes) without downloading anything.
    pub async fn probe_torrent(&self, source: TorrentSource) -> Result<crate::db::models::TorrentProbeInfo, String> {
        if let TorrentSource::Url(url) = &source {
            let lower = url.trim().to_lowercase();
            let ok = lower.starts_with("magnet:?") || lower.starts_with("http://") || lower.starts_with("https://");
            if !ok {
                return Err("That isn't a magnet link or a link to a .torrent file".to_string());
            }
        }
        let probe = self.torrents.probe(source, &self.torrent_settings()).await?;
        Ok(crate::db::models::TorrentProbeInfo {
            info_hash: probe.info_hash,
            name: probe.name,
            total_bytes: probe.total_bytes,
            files: probe.files,
        })
    }

    /// Adds a torrent to the list (and starts it when there's room in the queue).
    pub async fn add_torrent(self: &Arc<Self>, req: AddTorrentRequest) -> Result<DownloadTask, String> {
        let settings = self.get_settings();

        // The details come from an earlier probe when there was one, otherwise fetch them now.
        let cached = req.info_hash.as_deref().and_then(|h| self.torrents.cached_torrent_bytes(h));
        let (probe, torrent_bytes) = match cached {
            Some(bytes) => {
                let p = self.torrents.probe(TorrentSource::Bytes(bytes.clone()), &self.torrent_settings()).await?;
                (p, bytes)
            }
            None => {
                let source = req.source.clone().ok_or_else(|| "Give a magnet link or a .torrent file".to_string())?;
                let p = self.torrents.probe(TorrentSource::Url(source), &self.torrent_settings()).await?;
                let bytes = p.torrent_bytes.clone();
                (p, bytes)
            }
        };

        if let Ok(conn) = self.get_db_conn() {
            if Repository::find_by_info_hash(&conn, &probe.info_hash).ok().flatten().is_some() {
                return Err("This torrent is already in your list".to_string());
            }
        }

        let selected: Vec<usize> = match &req.selected_files {
            Some(list) => list.clone(),
            None => probe.files.iter().map(|f| f.index).collect(),
        };
        let files: Vec<crate::db::models::TorrentFile> = probe
            .files
            .iter()
            .map(|f| crate::db::models::TorrentFile { selected: selected.contains(&f.index), ..f.clone() })
            .collect();
        let total: u64 = files.iter().filter(|f| f.selected).map(|f| f.length).sum();
        if total == 0 {
            return Err("Select at least one file to download".to_string());
        }

        let dest_dir = req
            .destination_path
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| settings.default_destination.clone());
        let _ = std::fs::create_dir_all(&dest_dir);

        // Display / open name: the file itself for single-file torrents, the folder otherwise.
        let filename = if files.len() == 1 {
            files[0].path.rsplit('/').next().unwrap_or(&files[0].path).to_string()
        } else {
            sanitize_filename(&probe.name)
        };
        let category = if files.len() == 1 { FileCategory::from_filename_or_mime(&filename, None) } else { FileCategory::Other };

        let now = chrono::Utc::now().timestamp();
        let id = Uuid::new_v4().to_string();
        let queue_id = self.valid_queue_id(req.queue_id.as_deref());
        let task = DownloadTask {
            id: id.clone(),
            url: req.source.clone().unwrap_or_else(|| format!("magnet:?xt=urn:btih:{}", probe.info_hash)),
            final_url: None,
            filename,
            destination_path: dest_dir,
            temp_path: String::new(),
            total_bytes: total as i64,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            resumable: true,
            etag: None,
            last_modified: None,
            mime_type: None,
            category,
            connections: 1,
            speed_limit: 0,
            priority: 0,
            speed_bps: 0,
            eta_seconds: None,
            segments: vec![],
            created_at: now,
            updated_at: now,
            completed_at: None,
            error_message: None,
            scheduled_time: None,
            checksum_sha256: None,
            checksum_md5: None,
            user_agent: None,
            referer: None,
            mirrors: vec![],
            min_chunk_size: 0,
            proxy_mode: None,
            custom_proxy_url: None,
            antivirus_status: None,
            expected_checksum: None,
            custom_headers: HashMap::new(),
            kind: "torrent".to_string(),
            info_hash: Some(probe.info_hash.clone()),
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: files,
            queue_id,
        };

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_download(&conn, &task).map_err(|e| e.to_string())?;
        Repository::save_torrent_bytes(&conn, &id, &torrent_bytes).map_err(|e| e.to_string())?;
        drop(conn);

        self.process_queue().await;
        Ok(task)
    }

    /// Runs one torrent task through the torrent engine (called from `start_download`).
    async fn run_torrent_task<F>(
        self: &Arc<Self>,
        task: DownloadTask,
        pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let (bytes, resume) = match self.get_db_conn() {
            Ok(conn) => {
                let bytes = Repository::get_torrent_bytes(&conn, &task.id).ok().flatten();
                let resume = Repository::torrent_started(&conn, &task.id).unwrap_or(false);
                let _ = Repository::mark_torrent_started(&conn, &task.id);
                (bytes, resume)
            }
            Err(_) => (None, false),
        };
        let app = self.app_handle.clone();
        self.torrents
            .run(task, bytes, resume, pause_rx, &self.torrent_settings(), on_progress, move |live: TorrentLive| {
                let _ = app.emit("torrent-live", &live);
            })
            .await
    }

    /// Stops sharing a finished torrent (it stays in the list, completed).
    pub async fn stop_seeding(&self, id: &str) -> Result<(), String> {
        let task = self.get_download(id)?.ok_or_else(|| "Download not found".to_string())?;
        if let Some(hash) = &task.info_hash {
            self.torrents.stop(hash).await;
        }
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::update_torrent_state(&conn, id, task.uploaded_bytes, false).map_err(|e| e.to_string())?;
        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({ "id": id }));
        Ok(())
    }

    /// Starts sharing a finished torrent again.
    pub async fn start_seeding(&self, id: &str) -> Result<(), String> {
        let task = self.get_download(id)?.ok_or_else(|| "Download not found".to_string())?;
        if task.kind != "torrent" || task.status != DownloadStatus::Completed {
            return Err("Only finished torrents can be shared".to_string());
        }
        let bytes = self.get_db_conn().ok().and_then(|c| Repository::get_torrent_bytes(&c, id).ok().flatten());
        self.torrents.start_seeding(&task, bytes, &self.torrent_settings()).await?;
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::update_torrent_state(&conn, id, task.uploaded_bytes, true).map_err(|e| e.to_string())?;
        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({ "id": id }));
        Ok(())
    }

    // ───────────────────────────── tray / exit ─────────────────────────────

    pub fn status_summary(&self) -> StatusSummary {
        match self.get_db_conn().and_then(|c| Repository::status_summary(&c)) {
            Ok((downloading, queued, paused, speed_bps)) => StatusSummary { downloading, queued, paused, speed_bps },
            Err(_) => StatusSummary::default(),
        }
    }

    /// Pauses everything that is running or waiting.
    pub fn pause_all(&self) {
        if let Ok(tasks) = self.get_all_downloads() {
            for task in tasks {
                if matches!(task.status, DownloadStatus::Downloading | DownloadStatus::Queued) {
                    let _ = self.pause_download(&task.id);
                }
            }
        }
    }

    /// Resumes everything that was paused.
    pub async fn resume_all(self: &Arc<Self>) {
        if let Ok(tasks) = self.get_all_downloads() {
            for task in tasks.into_iter().filter(|t| t.status == DownloadStatus::Paused) {
                let _ = self.resume_download(&task.id).await;
            }
        }
    }

    /// Called when the app is quitting: stop every running download cleanly (progress
    /// is saved) but leave it marked as in progress, so it carries on at the next launch.
    pub async fn shutdown(&self) {
        {
            let active = self.active_controllers.lock();
            for tx in active.values() {
                let _ = tx.send(true);
            }
        }
        for _ in 0..100 {
            if self.active_controllers.lock().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        self.torrents.shutdown().await;
    }

    /// Housekeeping for torrents that are being shared: report their live numbers
    /// and stop them once the ratio goal is reached. Called every few seconds.
    pub async fn seeding_tick(&self) {
        if !self.torrents.has_session() {
            return;
        }
        let settings = self.get_settings();
        let Ok(tasks) = self.get_all_downloads() else { return };
        for task in tasks.iter().filter(|t| t.kind == "torrent" && t.status == DownloadStatus::Completed && t.seeding) {
            let Some(live) = self.torrents.live(task) else { continue };
            let _ = self.app_handle.emit("torrent-live", &live);
            if let Ok(conn) = self.get_db_conn() {
                let _ = Repository::update_torrent_state(&conn, &task.id, live.uploaded_bytes as i64, true);
            }
            let goal_met = settings.torrent_ratio_limit > 0.0 && live.ratio >= settings.torrent_ratio_limit;
            if goal_met || !settings.torrent_seed_after_download {
                let _ = self.stop_seeding(&task.id).await;
            }
        }
    }

    /// After a restart: bring finished torrents that were being shared back online.
    async fn resume_seeding_after_start(self: &Arc<Self>) {
        let settings = self.get_settings();
        if !settings.torrent_seed_after_download {
            return;
        }
        let Ok(tasks) = self.get_all_downloads() else { return };
        for task in tasks.into_iter().filter(|t| t.kind == "torrent" && t.status == DownloadStatus::Completed && t.seeding) {
            let bytes = self.get_db_conn().ok().and_then(|c| Repository::get_torrent_bytes(&c, &task.id).ok().flatten());
            if let Err(e) = self.torrents.start_seeding(&task, bytes, &self.torrent_settings()).await {
                log::warn!("Could not resume sharing {}: {e}", task.filename);
                if let Ok(conn) = self.get_db_conn() {
                    let _ = Repository::update_torrent_state(&conn, &task.id, task.uploaded_bytes, false);
                }
            }
        }
    }

    // ───────────────────────────── scheduler support ─────────────────────────────

    /// Moves downloads whose scheduled time has come into the queue and starts them.
    pub async fn start_due_scheduled(self: &Arc<Self>) {
        let now = chrono::Utc::now().timestamp();
        let mut any = false;
        if let Ok(conn) = self.get_db_conn() {
            for task in Repository::get_all_downloads(&conn).unwrap_or_default() {
                let due = task.status == DownloadStatus::Scheduled
                    && task.scheduled_time.map(|t| crate::engine::scheduler::to_unix_seconds(t) <= now).unwrap_or(true);
                if due {
                    let _ = Repository::update_status(&conn, &task.id, DownloadStatus::Queued, None, None, now);
                    any = true;
                }
            }
        }
        if any {
            let _ = self.app_handle.emit("download-status-changed", serde_json::json!({}));
            self.process_queue().await;
        }
    }

    // ───────────────────────────── queues ─────────────────────────────

    /// The queues, oldest first. The Main queue always exists: it is created
    /// from the old single-scheduler settings the first time.
    pub fn ensure_queues(&self) -> Vec<DownloadQueue> {
        let Ok(conn) = self.get_db_conn() else { return vec![queues::main_queue_from_settings(&self.get_settings())] };
        let mut list = Repository::list_queues(&conn).unwrap_or_default();
        if !list.iter().any(|q| q.id == MAIN_QUEUE_ID) {
            let main = queues::main_queue_from_settings(&self.get_settings());
            let _ = Repository::save_queue(&conn, &main);
            list.insert(0, main);
        }
        list
    }

    pub fn queue_by_id(&self, id: &str) -> Option<DownloadQueue> {
        self.ensure_queues().into_iter().find(|q| q.id == id)
    }

    /// `requested` if such a queue exists, otherwise the Main queue.
    fn valid_queue_id(&self, requested: Option<&str>) -> String {
        match requested {
            Some(id) if !id.is_empty() && self.queue_by_id(id).is_some() => id.to_string(),
            _ => MAIN_QUEUE_ID.to_string(),
        }
    }

    fn queue_limiter(&self, q: &DownloadQueue) -> Arc<RateLimiter> {
        let mut map = self.queue_limiters.lock();
        let limiter = map.entry(q.id.clone()).or_insert_with(|| Arc::new(RateLimiter::new(0)));
        limiter.set_rate(q.speed_limit);
        limiter.clone()
    }

    fn queue_forced_open(&self, id: &str) -> bool {
        self.queue_runtime.lock().get(id).map(|r| r.forced_open).unwrap_or(false)
    }

    pub fn queue_is_open(&self, q: &DownloadQueue) -> bool {
        queues::is_open(q, self.queue_forced_open(&q.id), chrono::Local::now())
    }

    /// Queues with live numbers for the interface.
    pub fn queue_infos(&self) -> Vec<QueueInfo> {
        let all = self
            .get_db_conn()
            .and_then(|conn| Repository::get_all_downloads(&conn))
            .unwrap_or_default();
        let active = self.get_active_task_ids();
        let now = chrono::Local::now();
        self.ensure_queues()
            .into_iter()
            .map(|queue| {
                let open = self.queue_is_open(&queue);
                let in_queue = |t: &&DownloadTask| t.queue_id == queue.id;
                let active_n = all.iter().filter(in_queue).filter(|t| active.contains(&t.id)).count();
                let queued = all.iter().filter(in_queue).filter(|t| t.status == DownloadStatus::Queued && !active.contains(&t.id)).count();
                let next_start = if open { None } else { queues::next_start(&queue, now).map(|t| t.timestamp()) };
                QueueInfo { queue, open, active: active_n, queued, next_start }
            })
            .collect()
    }

    pub fn save_queue(&self, queue: DownloadQueue) -> Result<DownloadQueue, String> {
        let existing = self.ensure_queues();
        let mut queue = queues::sanitize(queue)?;
        if queue.id.is_empty() {
            queue.id = Uuid::new_v4().to_string();
            queue.position = existing.iter().map(|q| q.position).max().unwrap_or(0) + 1;
        } else if let Some(old) = existing.iter().find(|q| q.id == queue.id) {
            queue.position = old.position;
        }
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::save_queue(&conn, &queue).map_err(|e| e.to_string())?;
        // A changed schedule is picked up on the next tick (which also pauses or
        // resumes the queue if the new schedule says it should be closed or open).
        if let Some(rt) = self.queue_runtime.lock().get_mut(&queue.id) {
            rt.last_checked = None;
        }
        if let Some(limiter) = self.queue_limiters.lock().get(&queue.id) {
            limiter.set_rate(queue.speed_limit);
        }
        let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        Ok(queue)
    }

    /// Deletes a queue; its downloads move to the Main queue.
    pub fn delete_queue(&self, id: &str) -> Result<(), String> {
        if id == MAIN_QUEUE_ID {
            return Err("The Main queue can't be deleted".to_string());
        }
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::delete_queue(&conn, id, MAIN_QUEUE_ID).map_err(|e| e.to_string())?;
        self.queue_runtime.lock().remove(id);
        self.queue_limiters.lock().remove(id);
        let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({}));
        Ok(())
    }

    pub async fn move_to_queue(self: &Arc<Self>, ids: Vec<String>, queue_id: &str) -> Result<(), String> {
        let queue_id = self.valid_queue_id(Some(queue_id));
        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        Repository::set_download_queue(&conn, &ids, &queue_id).map_err(|e| e.to_string())?;
        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({}));
        let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        self.process_queue().await;
        Ok(())
    }

    /// Ids of the downloads in `queue_id` that are not finished.
    fn unfinished_in_queue(&self, queue_id: &str) -> Vec<DownloadTask> {
        self.get_db_conn()
            .and_then(|conn| Repository::get_all_downloads(&conn))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.queue_id == queue_id && matches!(t.status, DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Paused))
            .collect()
    }

    /// "Start now": opens the queue regardless of its schedule, resumes what
    /// was paused in it and starts working through it.
    pub async fn start_queue(self: &Arc<Self>, id: &str) -> Result<(), String> {
        if self.queue_by_id(id).is_none() {
            return Err("Queue not found".to_string());
        }
        self.queue_runtime.lock().entry(id.to_string()).or_default().forced_open = true;
        if let Ok(conn) = self.get_db_conn() {
            let now = chrono::Utc::now().timestamp();
            for task in self.unfinished_in_queue(id) {
                if task.status == DownloadStatus::Paused {
                    let _ = Repository::update_status(&conn, &task.id, DownloadStatus::Queued, None, None, now);
                }
            }
        }
        self.scheduler_paused.lock().retain(|tid| {
            // Forgotten: everything paused in this queue was just re-queued.
            self.get_db_conn().ok().and_then(|c| Repository::get_download(&c, tid).ok().flatten()).map(|t| t.queue_id != id).unwrap_or(false)
        });
        let _ = self.app_handle.emit("download-status-changed", serde_json::json!({}));
        let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        self.process_queue().await;
        Ok(())
    }

    /// "Stop": pauses everything running or waiting in the queue and takes back
    /// any "Start now".
    pub async fn stop_queue(self: &Arc<Self>, id: &str) -> Result<(), String> {
        if self.queue_by_id(id).is_none() {
            return Err("Queue not found".to_string());
        }
        self.queue_runtime.lock().entry(id.to_string()).or_default().forced_open = false;
        for task in self.unfinished_in_queue(id) {
            if task.status != DownloadStatus::Paused {
                let _ = self.pause_download(&task.id);
            }
        }
        let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        Ok(())
    }

    /// Called every few seconds: opens and closes queues as their schedules say.
    pub async fn queue_tick(self: &Arc<Self>) {
        let now = chrono::Local::now();
        let mut opened_any = false;
        let mut changed = false;
        for queue in self.ensure_queues() {
            // Start-time triggers: did the start time pass since we last looked?
            let (forced, was_open, since) = {
                let mut map = self.queue_runtime.lock();
                let rt = map.entry(queue.id.clone()).or_default();
                // On the first look, catch a start time that passed in the last few minutes.
                let since = rt.last_checked.unwrap_or(now - chrono::Duration::minutes(5));
                if queues::start_crossed(&queue, since, now) {
                    rt.forced_open = true;
                }
                rt.last_checked = Some(now);
                (rt.forced_open, rt.was_open, since)
            };
            let _ = (forced, since);
            let open = self.queue_is_open(&queue);

            if was_open != Some(open) {
                self.queue_runtime.lock().entry(queue.id.clone()).or_default().was_open = Some(open);
                changed = true;
                match was_open {
                    // First evaluation. A closed queue must not have anything running
                    // (e.g. its schedule was edited before we ever saw it open).
                    None if open => opened_any = true,
                    None => {
                        let paused = self.pause_queue_for_schedule(&queue.id).await;
                        if paused > 0 {
                            let _ = self.app_handle.emit(
                                "scheduler-event",
                                serde_json::json!({ "event": "stopped", "queue": queue.name, "paused": paused }),
                            );
                        }
                    }
                    Some(false) if open => {
                        opened_any = true;
                        self.resume_scheduler_paused(&queue.id);
                        let _ = self.app_handle.emit(
                            "scheduler-event",
                            serde_json::json!({ "event": "started", "queue": queue.name }),
                        );
                    }
                    Some(true) if !open => {
                        let paused = self.pause_queue_for_schedule(&queue.id).await;
                        let _ = self.app_handle.emit(
                            "scheduler-event",
                            serde_json::json!({ "event": "stopped", "queue": queue.name, "paused": paused }),
                        );
                        if paused > 0 {
                            self.run_queue_action(&queue);
                        }
                    }
                    _ => {}
                }
            }
        }
        if changed {
            let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        }
        if opened_any {
            let _ = self.app_handle.emit("download-status-changed", serde_json::json!({}));
            self.process_queue().await;
        }
    }

    /// A queue's window closed: pause what it was running and remember it.
    async fn pause_queue_for_schedule(&self, queue_id: &str) -> usize {
        let all = self.get_db_conn().and_then(|c| Repository::get_all_downloads(&c)).unwrap_or_default();
        let ids: Vec<String> = self
            .get_active_task_ids()
            .into_iter()
            .filter(|id| all.iter().any(|t| &t.id == id && t.queue_id == queue_id))
            .collect();
        for id in &ids {
            if self.pause_download(id).is_ok() {
                self.scheduler_paused.lock().insert(id.clone());
            }
        }
        // Give the runs a moment to stop and save their progress.
        for _ in 0..100 {
            if ids.iter().all(|id| !self.active_controllers.lock().contains_key(id)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        ids.len()
    }

    /// A queue's window opened: bring back what the schedule paused in it.
    fn resume_scheduler_paused(&self, queue_id: &str) {
        let Ok(conn) = self.get_db_conn() else { return };
        let now = chrono::Utc::now().timestamp();
        let ids: Vec<String> = self.scheduler_paused.lock().iter().cloned().collect();
        for id in ids {
            let Ok(Some(task)) = Repository::get_download(&conn, &id) else {
                self.scheduler_paused.lock().remove(&id);
                continue;
            };
            if task.queue_id != queue_id {
                continue;
            }
            self.scheduler_paused.lock().remove(&id);
            // Only if it's still sitting where the schedule left it.
            if task.status == DownloadStatus::Paused {
                let _ = Repository::update_status(&conn, &id, DownloadStatus::Queued, None, None, now);
            }
        }
    }

    /// Puts a failed download back in its queue after the queue's retry delay.
    fn schedule_auto_retry(self: &Arc<Self>, task_id: &str, queue_id: &str) {
        let Some(queue) = self.queue_by_id(queue_id) else { return };
        if queue.retry_count == 0 {
            return;
        }
        let used = {
            let mut map = self.retries_used.lock();
            let n = map.entry(task_id.to_string()).or_insert(0);
            if *n >= queue.retry_count {
                return;
            }
            *n += 1;
            *n
        };
        let (manager, id) = (Arc::clone(self), task_id.to_string());
        let delay = Duration::from_secs(queue.retry_delay_secs);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(delay).await;
            let Ok(conn) = manager.get_db_conn() else { return };
            // Still failed? (The user may have retried, paused or removed it meanwhile.)
            if let Ok(Some(task)) = Repository::get_download(&conn, &id) {
                if task.status == DownloadStatus::Failed {
                    let _ = Repository::update_status(&conn, &id, DownloadStatus::Queued, None, None, chrono::Utc::now().timestamp());
                    log::info!("Retrying {} automatically (attempt {used})", task.filename);
                    let _ = manager.app_handle.emit("download-status-changed", serde_json::json!({ "id": id }));
                    manager.process_queue().await;
                }
            }
        });
    }

    /// After a run ends: if its queue is now empty, finish the queue (clear
    /// "Start now" and run its "when finished" action).
    fn queue_finished_check(self: &Arc<Self>, queue_id: &str) {
        let active = self.get_active_task_ids();
        let busy = self
            .get_db_conn()
            .and_then(|conn| Repository::get_all_downloads(&conn))
            .map(|all| {
                all.iter().any(|t| {
                    t.queue_id == queue_id
                        && (active.contains(&t.id) || t.status == DownloadStatus::Queued)
                })
            })
            .unwrap_or(true);
        if busy {
            return;
        }
        let finished = self.queue_batches.lock().remove(queue_id).unwrap_or(0);
        let was_forced = self.queue_runtime.lock().get_mut(queue_id).map(|r| std::mem::take(&mut r.forced_open)).unwrap_or(false);
        if was_forced {
            let _ = self.app_handle.emit("queues-changed", serde_json::json!({}));
        }
        if finished == 0 {
            return;
        }
        if let Some(queue) = self.queue_by_id(queue_id) {
            self.run_queue_action(&queue);
        }
    }

    /// The queue's "when finished" action. Sleep/hibernate/shut down get a cancelable countdown.
    fn run_queue_action(self: &Arc<Self>, queue: &DownloadQueue) {
        match queue.after_done.as_str() {
            "sleep" => self.schedule_power_action("sleep", 30),
            "hibernate" => self.schedule_power_action("hibernate", 30),
            "shutdown" => self.schedule_power_action("shutdown", 60),
            "lock" => crate::power::lock_computer(),
            "exit" => crate::quit_app(&self.app_handle),
            "program" => {
                if let Some(program) = &queue.after_done_program {
                    let _ = crate::process::command(program).spawn();
                }
            }
            "notify" => {
                let settings = self.get_settings();
                crate::notify::toast(
                    &self.app_handle,
                    &format!("{} finished", queue.name),
                    "All downloads in this queue are done.",
                    settings.sound_enabled,
                );
            }
            _ => {}
        }
    }

    // ───────────────────────────── "when finished" actions ─────────────────────────────

    /// Runs the user's "when all downloads finish" action once everything has gone idle.
    fn run_completion_action_if_idle(self: &Arc<Self>) {
        if !self.active_controllers.lock().is_empty() {
            return;
        }
        let queued_left = self
            .get_db_conn()
            .and_then(|conn| Repository::get_all_downloads(&conn))
            .map(|all| all.iter().any(|t| t.status == DownloadStatus::Queued))
            .unwrap_or(false);
        if queued_left {
            return;
        }
        let finished = self.completed_in_batch.swap(0, Ordering::SeqCst);
        if finished == 0 {
            return;
        }

        let settings = self.get_settings();
        match settings.on_complete_action.as_str() {
            "sleep" => self.schedule_power_action("sleep", 30),
            "shutdown" => self.schedule_power_action("shutdown", 60),
            "notify" if finished > 1 => {
                if !crate::notify::window_has_focus(&self.app_handle) {
                    crate::notify::toast(
                        &self.app_handle,
                        "All downloads complete",
                        &format!("{finished} downloads finished."),
                        settings.sound_enabled,
                    );
                }
            }
            _ => {}
        }
    }

    /// Sleep / shut down after a countdown the user can cancel from the app.
    pub fn schedule_power_action(self: &Arc<Self>, action: &str, seconds: u64) {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self.pending_power.lock().replace(cancel.clone()) {
            previous.store(true, Ordering::SeqCst);
        }
        let _ = self.app_handle.emit(
            "completion-action",
            serde_json::json!({ "action": action, "seconds": seconds }),
        );

        let action = action.to_string();
        let manager = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            for _ in 0..seconds {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
            }
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            manager.pending_power.lock().take();
            match action.as_str() {
                "sleep" => crate::power::sleep_computer(),
                "hibernate" => crate::power::hibernate_computer(),
                "shutdown" => crate::power::shutdown_computer(),
                _ => {}
            }
        });
    }

    /// Cancels a pending sleep/shutdown countdown. Returns whether there was one.
    pub fn cancel_pending_power_action(&self) -> bool {
        match self.pending_power.lock().take() {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                let _ = self.app_handle.emit("completion-action-cancelled", serde_json::json!({}));
                true
            }
            None => false,
        }
    }

    pub async fn process_queue(self: &Arc<Self>) {
        // Only one scheduling pass at a time.
        let _guard = self.queue_lock.lock().await;

        let settings = self.get_settings();
        let active_ids: Vec<String> = self.active_controllers.lock().keys().cloned().collect();
        if active_ids.len() >= settings.max_active_downloads {
            return;
        }

        let slots_available = settings.max_active_downloads - active_ids.len();
        let all = match self.get_db_conn().and_then(|conn| Repository::get_all_downloads(&conn)) {
            Ok(tasks) => tasks,
            Err(_) => return,
        };

        // Which queues may start something, and how many each is already running.
        let queue_list = self.ensure_queues();
        let mut running: HashMap<&str, usize> = HashMap::new();
        for t in all.iter().filter(|t| active_ids.contains(&t.id)) {
            *running.entry(t.queue_id.as_str()).or_insert(0) += 1;
        }

        // Filter queued tasks, sorted by priority DESC, created_at ASC
        let mut queued: Vec<&DownloadTask> = all.iter().filter(|t| t.status == DownloadStatus::Queued).collect();
        queued.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.created_at.cmp(&b.created_at)));

        let mut started = 0;
        for task in queued {
            if started >= slots_available {
                break;
            }
            let queue = queue_list.iter().find(|q| q.id == task.queue_id).or_else(|| queue_list.iter().find(|q| q.id == MAIN_QUEUE_ID));
            if let Some(queue) = queue {
                if !self.queue_is_open(queue) {
                    continue;
                }
                let n = running.get(queue.id.as_str()).copied().unwrap_or(0);
                if queue.max_active > 0 && n >= queue.max_active {
                    continue;
                }
            }
            if self.start_download(task.clone()) {
                started += 1;
                if let Some(queue) = queue {
                    *running.entry(queue.id.as_str()).or_insert(0) += 1;
                }
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
        let engine = self.engine.clone().with_local_addresses(self.local_addresses());
        let is_torrent = task.kind == "torrent";
        let is_stream = task.kind == "stream";
        let queue_id = task.queue_id.clone();
        let task_queue = self.queue_by_id(&queue_id);
        let engine = match &task_queue {
            Some(q) => engine.with_group_limit(self.queue_limiter(q)),
            None => engine,
        };
        // Saved-login headers are looked up fresh on every start (the login may have changed).
        let vault_headers = self.vault_headers(&task.url);

        tauri::async_runtime::spawn(async move {
            let app_handle = manager.app_handle.clone();
            let db_path = manager.db_path.clone();

            // Progress goes to the UI on every tick, but to the database only
            // every couple of seconds and off the async threads.
            let clock = Instant::now();
            let last_db_write = Arc::new(AtomicU64::new(0));
            let progress_app = app_handle.clone();
            let progress_db = db_path.clone();

            let on_progress = move |event: DownloadProgressEvent| {
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
            };
            let res = if is_torrent {
                manager.run_torrent_task(task, pause_rx, on_progress).await
            } else if is_stream {
                engine.run_stream(task, pause_rx, vault_headers, on_progress).await
            } else {
                engine.run_download_with(task, pause_rx, vault_headers, on_progress).await
            };

            match res {
                // Stopped on request. The pause (or delete) already updated the
                // task's status; just remember how far we got. This is NOT a
                // completed download, so no hashing, scanning or "done" event.
                Ok(task) if task.status == DownloadStatus::Paused => {
                    if let Ok(conn) = Repository::open_or_create(&db_path) {
                        let _ = Repository::update_downloaded_bytes(&conn, &task_id, task.downloaded_bytes);
                        let _ = Repository::replace_segments(&conn, &task_id, &task.segments);
                        if task.kind == "torrent" {
                            let _ = Repository::update_torrent_state(&conn, &task_id, task.uploaded_bytes, false);
                        }
                    }
                }
                Ok(mut final_task) => {
                    let final_path = Path::new(&final_task.destination_path).join(&final_task.filename);
                    let settings = Repository::open_or_create(&db_path)
                        .ok()
                        .and_then(|c| Repository::load_settings(&c).ok())
                        .unwrap_or_default();

                    // Marking, hashing and scanning are blocking work: keep them off the async threads.
                    // The Mark of the Web goes on first so every later step (and the user)
                    // sees the file as downloaded from the Internet.
                    let job_path = final_path.clone();
                    let host_url = final_task.final_url.clone().or_else(|| Some(final_task.url.clone()));
                    let referer = final_task.referer.clone();
                    let skip_file_checks = final_task.kind == "torrent";
                    let hashes = tauri::async_runtime::spawn_blocking(move || {
                        if skip_file_checks {
                            // Torrents are verified piece by piece by the protocol, and their
                            // files were marked individually; there is no single file to hash.
                            return Err(std::io::Error::new(std::io::ErrorKind::Other, "torrent"));
                        }
                        security::mark_of_the_web(&job_path, host_url.as_deref(), referer.as_deref());
                        crate::engine::compute_checksums(&job_path)
                    })
                    .await;
                    let hashes = hashes.ok().and_then(|r| r.ok());
                    if let Some((sha256, md5)) = &hashes {
                        final_task.checksum_sha256 = Some(sha256.clone());
                        final_task.checksum_md5 = Some(md5.clone());
                    }

                    // Integrity: the file must match the hash the user said to expect.
                    let integrity_error = match (&final_task.expected_checksum, &hashes) {
                        (Some(expected), Some((sha256, md5))) => check_expected(expected, sha256, md5).err(),
                        (Some(_), None) => Some("Could not read the file to verify its checksum".to_string()),
                        _ => None,
                    };
                    if let Some(message) = integrity_error {
                        // A file that fails verification is corrupt or tampered with: don't leave it lying around.
                        if hashes.is_some() {
                            let _ = std::fs::remove_file(&final_path);
                        }
                        let message = format!("Integrity check failed — {message}. The file was removed; retry to download it again.");
                        if let Ok(conn) = Repository::open_or_create(&db_path) {
                            let now = chrono::Utc::now().timestamp();
                            let _ = Repository::update_status(&conn, &task_id, DownloadStatus::Failed, Some(&message), None, now);
                            let _ = Repository::update_checksum(&conn, &task_id, final_task.checksum_sha256.as_deref(), final_task.checksum_md5.as_deref());
                        }
                        let _ = app_handle.emit("download-failed", serde_json::json!({ "id": task_id, "error": message }));
                        manager.notify_outcome(&settings, "Download failed", &final_task.filename, false);
                    } else {
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
                        manager.completed_in_batch.fetch_add(1, Ordering::SeqCst);
                        *manager.queue_batches.lock().entry(queue_id.clone()).or_insert(0) += 1;
                        manager.retries_used.lock().remove(&task_id);
                        let threat = final_task.antivirus_status.as_deref() == Some("threat_detected");
                        let (title, success) = if threat {
                            ("Threat detected in download", false)
                        } else {
                            ("Download complete", true)
                        };
                        manager.notify_outcome(&settings, title, &final_task.filename, success);
                    }
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
                    let settings = Repository::open_or_create(&db_path)
                        .ok()
                        .and_then(|c| Repository::load_settings(&c).ok())
                        .unwrap_or_default();
                    let name = Repository::open_or_create(&db_path)
                        .ok()
                        .and_then(|c| Repository::get_download(&c, &task_id).ok().flatten())
                        .map(|t| t.filename)
                        .unwrap_or_default();
                    manager.notify_outcome(&settings, "Download failed", &name, false);
                    manager.schedule_auto_retry(&task_id, &queue_id);
                }
            }

            // The run is over: free its slot, then let the next queued download start.
            manager.active_controllers.lock().remove(&task_id);
            let _ = app_handle.emit("download-status-changed", serde_json::json!({ "id": task_id }));
            manager.process_queue().await;
            manager.run_completion_action_if_idle();
            manager.queue_finished_check(&queue_id);
        });

        true
    }

    /// Desktop notification and/or sound for a finished download, per the user's settings.
    fn notify_outcome(&self, settings: &AppSettings, title: &str, name: &str, play_sound: bool) {
        let sound = settings.sound_enabled && play_sound;
        if settings.notify_on_complete && !crate::notify::window_has_focus(&self.app_handle) {
            crate::notify::toast(&self.app_handle, title, name, sound);
        } else if sound {
            crate::notify::beep();
        }
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
            self.retries_used.lock().remove(id);
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

    /// Removes a torrent from the engine and, if asked, deletes the files it downloaded.
    async fn remove_torrent(&self, task: &DownloadTask, delete_files: bool) {
        if let Some(hash) = &task.info_hash {
            self.torrents.remove(hash, delete_files).await;
        }
        if delete_files {
            // The engine only knows torrents it has loaded this session; make sure the files go either way.
            let folder = crate::engine::torrent::output_folder(task);
            for file in task.torrent_files.iter().filter(|f| f.selected) {
                let _ = std::fs::remove_file(folder.join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR)));
            }
            if task.torrent_files.len() > 1 {
                // Remove emptied sub-folders, deepest first, and never anything that still has files.
                let mut dirs: Vec<PathBuf> = task
                    .torrent_files
                    .iter()
                    .filter_map(|f| Path::new(&f.path.replace('/', std::path::MAIN_SEPARATOR_STR)).parent().map(|p| folder.join(p)))
                    .collect();
                dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
                dirs.dedup();
                for d in dirs {
                    let _ = std::fs::remove_dir(d);
                }
                let _ = std::fs::remove_dir(&folder);
            }
        }
    }

    pub async fn cancel_download(&self, id: &str, delete_file: bool) -> Result<(), String> {
        let _ = self.pause_download(id);
        self.wait_until_idle(id).await;

        let conn = self.get_db_conn().map_err(|e| e.to_string())?;
        if let Ok(Some(task)) = Repository::get_download(&conn, id) {
            if task.kind == "torrent" {
                self.remove_torrent(&task, delete_file).await;
            } else if delete_file {
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
            if task.kind == "torrent" {
                self.remove_torrent(&task, delete_file).await;
            } else if delete_file {
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
    stream::remove_partials(dir, &task.filename);
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
