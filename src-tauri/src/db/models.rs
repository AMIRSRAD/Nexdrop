use std::collections::HashMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Scheduled,
    Canceled,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadStatus::Queued => "queued",
            DownloadStatus::Downloading => "downloading",
            DownloadStatus::Paused => "paused",
            DownloadStatus::Completed => "completed",
            DownloadStatus::Failed => "failed",
            DownloadStatus::Scheduled => "scheduled",
            DownloadStatus::Canceled => "canceled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "downloading" => DownloadStatus::Downloading,
            "paused" => DownloadStatus::Paused,
            "completed" => DownloadStatus::Completed,
            "failed" => DownloadStatus::Failed,
            "scheduled" => DownloadStatus::Scheduled,
            "canceled" => DownloadStatus::Canceled,
            _ => DownloadStatus::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileCategory {
    All,
    Compressed,
    Documents,
    Music,
    Videos,
    Programs,
    Other,
}

impl FileCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileCategory::All => "all",
            FileCategory::Compressed => "compressed",
            FileCategory::Documents => "documents",
            FileCategory::Music => "music",
            FileCategory::Videos => "videos",
            FileCategory::Programs => "programs",
            FileCategory::Other => "other",
        }
    }

    pub fn subfolder_name(&self) -> &'static str {
        match self {
            FileCategory::All => "",
            FileCategory::Compressed => "Compressed",
            FileCategory::Documents => "Documents",
            FileCategory::Music => "Music",
            FileCategory::Videos => "Videos",
            FileCategory::Programs => "Programs",
            FileCategory::Other => "Other",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "compressed" => FileCategory::Compressed,
            "documents" => FileCategory::Documents,
            "music" => FileCategory::Music,
            "videos" => FileCategory::Videos,
            "programs" => FileCategory::Programs,
            _ => FileCategory::Other,
        }
    }

    pub fn from_filename_or_mime(filename: &str, mime: Option<&str>) -> Self {
        if let Some(m) = mime {
            let m = m.to_lowercase();
            if m.starts_with("video/") {
                return FileCategory::Videos;
            }
            if m.starts_with("audio/") {
                return FileCategory::Music;
            }
            if m.contains("pdf") || m.contains("word") || m.contains("document") || m.contains("text/") {
                return FileCategory::Documents;
            }
            if m.contains("zip") || m.contains("tar") || m.contains("gzip") || m.contains("rar") || m.contains("7z") {
                return FileCategory::Compressed;
            }
        }

        let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
        match ext.as_str() {
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "iso" => FileCategory::Compressed,
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "flv" | "wmv" | "m4v" => FileCategory::Videos,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" => FileCategory::Music,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv" => FileCategory::Documents,
            "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" => FileCategory::Programs,
            _ => FileCategory::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSegment {
    pub segment_index: usize,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub filename: String,
    pub destination_path: String,
    pub temp_path: String,
    pub total_bytes: i64,
    pub downloaded_bytes: i64,
    pub status: DownloadStatus,
    pub resumable: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub mime_type: Option<String>,
    pub category: FileCategory,
    pub connections: usize,
    pub speed_limit: u64,
    pub priority: i32,
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
    pub segments: Vec<DownloadSegment>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub error_message: Option<String>,
    pub scheduled_time: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub checksum_md5: Option<String>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    #[serde(default)]
    pub mirrors: Vec<String>,
    #[serde(default = "default_min_chunk")]
    pub min_chunk_size: u64,
    pub proxy_mode: Option<String>,
    pub custom_proxy_url: Option<String>,
    pub antivirus_status: Option<String>,
    /// Hash the user expects the finished file to have (MD5 or SHA-256, hex).
    /// Verified automatically when the download completes.
    #[serde(default)]
    pub expected_checksum: Option<String>,
    /// Extra request headers (e.g. `Cookie`, `Authorization` from an imported
    /// cURL command). Stored encrypted; sent only to the original host.
    #[serde(default)]
    pub custom_headers: HashMap<String, String>,
    /// "http" (a normal download) or "torrent".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// BitTorrent info hash (hex), for torrents.
    #[serde(default)]
    pub info_hash: Option<String>,
    /// Bytes uploaded to other peers so far (torrents).
    #[serde(default)]
    pub uploaded_bytes: i64,
    /// Connected peers right now (torrents, live only; not stored).
    #[serde(default)]
    pub peers: u32,
    /// A finished torrent that is still being shared with other peers.
    #[serde(default)]
    pub seeding: bool,
    /// The files inside a torrent and which of them are being downloaded.
    #[serde(default)]
    pub torrent_files: Vec<TorrentFile>,
    /// The queue this download belongs to (`"main"` unless the user made more).
    #[serde(default = "default_queue_id")]
    pub queue_id: String,
}

pub fn default_kind() -> String {
    "http".to_string()
}

pub const MAIN_QUEUE_ID: &str = "main";

pub fn default_queue_id() -> String {
    MAIN_QUEUE_ID.to_string()
}

/// A named list of downloads with its own schedule, limits and "when finished" action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadQueue {
    pub id: String,
    pub name: String,
    /// Off: the queue runs whenever it has work. On: only inside its schedule (or after "Start now").
    pub schedule_enabled: bool,
    /// Days the schedule applies to: "mon" .. "sun".
    pub days: Vec<String>,
    /// "HH:MM"
    pub start_time: String,
    /// "HH:MM". `None`: after starting, run until the queue is empty.
    pub stop_time: Option<String>,
    /// Simultaneous downloads in this queue (0 = the global limit).
    pub max_active: usize,
    /// Bandwidth shared by the whole queue, bytes/s (0 = unlimited).
    pub speed_limit: u64,
    /// When the queue empties: "none", "notify", "exit", "sleep", "hibernate", "shutdown", "lock" or "program".
    pub after_done: String,
    pub after_done_program: Option<String>,
    /// How many times a failed download is put back in the queue automatically.
    pub retry_count: u32,
    /// Seconds to wait before each automatic retry.
    pub retry_delay_secs: u64,
    pub position: i64,
}

impl Default for DownloadQueue {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Queue".to_string(),
            schedule_enabled: false,
            days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"].iter().map(|d| d.to_string()).collect(),
            start_time: "02:00".to_string(),
            stop_time: None,
            max_active: 0,
            speed_limit: 0,
            after_done: "none".to_string(),
            after_done_program: None,
            retry_count: 0,
            retry_delay_secs: 60,
            position: 0,
        }
    }
}

