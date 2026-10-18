//! BitTorrent support, built on `librqbit`.
//!
//! A torrent is an ordinary [`DownloadTask`] with `kind == "torrent"`, so it
//! shares the queue, pause/resume, notifications and the list in the UI with
//! HTTP downloads. This module owns the BitTorrent session and translates its
//! state into the same progress events the HTTP engine emits.
//!
//! Design decisions:
//! * Nexdrop's database is the source of truth; librqbit's own persistence is
//!   off. The `.torrent` file is stored with the task so a torrent resumes
//!   without having to fetch its metadata from peers again.
//! * Torrent metadata is untrusted input. File paths are validated *before*
//!   anything is written (see [`validate_torrent_files`]).
//! * Existing files are only overwritten when they belong to a download we
//!   started ourselves (`resume`), never for a freshly added torrent.
//! * Finished torrents keep sharing ("seeding") until a ratio goal is met, the
//!   user stops them, or seeding is switched off in Settings.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use librqbit::limits::LimitsConfig;
use librqbit::api::TorrentIdOrHash;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ListenerMode, ListenerOptions, ManagedTorrent,
    Session, SessionOptions, TorrentStatsState,
};
use parking_lot::Mutex;
use tokio::sync::watch;

use crate::db::models::{DownloadProgressEvent, DownloadSegment, DownloadStatus, DownloadTask, TorrentFile};
use crate::engine::downloader::wait_paused;
use crate::engine::probe::sanitize_filename;
use crate::engine::security;

/// How long to wait for a magnet link's metadata before giving up.
const METADATA_TIMEOUT: Duration = Duration::from_secs(90);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const PROBE_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Settings the torrent engine needs (a slice of `AppSettings`).
#[derive(Clone, Debug)]
pub struct TorrentSettings {
    /// Bytes/s, 0 = unlimited.
    pub upload_limit: u64,
    /// Bytes/s, 0 = unlimited (the app's effective global limit).
    pub download_limit: u64,
    pub dht: bool,
    pub upnp: bool,
    pub seed_after_download: bool,
    /// Default output folder for the session (individual torrents override it).
    pub default_dir: PathBuf,
}

impl Default for TorrentSettings {
    fn default() -> Self {
        Self {
            upload_limit: 0,
            download_limit: 0,
            dht: true,
            upnp: false,
            seed_after_download: true,
            default_dir: std::env::temp_dir(),
        }
    }
}

/// Where a torrent comes from.
#[derive(Clone, Debug)]
pub enum TorrentSource {
    /// A magnet link, or an `http(s)` link to a `.torrent` file.
    Url(String),
    /// The contents of a `.torrent` file.
    Bytes(Vec<u8>),
}

/// What a torrent contains, resolved *before* anything is downloaded.
#[derive(Clone, Debug)]
pub struct TorrentProbe {
    pub info_hash: String,
    pub name: String,
    pub total_bytes: u64,
    pub files: Vec<TorrentFile>,
    pub torrent_bytes: Vec<u8>,
}

/// Live numbers for a torrent, sent to the UI next to the usual progress event.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TorrentLive {
    pub id: String,
    pub uploaded_bytes: u64,
    pub upload_bps: u64,
    pub peers: u32,
    pub seeding: bool,
    pub ratio: f64,
}

pub struct TorrentEngine {
    session: Mutex<Option<Arc<Session>>>,
    init: tokio::sync::Mutex<()>,
    handles: Mutex<HashMap<String, Arc<ManagedTorrent>>>,
    probes: Mutex<HashMap<String, (Instant, Vec<u8>)>>,
    /// Peers to connect to immediately (used by tests to join a local swarm).
    initial_peers: Vec<SocketAddr>,
}

impl Default for TorrentEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TorrentEngine {
    pub fn new() -> Self {
        Self {
            session: Mutex::new(None),
            init: tokio::sync::Mutex::new(()),
            handles: Mutex::new(HashMap::new()),
            probes: Mutex::new(HashMap::new()),
            initial_peers: Vec::new(),
        }
    }

    #[doc(hidden)]
    pub fn with_initial_peers(mut self, peers: Vec<SocketAddr>) -> Self {
        self.initial_peers = peers;
        self
    }

    // ───────────────────────────── session ─────────────────────────────

