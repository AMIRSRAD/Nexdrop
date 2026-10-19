use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use crate::db::models::{AppSettings, DownloadQueue, DownloadSegment, DownloadStatus, DownloadTask, FileCategory, SiteCredential};

pub struct Repository;

/// Column order that [`Repository::map_task_row`] expects.
const TASK_COLUMNS: &str = "id, url, final_url, filename, destination_path, temp_path,
    total_bytes, downloaded_bytes, status, resumable, etag,
    last_modified, mime_type, category, connections, speed_limit,
    priority, speed_bps, eta_seconds, created_at, updated_at,
    completed_at, error_message, scheduled_time, checksum_sha256,
    checksum_md5, user_agent, referer, mirrors, min_chunk_size,
    proxy_mode, custom_proxy_url, antivirus_status,
    expected_checksum, custom_headers,
    kind, info_hash, uploaded_bytes, seeding, torrent_files, queue_id";

/// Header maps hold cookies and tokens, so they are encrypted before they hit the disk.
fn encode_headers(headers: &std::collections::HashMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    serde_json::to_string(headers).ok().map(|json| crate::secrets::protect(&json))
}

fn decode_headers(stored: Option<String>) -> std::collections::HashMap<String, String> {
    stored
        .and_then(|s| crate::secrets::unprotect(&s))
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

impl Repository {
    /// Opens the database. Schema setup runs once per process per file; every
    /// other call only sets the cheap per-connection options.
    pub fn open_or_create<P: AsRef<Path>>(path: P) -> Result<Connection> {
        static INITIALIZED: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        // Several threads write (progress, status changes). Without a busy
        // timeout a write that collides with another fails instantly and is lost.
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;

        let initialized = INITIALIZED.get_or_init(|| Mutex::new(HashSet::new()));
        let already = initialized.lock().map(|set| set.contains(path)).unwrap_or(false);
        if !already {
            conn.execute_batch("PRAGMA journal_mode = WAL;")?;
            Self::create_tables(&conn)?;
            if let Ok(mut set) = initialized.lock() {
                set.insert(path.to_path_buf());
            }
        }
        Ok(conn)
    }

    pub fn create_tables(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS downloads (
                id TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                final_url TEXT,
                filename TEXT NOT NULL,
                destination_path TEXT NOT NULL,
                temp_path TEXT NOT NULL,
                total_bytes INTEGER NOT NULL,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                resumable INTEGER NOT NULL DEFAULT 0,
                etag TEXT,
                last_modified TEXT,
                mime_type TEXT,
                category TEXT NOT NULL,
                connections INTEGER NOT NULL DEFAULT 1,
                speed_limit INTEGER NOT NULL DEFAULT 0,
                priority INTEGER NOT NULL DEFAULT 0,
                speed_bps INTEGER NOT NULL DEFAULT 0,
                eta_seconds INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                completed_at INTEGER,
                error_message TEXT,
                scheduled_time INTEGER,
                checksum_sha256 TEXT,
                checksum_md5 TEXT,
                user_agent TEXT,
                referer TEXT,
                mirrors TEXT,
                min_chunk_size INTEGER NOT NULL DEFAULT 1048576,
                proxy_mode TEXT,
                custom_proxy_url TEXT,
                antivirus_status TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
            CREATE INDEX IF NOT EXISTS idx_downloads_created_at ON downloads(created_at DESC);

            CREATE TABLE IF NOT EXISTS segments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                download_id TEXT NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
                segment_index INTEGER NOT NULL,
                start_byte INTEGER NOT NULL,
                end_byte INTEGER NOT NULL,
                downloaded_bytes INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL,
                UNIQUE(download_id, segment_index)
            );

            CREATE INDEX IF NOT EXISTS idx_segments_download_id ON segments(download_id);

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS site_credentials (
                id TEXT PRIMARY KEY,
                domain TEXT UNIQUE NOT NULL,
                username TEXT NOT NULL,
                password TEXT,
                token TEXT,
                notes TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_site_credentials_domain ON site_credentials(domain);"
        )?;

        // Run non-destructive column additions for existing databases
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN checksum_sha256 TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN checksum_md5 TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN user_agent TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN referer TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN mirrors TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN min_chunk_size INTEGER DEFAULT 1048576", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN proxy_mode TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN custom_proxy_url TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN antivirus_status TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN expected_checksum TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN custom_headers TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN kind TEXT NOT NULL DEFAULT 'http'", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN info_hash TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN uploaded_bytes INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN seeding INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN torrent_files TEXT", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN torrent_bytes BLOB", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN torrent_started INTEGER NOT NULL DEFAULT 0", []);
        let _ = conn.execute("ALTER TABLE downloads ADD COLUMN queue_id TEXT NOT NULL DEFAULT 'main'", []);
        let _ = conn.execute("CREATE INDEX IF NOT EXISTS idx_downloads_info_hash ON downloads(info_hash)", []);
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS queues (
                id TEXT PRIMARY KEY,
                position INTEGER NOT NULL DEFAULT 0,
                data TEXT NOT NULL
            );",
        )?;

        Ok(())
    }

    pub fn save_download(conn: &Connection, task: &DownloadTask) -> Result<()> {
        conn.execute(
            "INSERT INTO downloads (
                id, url, final_url, filename, destination_path, temp_path,
                total_bytes, downloaded_bytes, status, resumable, etag,
                last_modified, mime_type, category, connections, speed_limit,
                priority, speed_bps, eta_seconds, created_at, updated_at,
                completed_at, error_message, scheduled_time, checksum_sha256,
                checksum_md5, user_agent, referer, mirrors, min_chunk_size,
                proxy_mode, custom_proxy_url, antivirus_status,
                expected_checksum, custom_headers,
                kind, info_hash, uploaded_bytes, seeding, torrent_files, queue_id
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21,
                ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                ?29, ?30, ?31, ?32, ?33,
                ?34, ?35,
                ?36, ?37, ?38, ?39, ?40, ?41
            )
            ON CONFLICT(id) DO UPDATE SET
                final_url = excluded.final_url,
                filename = excluded.filename,
                destination_path = excluded.destination_path,
                temp_path = excluded.temp_path,
                total_bytes = excluded.total_bytes,
                downloaded_bytes = excluded.downloaded_bytes,
                status = excluded.status,
                resumable = excluded.resumable,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                mime_type = excluded.mime_type,
                category = excluded.category,
                connections = excluded.connections,
                speed_limit = excluded.speed_limit,
                priority = excluded.priority,
                speed_bps = excluded.speed_bps,
                eta_seconds = excluded.eta_seconds,
                updated_at = excluded.updated_at,
                completed_at = excluded.completed_at,
                error_message = excluded.error_message,
                scheduled_time = excluded.scheduled_time,
                checksum_sha256 = excluded.checksum_sha256,
                checksum_md5 = excluded.checksum_md5,
                user_agent = excluded.user_agent,
                referer = excluded.referer,
                mirrors = excluded.mirrors,
                min_chunk_size = excluded.min_chunk_size,
                proxy_mode = excluded.proxy_mode,
                custom_proxy_url = excluded.custom_proxy_url,
                antivirus_status = excluded.antivirus_status,
                expected_checksum = excluded.expected_checksum,
                custom_headers = excluded.custom_headers,
                kind = excluded.kind,
                info_hash = excluded.info_hash,
                uploaded_bytes = excluded.uploaded_bytes,
                seeding = excluded.seeding,
                torrent_files = excluded.torrent_files;",
            params![
                task.id,
                task.url,
                task.final_url,
                task.filename,
                task.destination_path,
                task.temp_path,
                task.total_bytes,
                task.downloaded_bytes,
                task.status.as_str(),
                if task.resumable { 1 } else { 0 },
                task.etag,
                task.last_modified,
                task.mime_type,
                task.category.as_str(),
                task.connections as i64,
                task.speed_limit as i64,
                task.priority,
                task.speed_bps as i64,
                task.eta_seconds.map(|e| e as i64),
                task.created_at,
                task.updated_at,
                task.completed_at,
                task.error_message,
                task.scheduled_time,
                task.checksum_sha256,
                task.checksum_md5,
                task.user_agent,
                task.referer,
                serde_json::to_string(&task.mirrors).unwrap_or_else(|_| "[]".to_string()),
                task.min_chunk_size as i64,
                task.proxy_mode,
                task.custom_proxy_url,
                task.antivirus_status,
                task.expected_checksum,
                encode_headers(&task.custom_headers),
                task.kind,
                task.info_hash,
                task.uploaded_bytes,
                if task.seeding { 1 } else { 0 },
                if task.torrent_files.is_empty() { None } else { serde_json::to_string(&task.torrent_files).ok() },
                task.queue_id,
            ],
        )?;

        if !task.segments.is_empty() {
            Self::replace_segments(conn, &task.id, &task.segments)?;
        }

        Ok(())
    }

    /// Records live progress. Only touches a task that is still downloading, so a
    /// late write can never undo a pause, cancel or delete the user just made.
    pub fn update_progress(
        conn: &Connection,
        id: &str,
        downloaded_bytes: i64,
        speed_bps: u64,
        eta_seconds: Option<u64>,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "UPDATE downloads
             SET downloaded_bytes = ?1, speed_bps = ?2, eta_seconds = ?3, updated_at = ?4
             WHERE id = ?5 AND status = 'downloading'",
            params![downloaded_bytes, speed_bps as i64, eta_seconds.map(|e| e as i64), now, id],
        )?;
        Ok(())
    }

    /// Saves how many bytes a stopped task has, without touching its status.
    pub fn update_downloaded_bytes(conn: &Connection, id: &str, downloaded_bytes: i64) -> Result<()> {
        conn.execute(
            "UPDATE downloads SET downloaded_bytes = ?1, speed_bps = 0, eta_seconds = NULL WHERE id = ?2",
            params![downloaded_bytes, id],
        )?;
        Ok(())
    }

    /// Counts for the tray: (downloading, queued, paused, bytes/s of the running ones).
    pub fn status_summary(conn: &Connection) -> Result<(u32, u32, u32, u64)> {
        conn.query_row(
            "SELECT
                COALESCE(SUM(status = 'downloading'), 0),
                COALESCE(SUM(status = 'queued'), 0),
                COALESCE(SUM(status = 'paused'), 0),
                COALESCE(SUM(CASE WHEN status = 'downloading' THEN speed_bps END), 0)
             FROM downloads",
            [],
            |r| Ok((r.get::<_, i64>(0)? as u32, r.get::<_, i64>(1)? as u32, r.get::<_, i64>(2)? as u32, r.get::<_, i64>(3)?.max(0) as u64)),
        )
    }

    /// Startup recovery: tasks left as `downloading` by a crash or quit have no
    /// running engine; move them to `status` so they can be resumed.
    pub fn reset_interrupted(conn: &Connection, status: DownloadStatus, now: i64) -> Result<usize> {
        conn.execute(
            "UPDATE downloads SET status = ?1, speed_bps = 0, eta_seconds = NULL, updated_at = ?2
             WHERE status = 'downloading'",
            params![status.as_str(), now],
        )
    }

    pub fn update_status(
        conn: &Connection,
        id: &str,
        status: DownloadStatus,
        error_message: Option<&str>,
        completed_at: Option<i64>,
        now: i64,
    ) -> Result<()> {
        conn.execute(
            "UPDATE downloads 
             SET status = ?1, error_message = ?2, completed_at = ?3, speed_bps = 0, eta_seconds = NULL, updated_at = ?4 
             WHERE id = ?5",
            params![
                status.as_str(),
                error_message,
                completed_at,
                now,
                id
            ],
        )?;
        Ok(())
    }

    pub fn save_segments(conn: &Connection, download_id: &str, segments: &[DownloadSegment]) -> Result<()> {
        for seg in segments {
            conn.execute(
                "INSERT INTO segments (download_id, segment_index, start_byte, end_byte, downloaded_bytes, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(download_id, segment_index) DO UPDATE SET
                    downloaded_bytes = excluded.downloaded_bytes,
                    status = excluded.status;",
                params![
                    download_id,
                    seg.segment_index as i64,
                    seg.start_byte as i64,
                    seg.end_byte as i64,
                    seg.downloaded_bytes as i64,
                    seg.status
                ],
            )?;
        }
        Ok(())
    }

    /// Replaces a download's segments atomically. The set of segments changes
    /// shape while a download runs (ranges are split and merged), so rows must be
    /// swapped as a whole rather than updated one by one.
    pub fn replace_segments(conn: &Connection, download_id: &str, segments: &[DownloadSegment]) -> Result<()> {
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM segments WHERE download_id = ?1", params![download_id])?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO segments (download_id, segment_index, start_byte, end_byte, downloaded_bytes, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (i, seg) in segments.iter().enumerate() {
                stmt.execute(params![
                    download_id,
                    i as i64,
                    seg.start_byte as i64,
                    seg.end_byte as i64,
                    seg.downloaded_bytes as i64,
                    seg.status
                ])?;
            }
        }
        tx.commit()
    }

    fn map_segment(row: &rusqlite::Row, first_col: usize) -> Result<DownloadSegment> {
        let start_byte: i64 = row.get(first_col + 1)?;
        let end_byte: i64 = row.get(first_col + 2)?;
        let total_bytes = if end_byte >= start_byte { (end_byte - start_byte + 1) as u64 } else { 0 };
        Ok(DownloadSegment {
            segment_index: row.get::<_, i64>(first_col)? as usize,
            start_byte: start_byte as u64,
            end_byte: end_byte as u64,
            downloaded_bytes: row.get::<_, i64>(first_col + 3)? as u64,
            total_bytes,
            status: row.get(first_col + 4)?,
        })
    }

    /// Every download's segments in one query (instead of one query per download).
    fn get_all_segments(conn: &Connection) -> Result<HashMap<String, Vec<DownloadSegment>>> {
        let mut stmt = conn.prepare(
            "SELECT download_id, segment_index, start_byte, end_byte, downloaded_bytes, status
             FROM segments ORDER BY download_id, segment_index",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, Self::map_segment(row, 1)?)))?;
        let mut map: HashMap<String, Vec<DownloadSegment>> = HashMap::new();
        for r in rows {
            let (id, seg) = r?;
            map.entry(id).or_default().push(seg);
        }
        Ok(map)
    }

    pub fn get_segments(conn: &Connection, download_id: &str) -> Result<Vec<DownloadSegment>> {
        let mut stmt = conn.prepare(
            "SELECT segment_index, start_byte, end_byte, downloaded_bytes, status
             FROM segments
             WHERE download_id = ?1
             ORDER BY segment_index ASC",
        )?;
        let rows = stmt.query_map(params![download_id], |row| Self::map_segment(row, 0))?;
        rows.collect()
    }

    fn map_task_row(row: &rusqlite::Row) -> Result<DownloadTask> {
let status_str: String = row.get(8)?;
        let resumable_int: i32 = row.get(9)?;
        let category_str: String = row.get(13)?;
        let connections_int: i64 = row.get(14)?;
        let speed_limit_int: i64 = row.get(15)?;
        let speed_bps_int: i64 = row.get(17)?;
        let eta_sec_int: Option<i64> = row.get(18)?;

        let mirrors_str: Option<String> = row.get(28).unwrap_or(None);
        let mirrors: Vec<String> = mirrors_str
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let min_chunk_size: i64 = row.get(29).unwrap_or(1024 * 1024);
        let proxy_mode: Option<String> = row.get(30).unwrap_or(None);
        let custom_proxy_url: Option<String> = row.get(31).unwrap_or(None);
        let antivirus_status: Option<String> = row.get(32).unwrap_or(None);

        Ok(DownloadTask {
            id: row.get(0)?,
            url: row.get(1)?,
            final_url: row.get(2)?,
            filename: row.get(3)?,
            destination_path: row.get(4)?,
            temp_path: row.get(5)?,
            total_bytes: row.get(6)?,
            downloaded_bytes: row.get(7)?,
            status: DownloadStatus::from_str(&status_str),
            resumable: resumable_int == 1,
            etag: row.get(10)?,
            last_modified: row.get(11)?,
            mime_type: row.get(12)?,
            category: FileCategory::from_str(&category_str),
            connections: connections_int as usize,
            speed_limit: speed_limit_int as u64,
            priority: row.get(16)?,
            speed_bps: speed_bps_int as u64,
            eta_seconds: eta_sec_int.map(|e| e as u64),
            segments: Vec::new(),
            created_at: row.get(19)?,
            updated_at: row.get(20)?,
            completed_at: row.get(21)?,
            error_message: row.get(22)?,
            scheduled_time: row.get(23)?,
            checksum_sha256: row.get(24).unwrap_or(None),
            checksum_md5: row.get(25).unwrap_or(None),
            user_agent: row.get(26).unwrap_or(None),
            referer: row.get(27).unwrap_or(None),
            mirrors,
            min_chunk_size: if min_chunk_size <= 0 { 1024 * 1024 } else { min_chunk_size as u64 },
            proxy_mode,
            custom_proxy_url,
            antivirus_status,
            expected_checksum: row.get(33).unwrap_or(None),
            custom_headers: decode_headers(row.get(34).unwrap_or(None)),
            kind: row.get::<_, Option<String>>(35).unwrap_or(None).unwrap_or_else(|| "http".to_string()),
            info_hash: row.get(36).unwrap_or(None),
            uploaded_bytes: row.get(37).unwrap_or(0),
            peers: 0,
            seeding: row.get::<_, i32>(38).unwrap_or(0) == 1,
            torrent_files: row
                .get::<_, Option<String>>(39)
                .unwrap_or(None)
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default(),
            queue_id: row
                .get::<_, Option<String>>(40)
                .unwrap_or(None)
                .filter(|q| !q.is_empty())
                .unwrap_or_else(|| crate::db::models::MAIN_QUEUE_ID.to_string()),
        })
    }

    // ───────────────────────────── queues ─────────────────────────────

    pub fn list_queues(conn: &Connection) -> Result<Vec<DownloadQueue>> {
        let mut stmt = conn.prepare("SELECT id, data FROM queues ORDER BY position ASC, id ASC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, data) = row?;
            if let Ok(mut q) = serde_json::from_str::<DownloadQueue>(&data) {
                q.id = id;
                out.push(q);
            }
        }
        Ok(out)
    }

    pub fn save_queue(conn: &Connection, queue: &DownloadQueue) -> Result<()> {
        let data = serde_json::to_string(queue).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        conn.execute(
            "INSERT INTO queues (id, position, data) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET position = excluded.position, data = excluded.data",
            params![queue.id, queue.position, data],
        )?;
        Ok(())
    }

    /// Deletes a queue; its downloads move to `fallback`.
    pub fn delete_queue(conn: &Connection, id: &str, fallback: &str) -> Result<()> {
        conn.execute("UPDATE downloads SET queue_id = ?1 WHERE queue_id = ?2", params![fallback, id])?;
        conn.execute("DELETE FROM queues WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_download_queue(conn: &Connection, ids: &[String], queue_id: &str) -> Result<()> {
        for id in ids {
            conn.execute("UPDATE downloads SET queue_id = ?1 WHERE id = ?2", params![queue_id, id])?;
        }
        Ok(())
    }

    pub fn get_all_downloads(conn: &Connection) -> Result<Vec<DownloadTask>> {
        let mut stmt = conn.prepare(
            &format!("SELECT {TASK_COLUMNS} FROM downloads ORDER BY created_at DESC"),
        )?;

        let rows = stmt.query_map([], Self::map_task_row)?;

        let mut segments = Self::get_all_segments(conn).unwrap_or_default();
        let mut tasks = Vec::new();
        for r in rows {
            let mut task = r?;
            task.segments = segments.remove(&task.id).unwrap_or_default();
            tasks.push(task);
        }
        Ok(tasks)
    }

    pub fn get_download(conn: &Connection, id: &str) -> Result<Option<DownloadTask>> {
        let mut stmt = conn.prepare(
            &format!("SELECT {TASK_COLUMNS} FROM downloads WHERE id = ?1"),
        )?;

        let mut rows = stmt.query_map(params![id], Self::map_task_row)?;

        if let Some(r) = rows.next() {
            let mut task = r?;
            task.segments = Self::get_segments(conn, &task.id).unwrap_or_default();
            Ok(Some(task))
        } else {
            Ok(None)
        }
    }

    /// The `.torrent` file contents kept so a torrent can resume without re-fetching its metadata.
    pub fn save_torrent_bytes(conn: &Connection, id: &str, bytes: &[u8]) -> Result<()> {
        conn.execute("UPDATE downloads SET torrent_bytes = ?1 WHERE id = ?2", params![bytes, id])?;
        Ok(())
    }

    pub fn get_torrent_bytes(conn: &Connection, id: &str) -> Result<Option<Vec<u8>>> {
        conn.query_row("SELECT torrent_bytes FROM downloads WHERE id = ?1", params![id], |r| r.get(0))
    }

    /// Whether a torrent has ever been started, i.e. whether files in its folder are ours.
    pub fn torrent_started(conn: &Connection, id: &str) -> Result<bool> {
        conn.query_row("SELECT torrent_started FROM downloads WHERE id = ?1", params![id], |r| Ok(r.get::<_, i32>(0)? == 1))
    }

    pub fn mark_torrent_started(conn: &Connection, id: &str) -> Result<()> {
        conn.execute("UPDATE downloads SET torrent_started = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_torrent_state(conn: &Connection, id: &str, uploaded_bytes: i64, seeding: bool) -> Result<()> {
        conn.execute(
            "UPDATE downloads SET uploaded_bytes = ?1, seeding = ?2 WHERE id = ?3",
            params![uploaded_bytes, if seeding { 1 } else { 0 }, id],
        )?;
        Ok(())
    }

    pub fn find_by_info_hash(conn: &Connection, info_hash: &str) -> Result<Option<String>> {
        let mut stmt = conn.prepare("SELECT id FROM downloads WHERE info_hash = ?1 LIMIT 1")?;
        let mut rows = stmt.query(params![info_hash])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get(0)?),
            None => None,
        })
    }

    pub fn update_antivirus_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
        conn.execute("UPDATE downloads SET antivirus_status = ?1 WHERE id = ?2", params![status, id])?;
        Ok(())
    }

    pub fn save_site_credential(conn: &Connection, cred: &SiteCredential) -> Result<()> {
        conn.execute(
            "INSERT INTO site_credentials (id, domain, username, password, token, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(domain) DO UPDATE SET
                username = excluded.username,
                password = excluded.password,
                token = excluded.token,
                notes = excluded.notes;",
            params![
                cred.id,
                cred.domain.to_lowercase().trim(),
                cred.username.clone().unwrap_or_default(),
                cred.password.as_deref().map(crate::secrets::protect),
                cred.token_or_cookie.as_deref().map(crate::secrets::protect),
                cred.notes
            ],
        )?;
        Ok(())
    }

    pub fn get_all_site_credentials(conn: &Connection) -> Result<Vec<SiteCredential>> {
        let mut stmt = conn.prepare("SELECT id, domain, username, password, token, notes FROM site_credentials ORDER BY domain ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(SiteCredential {
                id: row.get(0)?,
                domain: row.get(1)?,
                username: row.get::<_, Option<String>>(2)?.filter(|u| !u.is_empty()),
                password: row.get::<_, Option<String>>(3)?.and_then(|s| crate::secrets::unprotect(&s)),
                token_or_cookie: row.get::<_, Option<String>>(4)?.and_then(|s| crate::secrets::unprotect(&s)),
                notes: row.get(5)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn get_site_credential_for_domain(conn: &Connection, domain: &str) -> Result<Option<SiteCredential>> {
        let clean_domain = domain.to_lowercase().trim().to_string();
        let mut stmt = conn.prepare("SELECT id, domain, username, password, token, notes FROM site_credentials WHERE domain = ?1 OR ?1 LIKE '%.' || domain LIMIT 1")?;
        let mut rows = stmt.query_map(params![clean_domain], |row| {
            Ok(SiteCredential {
                id: row.get(0)?,
                domain: row.get(1)?,
                username: row.get::<_, Option<String>>(2)?.filter(|u| !u.is_empty()),
                password: row.get::<_, Option<String>>(3)?.and_then(|s| crate::secrets::unprotect(&s)),
                token_or_cookie: row.get::<_, Option<String>>(4)?.and_then(|s| crate::secrets::unprotect(&s)),
                notes: row.get(5)?,
            })
        })?;

        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    /// One-time upgrade: encrypts any credentials that were stored as plaintext
    /// by earlier versions. Safe to run on every start.
    pub fn protect_existing_secrets(conn: &Connection) -> Result<usize> {
        let mut stmt = conn.prepare("SELECT id, password, token FROM site_credentials")?;
        let rows: Vec<(String, Option<String>, Option<String>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_>>()?;
        drop(stmt);

        let needs = |v: &Option<String>| v.as_deref().map(|s| !s.is_empty() && !crate::secrets::is_protected(s)).unwrap_or(false);
        let mut updated = 0;
        for (id, password, token) in rows {
            if needs(&password) || needs(&token) {
                conn.execute(
                    "UPDATE site_credentials SET password = ?1, token = ?2 WHERE id = ?3",
                    params![
                        password.as_deref().map(crate::secrets::protect),
                        token.as_deref().map(crate::secrets::protect),
                        id
                    ],
                )?;
                updated += 1;
            }
        }
        Ok(updated)
    }

    pub fn delete_site_credential(conn: &Connection, id: &str) -> Result<()> {
        conn.execute("DELETE FROM site_credentials WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_checksum(
        conn: &Connection,
        id: &str,
        sha256: Option<&str>,
        md5: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE downloads SET checksum_sha256 = ?1, checksum_md5 = ?2 WHERE id = ?3",
            params![sha256, md5, id],
        )?;
        Ok(())
    }

    pub fn delete_download(conn: &Connection, id: &str) -> Result<()> {
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn load_settings(conn: &Connection) -> Result<AppSettings> {
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = 'app_settings'")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            let json_str: String = row.get(0)?;
            if let Ok(settings) = serde_json::from_str::<AppSettings>(&json_str) {
                return Ok(settings);
            }
        }
        let default_settings = AppSettings::default();
        let _ = Self::save_settings(conn, &default_settings);
        Ok(default_settings)
    }

    pub fn save_settings(conn: &Connection, settings: &AppSettings) -> Result<()> {
        let json_str = serde_json::to_string(settings).unwrap_or_default();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app_settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
            params![json_str],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::db::models::{DownloadSegment, DownloadStatus, DownloadTask, FileCategory};

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        Repository::create_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_task_crud_and_status_transitions() {
        let conn = setup_test_db();
        let task = DownloadTask {
            id: "test-uuid-1".to_string(),
            url: "https://example.com/test.zip".to_string(),
            final_url: Some("https://example.com/test.zip".to_string()),
            filename: "test.zip".to_string(),
            destination_path: "C:/Downloads".to_string(),
            temp_path: "C:/Downloads/test.zip.nexpart".to_string(),
            total_bytes: 10485760,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            resumable: true,
            etag: Some("\"etag123\"".to_string()),
            last_modified: None,
            mime_type: Some("application/zip".to_string()),
            category: FileCategory::Compressed,
            connections: 4,
            speed_limit: 0,
            priority: 1,
            speed_bps: 0,
            eta_seconds: None,
            segments: vec![
                DownloadSegment {
                    segment_index: 0,
                    start_byte: 0,
                    end_byte: 2621439,
                    downloaded_bytes: 0,
                    total_bytes: 2621440,
                    status: "pending".to_string(),
                },
                DownloadSegment {
                    segment_index: 1,
                    start_byte: 2621440,
                    end_byte: 5242879,
                    downloaded_bytes: 0,
                    total_bytes: 2621440,
                    status: "pending".to_string(),
                },
            ],
            created_at: 1000,
            updated_at: 1000,
            completed_at: None,
            error_message: None,
            scheduled_time: None,
            checksum_sha256: None,
            checksum_md5: None,
            user_agent: None,
            referer: None,
            mirrors: vec!["https://mirror1.example.com/test.zip".to_string()],
            min_chunk_size: 1024 * 1024,
            proxy_mode: Some("direct".to_string()),
            custom_proxy_url: None,
            antivirus_status: None,
            expected_checksum: Some("abc123".to_string()),
            custom_headers: [("Cookie".to_string(), "sid=1".to_string())].into_iter().collect(),
            kind: "http".to_string(),
            info_hash: None,
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: vec![],
            queue_id: "main".to_string(),
        };

        // 1. Save
        Repository::save_download(&conn, &task).unwrap();

        // 2. Fetch
        let fetched = Repository::get_download(&conn, "test-uuid-1").unwrap().expect("Task should exist");
        assert_eq!(fetched.filename, "test.zip");
        assert_eq!(fetched.status, DownloadStatus::Queued);
        assert_eq!(fetched.segments.len(), 2);
        assert_eq!(fetched.expected_checksum.as_deref(), Some("abc123"));
        assert_eq!(fetched.custom_headers.get("Cookie").map(String::as_str), Some("sid=1"));
        // Cookies and tokens must not be readable straight from the database file.
        let raw: Option<String> = conn
            .query_row("SELECT custom_headers FROM downloads WHERE id = ?1", ["test-uuid-1"], |r| r.get(0))
            .unwrap();
        if cfg!(windows) {
            assert!(!raw.unwrap().contains("sid=1"), "headers stored in plaintext");
        }

        // 3. Progress update (only applies while the task is downloading)
        Repository::update_status(&conn, "test-uuid-1", DownloadStatus::Downloading, None, None, 1040).unwrap();
        Repository::update_progress(&conn, "test-uuid-1", 5000000, 1024000, Some(5), 1050).unwrap();
        let fetched2 = Repository::get_download(&conn, "test-uuid-1").unwrap().unwrap();
        assert_eq!(fetched2.downloaded_bytes, 5000000);
        assert_eq!(fetched2.status, DownloadStatus::Downloading);
        assert_eq!(fetched2.speed_bps, 1024000);
        assert_eq!(fetched2.eta_seconds, Some(5));

        // 4. Status update to Completed
        Repository::update_status(&conn, "test-uuid-1", DownloadStatus::Completed, None, Some(1100), 1100).unwrap();
        let fetched3 = Repository::get_download(&conn, "test-uuid-1").unwrap().unwrap();
        assert_eq!(fetched3.status, DownloadStatus::Completed);
        assert_eq!(fetched3.completed_at, Some(1100));

        // 5. Delete
        Repository::delete_download(&conn, "test-uuid-1").unwrap();
        let fetched_del = Repository::get_download(&conn, "test-uuid-1").unwrap();
        assert!(fetched_del.is_none());
    }

    #[test]
    fn test_settings_persistence() {
        let conn = setup_test_db();
        let mut settings = Repository::load_settings(&conn).unwrap();
        assert_eq!(settings.max_active_downloads, 3);

        settings.max_active_downloads = 5;
        settings.default_connections = 16;
        settings.theme = "light".to_string();

        Repository::save_settings(&conn, &settings).unwrap();
        let reloaded = Repository::load_settings(&conn).unwrap();
        assert_eq!(reloaded.max_active_downloads, 5);
        assert_eq!(reloaded.default_connections, 16);
        assert_eq!(reloaded.theme, "light");
    }

    #[test]
    fn queues_round_trip_and_deleting_one_moves_its_downloads_to_the_main_queue() {
        let conn = setup_test_db();
        let night = DownloadQueue {
            id: "night".into(),
            name: "Night".into(),
            schedule_enabled: true,
            start_time: "01:30".into(),
            stop_time: Some("06:00".into()),
            speed_limit: 2_000_000,
            after_done: "shutdown".into(),
            position: 1,
            ..Default::default()
        };
        Repository::save_queue(&conn, &night).unwrap();
        let mut main = DownloadQueue { id: "main".into(), name: "Main queue".into(), ..Default::default() };
        Repository::save_queue(&conn, &main).unwrap();
        main.name = "Renamed".into();
        Repository::save_queue(&conn, &main).unwrap();

        let list = Repository::list_queues(&conn).unwrap();
        assert_eq!(list.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(), vec!["main", "night"]);
        assert_eq!(list[0].name, "Renamed");
        assert_eq!((list[1].start_time.as_str(), list[1].after_done.as_str(), list[1].speed_limit), ("01:30", "shutdown", 2_000_000));

        let task = sample_task("t1");
        Repository::save_download(&conn, &task).unwrap();
        Repository::set_download_queue(&conn, &["t1".to_string()], "night").unwrap();
        assert_eq!(Repository::get_download(&conn, "t1").unwrap().unwrap().queue_id, "night");

        Repository::delete_queue(&conn, "night", "main").unwrap();
        assert_eq!(Repository::get_download(&conn, "t1").unwrap().unwrap().queue_id, "main");
        assert_eq!(Repository::list_queues(&conn).unwrap().len(), 1);
    }

    #[test]
    fn settings_saved_before_versioning_count_as_version_zero_and_new_installs_start_current() {
        let conn = setup_test_db();
        conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('app_settings', ?1)", rusqlite::params![r#"{"auto_categorize": false, "max_active_downloads": 4}"#]).unwrap();
        let old = Repository::load_settings(&conn).unwrap();
        assert_eq!(old.settings_version, 0, "an old save has no version");
        assert!(!old.auto_categorize);
        assert_eq!(old.max_active_downloads, 4);
        let fresh = AppSettings::default();
        assert_eq!(fresh.settings_version, crate::db::models::CURRENT_SETTINGS_VERSION);
        assert!(fresh.auto_categorize, "sorting into folders by type is on by default");
    }

    #[test]
    fn test_site_credentials_crud() {
        let conn = setup_test_db();
        let cred = SiteCredential {
            id: "cred-1".to_string(),
            domain: "example.com".to_string(),
            username: Some("admin".to_string()),
            password: Some("secret123".to_string()),
            token_or_cookie: None,
            notes: Some("Test note".to_string()),
        };

        Repository::save_site_credential(&conn, &cred).unwrap();

        let list = Repository::get_all_site_credentials(&conn).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].domain, "example.com");

        let fetched = Repository::get_site_credential_for_domain(&conn, "sub.example.com").unwrap().unwrap();
        assert_eq!(fetched.username.as_deref(), Some("admin"));

        // Only the exact host or a real subdomain may match: never a look-alike.
        assert!(Repository::get_site_credential_for_domain(&conn, "evilexample.com").unwrap().is_none());
        assert!(Repository::get_site_credential_for_domain(&conn, "example.com").unwrap().is_some());

        Repository::delete_site_credential(&conn, "cred-1").unwrap();
        let list_empty = Repository::get_all_site_credentials(&conn).unwrap();
        assert_eq!(list_empty.len(), 0);
    }

    #[test]
    fn test_token_only_credential_round_trips() {
        let conn = setup_test_db();
        let cred = SiteCredential {
            id: "cred-2".to_string(),
            domain: "files.example.org".to_string(),
            username: None,
            password: None,
            token_or_cookie: Some("sessionid=abc".to_string()),
            notes: None,
        };
        Repository::save_site_credential(&conn, &cred).unwrap();
        let list = Repository::get_all_site_credentials(&conn).unwrap();
        assert_eq!(list[0].username, None);
        assert_eq!(list[0].token_or_cookie.as_deref(), Some("sessionid=abc"));

        // The shape the UI sends (`token_or_cookie`, no username) must deserialize.
        let parsed: SiteCredential =
            serde_json::from_str(r#"{"id":"x","domain":"a.com","token_or_cookie":"t"}"#).unwrap();
        assert_eq!(parsed.token_or_cookie.as_deref(), Some("t"));
    }

    #[test]
    fn test_late_progress_never_overwrites_a_pause() {
        let conn = setup_test_db();
        let mut task = sample_task("pause-race");
        task.status = DownloadStatus::Downloading;
        Repository::save_download(&conn, &task).unwrap();

        Repository::update_status(&conn, "pause-race", DownloadStatus::Paused, None, None, 10).unwrap();
        // A progress tick that was already in flight when the user paused:
        Repository::update_progress(&conn, "pause-race", 999, 500, Some(3), 11).unwrap();

        let t = Repository::get_download(&conn, "pause-race").unwrap().unwrap();
        assert_eq!(t.status, DownloadStatus::Paused);
        assert_eq!(t.downloaded_bytes, 0);
    }

    #[test]
    fn test_interrupted_downloads_are_recovered() {
        let conn = setup_test_db();
        let mut running = sample_task("was-running");
        running.status = DownloadStatus::Downloading;
        let mut done = sample_task("was-done");
        done.status = DownloadStatus::Completed;
        Repository::save_download(&conn, &running).unwrap();
        Repository::save_download(&conn, &done).unwrap();

        let n = Repository::reset_interrupted(&conn, DownloadStatus::Queued, 50).unwrap();
        assert_eq!(n, 1);
        assert_eq!(Repository::get_download(&conn, "was-running").unwrap().unwrap().status, DownloadStatus::Queued);
        assert_eq!(Repository::get_download(&conn, "was-done").unwrap().unwrap().status, DownloadStatus::Completed);
    }

    fn sample_task(id: &str) -> DownloadTask {
        DownloadTask {
            id: id.to_string(),
            url: "https://example.com/f.bin".to_string(),
            final_url: None,
            filename: "f.bin".to_string(),
            destination_path: "C:/Downloads".to_string(),
            temp_path: "C:/Downloads/f.bin.nexpart".to_string(),
            total_bytes: 1000,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            resumable: true,
            etag: None,
            last_modified: None,
            mime_type: None,
            category: FileCategory::Other,
            connections: 1,
            speed_limit: 0,
            priority: 0,
            speed_bps: 0,
            eta_seconds: None,
            segments: vec![],
            created_at: 1,
            updated_at: 1,
            completed_at: None,
            error_message: None,
            scheduled_time: None,
            checksum_sha256: None,
            checksum_md5: None,
            user_agent: None,
            referer: None,
            mirrors: vec![],
            min_chunk_size: 1048576,
            proxy_mode: None,
            custom_proxy_url: None,
            antivirus_status: None,
            expected_checksum: None,
            custom_headers: Default::default(),
            kind: "http".to_string(),
            info_hash: None,
            uploaded_bytes: 0,
            peers: 0,
            seeding: false,
            torrent_files: vec![],
            queue_id: "main".to_string(),
        }
    }
}