/// A queue plus what it's doing right now (for the interface).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueInfo {
    #[serde(flatten)]
    pub queue: DownloadQueue,
    /// Allowed to start downloads right now.
    pub open: bool,
    pub active: usize,
    pub queued: usize,
    /// Unix seconds when the schedule next opens (only while closed).
    pub next_start: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentFile {
    pub index: usize,
    /// Path inside the torrent, `/`-separated.
    pub path: String,
    pub length: u64,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteCredential {
    pub id: String,
    pub domain: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    /// Bearer token or cookie. (`token` is accepted for older callers.)
    #[serde(default, alias = "token")]
    pub token_or_cookie: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_min_chunk() -> u64 {
    1024 * 1024 // 1 MB default
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub default_destination: String,
    pub max_active_downloads: usize,
    pub default_connections: usize,
    pub max_connections_per_server: usize,
    pub min_chunk_size: u64,
    pub global_speed_limit: u64,
    pub auto_resume_on_launch: bool,
    pub notify_on_complete: bool,
    pub theme: String,
    pub clipboard_watch: bool,
    pub auto_categorize: bool,
    pub proxy_mode: String,
    pub custom_proxy_url: Option<String>,
    pub on_complete_action: String,
    pub sound_enabled: bool,
    // IDM-grade Scheduler Options
    pub scheduler_enabled: bool,
    pub scheduler_start_time: Option<String>, // e.g. "02:00"
    pub scheduler_stop_time: Option<String>,  // e.g. "07:00"
    pub scheduler_days: Vec<String>,          // ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
    pub scheduler_speed_limit: u64,           // Speed limit during scheduled hours
    pub scheduler_action: String,             // "none", "sleep", "shutdown", "exit"
    pub scan_with_antivirus: bool,
    pub antivirus_path: Option<String>,
    // Torrents
    /// Keep sharing a torrent after it has finished downloading.
    pub torrent_seed_after_download: bool,
    /// Stop sharing once uploaded/downloaded reaches this ratio (0 = never stop on ratio).
    pub torrent_ratio_limit: f64,
    /// Upload cap for torrents in bytes/s (0 = unlimited).
    pub torrent_upload_limit: u64,
    /// Find peers through the distributed hash table (works without trackers).
    pub torrent_dht: bool,
    /// Ask the router to forward the torrent port (UPnP). Off by default.
    pub torrent_upnp: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        let default_destination = dirs::download_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "Downloads".to_string());

        Self {
            default_destination,
            max_active_downloads: 3,
            default_connections: 8,
            max_connections_per_server: 8,
            min_chunk_size: 1024 * 1024, // 1 MB
            global_speed_limit: 0,
            auto_resume_on_launch: true,
            notify_on_complete: true,
            theme: "dark".to_string(),
            clipboard_watch: false,
            auto_categorize: false,
            proxy_mode: "system".to_string(),
            custom_proxy_url: None,
            on_complete_action: "notify".to_string(),
            sound_enabled: true,
            scheduler_enabled: false,
            scheduler_start_time: Some("02:00".to_string()),
            scheduler_stop_time: Some("07:00".to_string()),
            scheduler_days: vec![
                "mon".to_string(),
                "tue".to_string(),
                "wed".to_string(),
                "thu".to_string(),
                "fri".to_string(),
                "sat".to_string(),
                "sun".to_string(),
            ],
            scheduler_speed_limit: 0,
            scheduler_action: "none".to_string(),
            scan_with_antivirus: true,
            antivirus_path: None,
            torrent_seed_after_download: true,
            torrent_ratio_limit: 1.0,
            torrent_upload_limit: 0,
            torrent_dht: true,
            torrent_upnp: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlProbeResult {
    pub url: String,
    pub filename: String,
    pub total_bytes: i64,
    pub resumable: bool,
    pub mime_type: Option<String>,
    pub category: FileCategory,
    pub default_destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDownloadRequest {
    pub url: String,
    pub filename: Option<String>,
    pub destination_path: Option<String>,
    pub connections: Option<usize>,
    pub speed_limit: Option<u64>,
    pub priority: Option<i32>,
    pub scheduled_time: Option<i64>,
    pub user_agent: Option<String>,
    pub referer: Option<String>,
    pub custom_headers: Option<HashMap<String, String>>,
    pub expected_checksum: Option<String>,
    pub mirrors: Option<Vec<String>>,
    pub min_chunk_size: Option<u64>,
    pub proxy_mode: Option<String>,
    pub custom_proxy_url: Option<String>,
    pub queue_id: Option<String>,
}

/// Adding a torrent: either a magnet/`.torrent` link or an `info_hash` from an earlier probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddTorrentRequest {
    /// Magnet link or http(s) link to a `.torrent` file.
    pub source: Option<String>,
    /// Hash returned by `probe_torrent_cmd` (its metadata is cached for a while).
    pub info_hash: Option<String>,
    pub destination_path: Option<String>,
    /// Indices of the files to download; `None` means all of them.
    pub selected_files: Option<Vec<usize>>,
    pub queue_id: Option<String>,
}

/// What the "Add torrent" dialog shows before anything is downloaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentProbeInfo {
    pub info_hash: String,
    pub name: String,
    pub total_bytes: u64,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgressEvent {
    pub id: String,
    pub downloaded_bytes: i64,
    pub total_bytes: i64,
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
    pub status: DownloadStatus,
    pub segments: Vec<DownloadSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumResult {
    pub algorithm: String,
    pub calculated_hash: String,
    pub expected_hash: Option<String>,
    pub matches: bool,
}