    async fn session(&self, s: &TorrentSettings) -> Result<Arc<Session>, String> {
        if let Some(session) = self.session.lock().clone() {
            return Ok(session);
        }
        // Only one task builds the session; the rest wait and reuse it.
        let _guard = self.init.lock().await;
        if let Some(session) = self.session.lock().clone() {
            return Ok(session);
        }

        let opts = SessionOptions {
            dht: s.dht.then(DhtSessionConfig::default),
            // Accept incoming peers so finished torrents can be shared. A random port is used.
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpOnly,
                enable_upnp_port_forwarding: s.upnp,
                ..Default::default()
            }),
            ratelimits: limits(s.upload_limit, s.download_limit),
            ..Default::default()
        };
        let session = Session::new_with_opts(s.default_dir.clone(), opts)
            .await
            .map_err(|e| format!("Couldn't start the torrent engine: {e:#}"))?;
        *self.session.lock() = Some(session.clone());
        Ok(session)
    }

    /// Applies new speed limits to a running session (bytes/s, 0 = unlimited).
    pub fn apply_limits(&self, upload_bps: u64, download_bps: u64) {
        if let Some(session) = self.session.lock().as_ref() {
            session.ratelimits.set_upload_bps(NonZeroU32::new(clamp_u32(upload_bps)));
            session.ratelimits.set_download_bps(NonZeroU32::new(clamp_u32(download_bps)));
        }
    }

    /// Stops the BitTorrent session (on exit). Torrents are resumed from the database next start.
    pub async fn shutdown(&self) {
        let session = self.session.lock().take();
        if let Some(session) = session {
            session.stop().await;
        }
        self.handles.lock().clear();
    }

    pub fn has_session(&self) -> bool {
        self.session.lock().is_some()
    }

    // ───────────────────────────── probing ─────────────────────────────

    /// Resolves a torrent's name and file list without downloading anything.
    /// For a magnet link this has to fetch the metadata from peers first.
    pub async fn probe(&self, source: TorrentSource, s: &TorrentSettings) -> Result<TorrentProbe, String> {
        let session = self.session(s).await?;
        let add = match &source {
            TorrentSource::Url(url) => AddTorrent::from_url(url.clone()),
            TorrentSource::Bytes(bytes) => AddTorrent::from_bytes(bytes.clone()),
        };
        let opts = AddTorrentOptions {
            list_only: true,
            initial_peers: (!self.initial_peers.is_empty()).then(|| self.initial_peers.clone()),
            ..Default::default()
        };

        let resp = tokio::time::timeout(METADATA_TIMEOUT, session.add_torrent(add, Some(opts)))
            .await
            .map_err(|_| "Couldn't get the torrent's details: no peers answered in time. Check the link, or try again later.".to_string())?
            .map_err(|e| format!("Couldn't read that torrent: {e:#}"))?;

        let probe = match resp {
            AddTorrentResponse::ListOnly(listed) => {
                let name = listed.info.name().map(|n| n.to_string()).unwrap_or_else(|| "torrent".to_string());
                let files = files_from_info(&listed.info)?;
                (listed.info_hash.as_string(), name, files, listed.torrent_bytes.to_vec())
            }
            AddTorrentResponse::AlreadyManaged(_, handle) | AddTorrentResponse::Added(_, handle) => {
                // The session already has it (for example it is being shared): describe it from there.
                let meta = handle
                    .with_metadata(|m| (m.info.name().map(|n| n.to_string()), files_from_info(&m.info), m.torrent_bytes.to_vec()))
                    .map_err(|e| format!("{e:#}"))?;
                (handle.info_hash().as_string(), meta.0.unwrap_or_else(|| "torrent".to_string()), meta.1?, meta.2)
            }
        };
        let (info_hash, name, files, torrent_bytes) = probe;

        validate_torrent_files(&files)?;
        let total_bytes = files.iter().map(|f| f.length).sum();
        self.remember_probe(&info_hash, torrent_bytes.clone());
        Ok(TorrentProbe { info_hash, name, total_bytes, files, torrent_bytes })
    }

    fn remember_probe(&self, info_hash: &str, bytes: Vec<u8>) {
        let mut cache = self.probes.lock();
        cache.retain(|_, (at, _)| at.elapsed() < PROBE_CACHE_TTL);
        cache.insert(info_hash.to_string(), (Instant::now(), bytes));
    }

    /// The `.torrent` contents from a recent probe, so adding doesn't fetch metadata twice.
    pub fn cached_torrent_bytes(&self, info_hash: &str) -> Option<Vec<u8>> {
        self.probes.lock().get(info_hash).map(|(_, b)| b.clone())
    }

    // ───────────────────────────── running ─────────────────────────────

    /// Runs a torrent task until it finishes, is paused, or fails.
    ///
    /// Same contract as [`DownloadEngine::run_download`](crate::engine::downloader::DownloadEngine):
    /// `Ok(Completed)`, `Ok(Paused)` or `Err`. A completed torrent may keep
    /// sharing in the background (`task.seeding`).
    pub async fn run<F, L>(
        &self,
        mut task: DownloadTask,
        torrent_bytes: Option<Vec<u8>>,
        resume: bool,
        mut pause_rx: watch::Receiver<bool>,
        settings: &TorrentSettings,
        on_progress: F,
        on_live: L,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
        L: Fn(TorrentLive) + Send + Sync + 'static,
    {
        let session = self.session(settings).await?;
        let info_hash = task.info_hash.clone().unwrap_or_default();
        let output = output_folder(&task);
        std::fs::create_dir_all(&output).map_err(|e| format!("Cannot create the download folder: {e}"))?;

        let total_wanted: u64 = task.torrent_files.iter().filter(|f| f.selected).map(|f| f.length).sum();
        check_disk_space(&output, total_wanted)?;

        // Reuse the handle if the torrent is already in the session (e.g. it was paused).
        let existing = self.handles.lock().get(&info_hash).cloned();
        let handle = match existing {
            Some(handle) => handle,
            None => {
                let add = match torrent_bytes {
                    Some(bytes) => AddTorrent::from_bytes(bytes),
                    None => AddTorrent::from_url(task.url.clone()),
                };
                let selected: Vec<usize> = task.torrent_files.iter().filter(|f| f.selected).map(|f| f.index).collect();
                let all_selected = selected.len() == task.torrent_files.len();
                let opts = AddTorrentOptions {
                    only_files: (!all_selected && !selected.is_empty()).then_some(selected),
                    output_folder: Some(output.to_string_lossy().to_string()),
                    // Only files we created ourselves may be reused; a fresh add never overwrites anything.
                    overwrite: resume,
                    initial_peers: (!self.initial_peers.is_empty()).then(|| self.initial_peers.clone()),
                    ..Default::default()
                };
                let resp = tokio::select! {
                    biased;
                    _ = wait_paused(&mut pause_rx) => {
                        task.status = DownloadStatus::Paused;
                        task.speed_bps = 0;
                        return Ok(task);
                    }
                    r = tokio::time::timeout(METADATA_TIMEOUT, session.add_torrent(add, Some(opts))) => r,
                };
                let resp = resp
                    .map_err(|_| "Timed out waiting for the torrent's details".to_string())?
                    .map_err(|e| friendly_add_error(&format!("{e:#}")))?;
                let handle = resp.into_handle().ok_or_else(|| "The torrent engine didn't start the torrent".to_string())?;
                self.handles.lock().insert(info_hash.clone(), handle.clone());
                handle
            }
        };
        if handle.is_paused() {
            session.unpause(&handle).await.map_err(|e| format!("Couldn't resume the torrent: {e:#}"))?;
        }

        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                biased;
                _ = wait_paused(&mut pause_rx) => {
                    let _ = session.pause(&handle).await;
                    let snap = self.snapshot(&handle, &task);
                    task.downloaded_bytes = snap.downloaded as i64;
                    task.uploaded_bytes = snap.uploaded as i64;
                    task.status = DownloadStatus::Paused;
                    task.speed_bps = 0;
                    task.eta_seconds = None;
                    return Ok(task);
                }
                _ = ticker.tick() => {}
            }

            let snap = self.snapshot(&handle, &task);
            if let Some(error) = &snap.error {
                return Err(error.clone());
            }

            let total = if snap.total > 0 { snap.total } else { total_wanted };
            let remaining = total.saturating_sub(snap.downloaded);
            let eta = (snap.down_bps > 0 && remaining > 0).then(|| remaining / snap.down_bps);
            task.downloaded_bytes = snap.downloaded as i64;
            task.total_bytes = total as i64;
            task.uploaded_bytes = snap.uploaded as i64;
            task.peers = snap.peers;

            on_progress(DownloadProgressEvent {
                id: task.id.clone(),
                downloaded_bytes: snap.downloaded as i64,
                total_bytes: total as i64,
                speed_bps: snap.down_bps,
                eta_seconds: eta,
                status: DownloadStatus::Downloading,
                segments: file_segments(&task, &snap.file_progress),
            });
            on_live(TorrentLive {
                id: task.id.clone(),
                uploaded_bytes: snap.uploaded,
                upload_bps: snap.up_bps,
                peers: snap.peers,
                seeding: false,
                ratio: ratio(snap.uploaded, total),
            });

            if snap.finished {
                break;
            }
        }

        // Downloaded. Mark every file as coming from the Internet (SmartScreen etc.).
        let files: Vec<PathBuf> = task
            .torrent_files
            .iter()
            .filter(|f| f.selected)
            .map(|f| output.join(f.path.replace('/', std::path::MAIN_SEPARATOR_STR)))
            .collect();
        let _ = tokio::task::spawn_blocking(move || {
            for path in files {
                security::mark_of_the_web(&path, None, None);
            }
        })
        .await;

        let snap = self.snapshot(&handle, &task);
        let seeding = settings.seed_after_download;
        if !seeding {
            let _ = session.pause(&handle).await;
        }
        task.status = DownloadStatus::Completed;
        task.completed_at = Some(chrono::Utc::now().timestamp());
        task.downloaded_bytes = total_wanted as i64;
        task.total_bytes = total_wanted as i64;
        task.uploaded_bytes = snap.uploaded as i64;
        task.speed_bps = 0;
        task.eta_seconds = None;
        task.seeding = seeding;
        task.segments = file_segments(&task, &snap.file_progress);

        on_progress(DownloadProgressEvent {
            id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes,
            total_bytes: task.total_bytes,
            speed_bps: 0,
            eta_seconds: None,
            status: DownloadStatus::Completed,
            segments: task.segments.clone(),
        });
        Ok(task)
    }

    fn snapshot(&self, handle: &ManagedTorrent, task: &DownloadTask) -> Snapshot {
        let stats = handle.stats();
        let (down_bps, up_bps, peers) = match &stats.live {
            Some(live) => (live.download_speed.as_bytes(), live.upload_speed.as_bytes(), live.snapshot.peer_stats.live),
            None => (0, 0, 0),
        };
        let _ = task;
        Snapshot {
            downloaded: stats.progress_bytes,
            uploaded: stats.uploaded_bytes,
            total: stats.total_bytes,
            finished: stats.finished,
            down_bps,
            up_bps,
            peers,
            file_progress: stats.file_progress.clone(),
            error: matches!(stats.state, TorrentStatsState::Error).then(|| stats.error.clone().unwrap_or_else(|| "The torrent failed".to_string())),
        }
    }

    // ───────────────────────────── seeding & removal ─────────────────────────────

    /// Brings a finished torrent back online so it can be shared (after a restart,
    /// or when the user turns sharing back on). The files are ours, so verifying
    /// and reusing them is allowed.
    pub async fn start_seeding(&self, task: &DownloadTask, torrent_bytes: Option<Vec<u8>>, s: &TorrentSettings) -> Result<(), String> {
        let info_hash = task.info_hash.clone().unwrap_or_default();
        let session = self.session(s).await?;
        let existing = self.handles.lock().get(&info_hash).cloned();
        let handle = match existing {
            Some(h) => h,
            None => {
                let add = match torrent_bytes {
                    Some(bytes) => AddTorrent::from_bytes(bytes),
                    None => AddTorrent::from_url(task.url.clone()),
                };
                let selected: Vec<usize> = task.torrent_files.iter().filter(|f| f.selected).map(|f| f.index).collect();
                let opts = AddTorrentOptions {
                    only_files: (selected.len() != task.torrent_files.len() && !selected.is_empty()).then_some(selected),
                    output_folder: Some(output_folder(task).to_string_lossy().to_string()),
                    overwrite: true,
                    initial_peers: (!self.initial_peers.is_empty()).then(|| self.initial_peers.clone()),
                    ..Default::default()
                };
                let resp = session
                    .add_torrent(add, Some(opts))
                    .await
                    .map_err(|e| friendly_add_error(&format!("{e:#}")))?;
                let h = resp.into_handle().ok_or_else(|| "The torrent engine didn't start the torrent".to_string())?;
                self.handles.lock().insert(info_hash, h.clone());
                h
            }
        };
        if handle.is_paused() {
            session.unpause(&handle).await.map_err(|e| format!("{e:#}"))?;
        }
        Ok(())
    }

    /// Stops uploading/downloading a torrent but keeps it (and its files) around.
    pub async fn stop(&self, info_hash: &str) {
        let handle = self.handles.lock().get(info_hash).cloned();
        let session = self.session.lock().clone();
        if let (Some(handle), Some(session)) = (handle, session) {
            let _ = session.pause(&handle).await;
        }
    }

    /// Removes a torrent from the engine, optionally deleting its files.
    pub async fn remove(&self, info_hash: &str, delete_files: bool) {
        let handle = self.handles.lock().remove(info_hash);
        let session = self.session.lock().clone();
        if let (Some(handle), Some(session)) = (handle, session) {
            let _ = session.delete(TorrentIdOrHash::Id(handle.id()), delete_files).await;
        }
    }

    /// Current upload statistics of a torrent that is being shared.
    pub fn live(&self, task: &DownloadTask) -> Option<TorrentLive> {
        let hash = task.info_hash.as_ref()?;
        let handle = self.handles.lock().get(hash).cloned()?;
        let snap = self.snapshot(&handle, task);
        let total: u64 = task.torrent_files.iter().filter(|f| f.selected).map(|f| f.length).sum();
        Some(TorrentLive {
            id: task.id.clone(),
            uploaded_bytes: snap.uploaded,
            upload_bps: snap.up_bps,
            peers: snap.peers,
            seeding: !handle.is_paused(),
            ratio: ratio(snap.uploaded, total),
        })
    }

    pub fn is_running(&self, info_hash: &str) -> bool {
        self.handles.lock().get(info_hash).map(|h| !h.is_paused()).unwrap_or(false)
    }
}

struct Snapshot {
    downloaded: u64,
    uploaded: u64,
    total: u64,
    finished: bool,
    down_bps: u64,
    up_bps: u64,
    peers: u32,
    file_progress: Vec<u64>,
    error: Option<String>,
}

// ───────────────────────────── helpers (pure) ─────────────────────────────

fn limits(upload: u64, download: u64) -> LimitsConfig {
    LimitsConfig {
        upload_bps: NonZeroU32::new(clamp_u32(upload)),
        download_bps: NonZeroU32::new(clamp_u32(download)),
    }
}

fn clamp_u32(v: u64) -> u32 {
    v.min(u32::MAX as u64) as u32
}

pub fn ratio(uploaded: u64, downloaded_total: u64) -> f64 {
    if downloaded_total == 0 {
        0.0
    } else {
        uploaded as f64 / downloaded_total as f64
    }
}

/// Where a torrent's files go: single-file torrents straight into the chosen
/// folder, multi-file torrents into a folder named after the torrent.
pub fn output_folder(task: &DownloadTask) -> PathBuf {
    let dest = Path::new(&task.destination_path);
    if task.torrent_files.len() <= 1 {
        dest.to_path_buf()
    } else {
        dest.join(&task.filename)
    }
}

/// A segment per selected file, laid out end to end, so the UI's byte map shows each file's progress.
fn file_segments(task: &DownloadTask, file_progress: &[u64]) -> Vec<DownloadSegment> {
    let mut offset = 0u64;
    let mut out = Vec::new();
    for file in task.torrent_files.iter().filter(|f| f.selected) {
        let done = file_progress.get(file.index).copied().unwrap_or(0).min(file.length);
        out.push(DownloadSegment {
            segment_index: out.len(),
            start_byte: offset,
            end_byte: (offset + file.length).saturating_sub(1),
            downloaded_bytes: done,
            total_bytes: file.length,
            status: if done >= file.length { "completed" } else { "downloading" }.to_string(),
        });
        offset += file.length;
    }
    out
}

fn check_disk_space(dir: &Path, needed: u64) -> Result<(), String> {
    const MARGIN: u64 = 64 * 1024 * 1024;
    match crate::engine::fileio::free_space(dir) {
        Some(free) if free < needed.saturating_add(MARGIN) => Err(format!(
            "Not enough free disk space: {:.1} GB needed, {:.1} GB available",
            needed as f64 / 1e9,
            free as f64 / 1e9
        )),
        _ => Ok(()),
    }
}

fn friendly_add_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("already exist") || lower.contains("exists") && lower.contains("overwrite") {
        "Files with the same names already exist in that folder. Choose another folder or move the existing files.".to_string()
    } else {
        format!("Couldn't start the torrent: {raw}")
    }
}

fn files_from_info<B: AsRef<[u8]>>(info: &librqbit::ValidatedTorrentMetaV1Info<B>) -> Result<Vec<TorrentFile>, String> {
    let mut files = Vec::new();
    for (index, details) in info.iter_file_details().enumerate() {
        let path = details.filename.to_pathbuf();
        let path = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        files.push(TorrentFile { index, path, length: details.len, selected: true });
    }
    Ok(files)
}

/// Rejects torrents whose file names could do harm on Windows. Torrent
/// metadata comes from strangers, so every path component is checked:
///
/// * no `:` (it would create a hidden NTFS stream, or point at another drive),
///   and none of `\ * ? " < > |`, or control characters;
/// * no reserved device names (`CON`, `NUL`, `COM1`…), no trailing dots/spaces;
/// * no empty, `.` or `..` components, and no absolute paths.
pub fn validate_torrent_files(files: &[TorrentFile]) -> Result<(), String> {
    for file in files {
        if file.path.is_empty() || file.path.starts_with('/') || file.path.starts_with('\\') {
            return Err(format!("This torrent contains an unsafe file path: “{}”", file.path));
        }
        for part in file.path.split('/') {
            let bad = part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
                || part.chars().any(|c| c.is_control() || matches!(c, ':' | '\\' | '*' | '?' | '"' | '<' | '>' | '|'))
                || sanitize_filename(part) != part;
            if bad {
                return Err(format!(
                    "This torrent contains a file name that isn't safe to create on Windows (“{}”), so it was not added.",
                    file.path
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str) -> TorrentFile {
        TorrentFile { index: 0, path: path.to_string(), length: 10, selected: true }
    }

    #[test]
    fn ordinary_names_are_accepted() {
        for p in ["movie.mkv", "Season 1/Episode 01 - Pilot (1080p).mkv", "données/été.txt", ".hidden", "a b/c.d.e.tar.gz"] {
            assert!(validate_torrent_files(&[file(p)]).is_ok(), "{p}");
        }
    }

    #[test]
    fn dangerous_names_are_refused() {
        for p in [
            "../evil.dll",
            "a/../../evil.dll",
            "/etc/passwd",
            "C:/Windows/evil.exe",
            "name.txt:hidden",
            "Movie: Title.mkv",
            "CON",
            "dir/NUL.txt",
            "dir/COM1",
            "trailing.",
            "trailing ",
            "a//b",
            "star*.txt",
            "pipe|name",
            "bell\u{7}.txt",
            "rtl\u{202E}fdp.exe",
        ] {
            assert!(validate_torrent_files(&[file(p)]).is_err(), "{p:?} should be refused");
        }
    }

    #[test]
    fn output_folder_follows_the_torrent_layout() {
        let mut t = sample();
        t.torrent_files = vec![file("one.bin")];
        assert_eq!(output_folder(&t), PathBuf::from("D:/dl"));
        t.torrent_files = vec![file("a/1.bin"), file("a/2.bin")];
        assert_eq!(output_folder(&t), PathBuf::from("D:/dl").join("My Torrent"));
    }

    #[test]
    fn byte_map_has_one_segment_per_selected_file() {
        let mut t = sample();
        t.torrent_files = vec![
            TorrentFile { index: 0, path: "a".into(), length: 100, selected: true },
            TorrentFile { index: 1, path: "skip".into(), length: 999, selected: false },
            TorrentFile { index: 2, path: "b".into(), length: 50, selected: true },
        ];
        let segs = file_segments(&t, &[100, 0, 20]);
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].start_byte, segs[0].end_byte, segs[0].status.as_str()), (0, 99, "completed"));
        assert_eq!((segs[1].start_byte, segs[1].end_byte, segs[1].downloaded_bytes, segs[1].status.as_str()), (100, 149, 20, "downloading"));
    }

    #[test]
    fn ratio_handles_empty_torrents() {
        assert_eq!(ratio(5, 0), 0.0);
        assert!((ratio(150, 100) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn friendly_errors_explain_existing_files() {
        assert!(friendly_add_error("file already exists: foo").contains("same names already exist"));
        assert!(friendly_add_error("boom").contains("boom"));
    }

    fn sample() -> DownloadTask {
        DownloadTask {
            id: "t".into(),
            url: "magnet:?xt=urn:btih:abc".into(),
            final_url: None,
            filename: "My Torrent".into(),
            destination_path: "D:/dl".into(),
            temp_path: String::new(),
            total_bytes: 0,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            resumable: true,
            etag: None,
            last_modified: None,
            mime_type: None,
            category: crate::db::models::FileCategory::Other,
            connections: 1,
            speed_limit: 0,
            priority: 0,
            speed_bps: 0,
            eta_seconds: None,
            segments: vec![],
            created_at: 0,
            updated_at: 0,
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
            custom_headers: Default::default(),
            kind: "torrent".into(),
            info_hash: Some("abc".into()),
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: vec![],
        }
    }
}
