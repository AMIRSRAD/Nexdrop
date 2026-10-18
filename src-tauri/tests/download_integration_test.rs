//! Integration tests for the download engine, run against a local mock server
//! whose behaviour (failures, resets, slow connections...) is configurable.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadSegment, DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::downloader::DownloadEngine;
use nexdrop_lib::engine::ranges::{save_map, DoneMap};

// ───────────────────────────── mock server ─────────────────────────────

#[derive(Clone, Copy)]
enum Mode {
    /// Answers Range requests correctly with 206.
    Normal,
    /// Ignores Range and always answers 200 with the whole file.
    IgnoreRange,
    /// The first `n` requests get this HTTP status; later ones are normal.
    FailFirst(usize, u16),
    /// The first `n` responses are cut off halfway; later ones are normal.
    CutFirst(usize),
    /// Promises a body, sends none, and hangs up: a connection that never makes progress.
    CutAlways,
    /// Requests for a range that starts at 0 trickle slowly; all others are fast.
    SlowFirstRange,
    /// Every response trickles out slowly.
    Slow { chunk: usize, delay_ms: u64 },
    /// Always 404.
    NotFound,
}

struct TestServer {
    base: String,
    requests: Arc<AtomicUsize>,
    range_starts: Arc<Mutex<Vec<usize>>>,
    /// The raw text of every request received (lower-cased), to inspect headers.
    raw_requests: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    fn requests_text(&self) -> Vec<String> {
        self.raw_requests.lock().unwrap().clone()
    }
    fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.base)
    }
    fn request_count(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
    fn min_range_start(&self) -> usize {
        self.range_starts.lock().unwrap().iter().copied().min().unwrap_or(usize::MAX)
    }
}

async fn start_server(payload: Vec<u8>, mode: Mode) -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = Arc::new(payload);
    let requests = Arc::new(AtomicUsize::new(0));
    let range_starts = Arc::new(Mutex::new(Vec::new()));
    let raw_requests = Arc::new(Mutex::new(Vec::new()));
    let (req_counter, starts_log, raw_log) = (requests.clone(), range_starts.clone(), raw_requests.clone());

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let payload = payload.clone();
            let n = req_counter.fetch_add(1, Ordering::SeqCst);
            let starts_log = starts_log.clone();
            let raw_log = raw_log.clone();

            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let total = payload.len();
                raw_log.lock().unwrap().push(request.to_lowercase());

                let range = request
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("range:"))
                    .and_then(|l| l.split('=').nth(1).map(|v| v.to_string()));
                let (start, end) = match &range {
                    Some(r) => {
                        let mut it = r.trim().split('-');
                        let s: usize = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        let e: usize = it
                            .next()
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(total - 1)
                            .min(total - 1);
                        (s, e)
                    }
                    None => (0, total - 1),
                };
                if range.is_some() {
                    starts_log.lock().unwrap().push(start);
                }

                // Error responses
                let status_only = match mode {
                    Mode::NotFound => Some(404u16),
                    Mode::FailFirst(k, status) if n < k => Some(status),
                    _ => None,
                };
                if let Some(status) = status_only {
                    let head = format!("HTTP/1.1 {status} X\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    let _ = socket.write_all(head.as_bytes()).await;
                    return;
                }

                if matches!(mode, Mode::IgnoreRange) || range.is_none() {
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n");
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&payload).await;
                    return;
                }

                let body = &payload[start..=end];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;

                match mode {
                    Mode::CutAlways => {}
                    Mode::CutFirst(k) if n < k => {
                        let _ = socket.write_all(&body[..body.len() / 2]).await;
                    }
                    Mode::Slow { chunk, delay_ms } => trickle(&mut socket, body, chunk, delay_ms).await,
                    Mode::SlowFirstRange if start == 0 => trickle(&mut socket, body, 16 * 1024, 20).await,
                    _ => {
                        let _ = socket.write_all(body).await;
                    }
                }
                let _ = socket.flush().await;
            });
        }
    });

    TestServer { base: format!("http://{addr}"), requests, range_starts, raw_requests }
}

async fn trickle(socket: &mut tokio::net::TcpStream, body: &[u8], chunk: usize, delay_ms: u64) {
    for piece in body.chunks(chunk) {
        if socket.write_all(piece).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
}

// ───────────────────────────── test helpers ─────────────────────────────

fn patterned(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 7) % 251) as u8).collect()
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("nexdrop_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make_task(url: &str, dir: &Path, len: usize, name: &str, connections: usize) -> DownloadTask {
    DownloadTask {
        id: format!("t-{name}"),
        url: url.to_string(),
        final_url: Some(url.to_string()),
        filename: name.to_string(),
        destination_path: dir.to_string_lossy().to_string(),
        temp_path: dir.join(format!("{name}.nexpart")).to_string_lossy().to_string(),
        total_bytes: len as i64,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: None,
        last_modified: None,
        mime_type: None,
        category: FileCategory::Other,
        connections,
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments: vec![DownloadSegment {
            segment_index: 0,
            start_byte: 0,
            end_byte: (len - 1) as u64,
            downloaded_bytes: 0,
            total_bytes: len as u64,
            status: "pending".into(),
        }],
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
        // Small so that modest test files still split into several ranges.
        min_chunk_size: 64 * 1024,
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
    }
}

fn fast_engine() -> DownloadEngine {
    DownloadEngine::new().with_retry_base(Duration::from_millis(5))
}

async fn run(engine: &DownloadEngine, task: DownloadTask) -> Result<DownloadTask, String> {
    let (_tx, rx) = watch::channel(false);
    engine.run_download(task, rx, |_| {}).await
}

fn assert_file_equals(dir: &Path, task: &DownloadTask, payload: &[u8]) {
    let out = std::fs::read(dir.join(&task.filename)).expect("final file must exist");
    assert_eq!(out.len(), payload.len(), "final size differs");
    assert!(out == payload, "final contents differ from the original");
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".nexpart") || n.contains(".nexmerge"))
        .collect()
}

// ───────────────────────────────── tests ─────────────────────────────────

#[tokio::test]
async fn segmented_download_is_byte_exact_and_leaves_no_temp_files() {
    let payload = patterned(3 * 1024 * 1024 + 123);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("exact");

    let task = make_task(&server.url("exact.bin"), &dir, payload.len(), "exact.bin", 8);
    let done = run(&fast_engine(), task).await.unwrap();

    assert_eq!(done.status, DownloadStatus::Completed);
    assert_eq!(done.downloaded_bytes, payload.len() as i64);
    assert_file_equals(&dir, &done, &payload);
    assert!(leftovers(&dir).is_empty(), "temp files left behind: {:?}", leftovers(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn single_connection_ranged_download_works() {
    let payload = patterned(500_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("one");

    let task = make_task(&server.url("one.bin"), &dir, payload.len(), "one.bin", 1);
    let done = run(&fast_engine(), task).await.unwrap();
    assert_file_equals(&dir, &done, &payload);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Resume skips what the progress map says is already on disk.
#[tokio::test]
async fn resume_continues_from_the_saved_progress_map() {
    let payload = patterned(1_000_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("resume");

    // A previous run got the first 400_000 bytes down before stopping.
    let mut partial = vec![0u8; payload.len()];
    partial[..400_000].copy_from_slice(&payload[..400_000]);
    std::fs::write(dir.join("resume.bin.nexpart"), &partial).unwrap();
    save_map(&dir.join("resume.bin.nexpart.map"), payload.len() as u64, &DoneMap::from_ranges(vec![(0, 400_000)])).unwrap();

    let task = make_task(&server.url("resume.bin"), &dir, payload.len(), "resume.bin", 4);
    let done = run(&fast_engine(), task).await.unwrap();

    assert_file_equals(&dir, &done, &payload);
    assert!(
        server.min_range_start() >= 400_000,
        "re-downloaded data that was already on disk (first requested byte: {})",
        server.min_range_start()
    );
    assert!(leftovers(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A progress map written for a different-sized file must not be trusted.
#[tokio::test]
async fn progress_map_for_a_different_file_is_ignored() {
    let payload = patterned(600_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("badmap");

    std::fs::write(dir.join("m.bin.nexpart"), vec![0xEEu8; 700_000]).unwrap();
    save_map(&dir.join("m.bin.nexpart.map"), 700_000, &DoneMap::from_ranges(vec![(0, 700_000)])).unwrap();

    let task = make_task(&server.url("m.bin"), &dir, payload.len(), "m.bin", 3);
    let done = run(&fast_engine(), task).await.unwrap();
    assert_file_equals(&dir, &done, &payload);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server that ignores `Range` must not get its whole body written into every
/// part: the download switches to a single stream and still comes out exact.
#[tokio::test]
async fn server_ignoring_range_falls_back_to_a_single_stream() {
    let payload = patterned(600_000);
    let server = start_server(payload.clone(), Mode::IgnoreRange).await;
    let dir = temp_dir("norange");

    let task = make_task(&server.url("norange.bin"), &dir, 600_000, "norange.bin", 4);
    let done = run(&fast_engine(), task).await.expect("falls back to one stream");
    assert_file_equals(&dir, &done, &payload);
    assert!(!done.resumable, "the task should now be marked as not resumable");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Transient server errors (503) are retried instead of failing the download.
#[tokio::test]
async fn transient_server_errors_are_retried() {
    let payload = patterned(800_000);
    let server = start_server(payload.clone(), Mode::FailFirst(3, 503)).await;
    let dir = temp_dir("flaky");

    let task = make_task(&server.url("flaky.bin"), &dir, payload.len(), "flaky.bin", 4);
    let done = run(&fast_engine(), task).await.unwrap();
    assert_file_equals(&dir, &done, &payload);
    assert!(server.request_count() > 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Connections that drop mid-body resume from where they stopped.
#[tokio::test]
async fn dropped_connections_resume_mid_range() {
    let payload = patterned(1_200_000);
    let server = start_server(payload.clone(), Mode::CutFirst(5)).await;
    let dir = temp_dir("reset");

    let task = make_task(&server.url("reset.bin"), &dir, payload.len(), "reset.bin", 4);
    let done = run(&fast_engine(), task).await.unwrap();
    assert_file_equals(&dir, &done, &payload);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A connection that never delivers anything eventually fails with a clear reason.
#[tokio::test]
async fn hopeless_connection_gives_up_with_a_clear_error() {
    let payload = patterned(300_000);
    let server = start_server(payload, Mode::CutAlways).await;
    let dir = temp_dir("giveup");

    let task = make_task(&server.url("g.bin"), &dir, 300_000, "g.bin", 1);
    let err = run(&fast_engine(), task).await.expect_err("should give up");
    assert!(err.contains("gave up"), "unexpected error: {err}");
    assert!(!dir.join("g.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Permanent errors (404) fail immediately instead of burning through retries.
#[tokio::test]
async fn not_found_fails_fast_without_retrying() {
    let server = start_server(patterned(100_000), Mode::NotFound).await;
    let dir = temp_dir("404");

    let task = make_task(&server.url("x.bin"), &dir, 100_000, "x.bin", 2);
    let started = Instant::now();
    let err = run(&DownloadEngine::new(), task).await.expect_err("404 must fail");
    assert!(err.contains("404"), "unexpected error: {err}");
    assert!(started.elapsed() < Duration::from_secs(2), "should not have backed off and retried");
    assert!(server.request_count() <= 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// When one range is slow, idle connections take over the tail of it.
#[tokio::test]
async fn idle_connections_steal_work_from_a_slow_range() {
    let payload = patterned(3 * 1024 * 1024);
    let server = start_server(payload.clone(), Mode::SlowFirstRange).await;
    let dir = temp_dir("steal");

    let task = make_task(&server.url("steal.bin"), &dir, payload.len(), "steal.bin", 4);
    let done = run(&fast_engine(), task).await.unwrap();

    assert_file_equals(&dir, &done, &payload);
    assert!(
        server.request_count() > 4,
        "expected extra requests from work stealing, saw {}",
        server.request_count()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pausing lands mid-transfer, keeps what arrived, and a later run finishes byte-exact.
#[tokio::test]
async fn pause_mid_transfer_then_resume_is_byte_exact() {
    let payload = patterned(1_000_000);
    let server = start_server(payload.clone(), Mode::Slow { chunk: 16 * 1024, delay_ms: 15 }).await;
    let dir = temp_dir("pause");

    let task = make_task(&server.url("p.bin"), &dir, payload.len(), "p.bin", 2);
    let engine = fast_engine();
    let (pause_tx, pause_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let _ = pause_tx.send(true);
    });

    let paused = engine.run_download(task, pause_rx, |_| {}).await.unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused, "finished before the pause landed");
    assert!(paused.downloaded_bytes > 0 && paused.downloaded_bytes < payload.len() as i64);
    assert!(!dir.join("p.bin").exists(), "a paused download must not produce a final file");
    assert!(dir.join("p.bin.nexpart.map").exists(), "progress must be saved for resume");

    let mut resumed = paused;
    resumed.status = DownloadStatus::Downloading;
    resumed.downloaded_bytes = 0;
    let done = run(&engine, resumed).await.unwrap();
    assert_eq!(done.status, DownloadStatus::Completed);
    assert_file_equals(&dir, &done, &payload);
    assert!(leftovers(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pause must react immediately even while a speed limit is making connections wait.
#[tokio::test]
async fn pause_is_immediate_under_a_speed_limit() {
    let payload = patterned(2_000_000);
    let server = start_server(payload, Mode::Normal).await;
    let dir = temp_dir("limitpause");

    let mut task = make_task(&server.url("l.bin"), &dir, 2_000_000, "l.bin", 2);
    task.speed_limit = 20_000; // 20 KB/s: the file would take minutes
    let (pause_tx, pause_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let _ = pause_tx.send(true);
    });

    let started = Instant::now();
    let paused = fast_engine().run_download(task, pause_rx, |_| {}).await.unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(started.elapsed() < Duration::from_secs(3), "pause took {:?}", started.elapsed());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn per_download_speed_limit_is_enforced() {
    let payload = patterned(300_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("limit");

    let mut task = make_task(&server.url("lim.bin"), &dir, payload.len(), "lim.bin", 2);
    task.speed_limit = 100_000; // 100 KB/s => ~2.7s for 300 KB (minus a small burst)
    let started = Instant::now();
    let done = run(&fast_engine(), task).await.unwrap();
    let took = started.elapsed();

    assert_file_equals(&dir, &done, &payload);
    assert!(took > Duration::from_millis(1800), "finished too fast for the limit: {took:?}");
    assert!(took < Duration::from_secs(10), "limit slowed things down far too much: {took:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn global_speed_limit_applies_to_every_download() {
    let payload = patterned(300_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("global");

    let engine = fast_engine();
    engine.set_global_speed_limit(100_000);
    let task = make_task(&server.url("g.bin"), &dir, payload.len(), "g.bin", 3);
    let started = Instant::now();
    let done = run(&engine, task).await.unwrap();

    assert_file_equals(&dir, &done, &payload);
    assert!(started.elapsed() > Duration::from_millis(1800));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A mirror that 404s must not sink the download: its ranges fall back to the original.
#[tokio::test]
async fn broken_mirror_falls_back_to_the_original_url() {
    let payload = patterned(1_000_000);
    let primary = start_server(payload.clone(), Mode::Normal).await;
    let bad_mirror = start_server(payload.clone(), Mode::NotFound).await;
    let dir = temp_dir("mirror");

    let mut task = make_task(&primary.url("m.bin"), &dir, payload.len(), "m.bin", 4);
    task.mirrors = vec![bad_mirror.url("m.bin")];
    let done = run(&fast_engine(), task).await.unwrap();

    assert_file_equals(&dir, &done, &payload);
    assert!(bad_mirror.request_count() > 0, "the mirror should have been tried");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Servers without range support get a plain single-stream download, even if
/// a stale partial file is lying around.
#[tokio::test]
async fn non_resumable_download_streams_the_whole_file() {
    let payload = patterned(300_000);
    let server = start_server(payload.clone(), Mode::IgnoreRange).await;
    let dir = temp_dir("single");
    std::fs::write(dir.join("s.bin.nexpart"), &payload[..100_000]).unwrap();

    let mut task = make_task(&server.url("s.bin"), &dir, payload.len(), "s.bin", 1);
    task.resumable = false;
    let done = run(&fast_engine(), task).await.unwrap();
    assert_file_equals(&dir, &done, &payload);
    let _ = std::fs::remove_dir_all(&dir);
}

// ─────────────────────────── Phase 3: headers & safety ───────────────────────────

/// Cookies/tokens go to the original host only. Mirrors are different servers and must never see them.
#[tokio::test]
async fn custom_headers_reach_the_original_host_but_never_mirrors() {
    let payload = patterned(1_000_000);
    let primary = start_server(payload.clone(), Mode::Normal).await;
    let mirror = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("headers");

    let mut task = make_task(&primary.url("h.bin"), &dir, payload.len(), "h.bin", 4);
    task.mirrors = vec![mirror.url("h.bin")];
    task.custom_headers.insert("Cookie".into(), "sid=abc123".into());
    task.custom_headers.insert("X-Token".into(), "secret-token".into());
    // The engine owns these; a user-supplied value must be ignored, not allowed to corrupt the transfer.
    task.custom_headers.insert("Range".into(), "bytes=0-1".into());
    task.custom_headers.insert("Accept-Encoding".into(), "gzip".into());

    let (_tx, rx) = watch::channel(false);
    let done = fast_engine()
        .run_download_with(task, rx, vec![("Authorization".into(), "Bearer vault-token".into())], |_| {})
        .await
        .unwrap();
    assert_file_equals(&dir, &done, &payload);

    let primary_reqs = primary.requests_text();
    assert!(!primary_reqs.is_empty());
    for r in &primary_reqs {
        assert!(r.contains("cookie: sid=abc123"), "primary request missing cookie:\n{r}");
        assert!(r.contains("x-token: secret-token"));
        assert!(r.contains("authorization: bearer vault-token"));
    }

    let mirror_reqs = mirror.requests_text();
    assert!(!mirror_reqs.is_empty(), "the mirror should have served some ranges");
    for r in &mirror_reqs {
        assert!(!r.contains("sid=abc123") && !r.contains("secret-token") && !r.contains("vault-token"), "leaked to mirror:\n{r}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A task's own header wins over one from a saved login.
#[tokio::test]
async fn task_headers_override_saved_login_headers() {
    let payload = patterned(200_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("prec");

    let mut task = make_task(&server.url("p.bin"), &dir, payload.len(), "p.bin", 1);
    task.custom_headers.insert("Authorization".into(), "Bearer mine".into());
    let (_tx, rx) = watch::channel(false);
    fast_engine()
        .run_download_with(task, rx, vec![("Authorization".into(), "Bearer vault".into())], |_| {})
        .await
        .unwrap();

    let reqs = server.requests_text();
    assert!(reqs.iter().all(|r| r.contains("bearer mine") && !r.contains("bearer vault")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Finishing a download never replaces a file that's already there.
#[tokio::test]
async fn finished_file_never_overwrites_an_existing_one() {
    let payload = patterned(400_000);
    let server = start_server(payload.clone(), Mode::Normal).await;
    let dir = temp_dir("keep");
    std::fs::write(dir.join("e.bin"), b"keep me").unwrap();

    let task = make_task(&server.url("e.bin"), &dir, payload.len(), "e.bin", 2);
    let done = run(&fast_engine(), task).await.unwrap();

    assert_eq!(done.filename, "e (1).bin");
    assert_eq!(std::fs::read(dir.join("e.bin")).unwrap(), b"keep me", "the existing file was clobbered");
    assert_file_equals(&dir, &done, &payload);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Same guarantee for servers without range support.
#[tokio::test]
async fn single_stream_download_never_overwrites_an_existing_file_either() {
    let payload = patterned(150_000);
    let server = start_server(payload.clone(), Mode::IgnoreRange).await;
    let dir = temp_dir("keep1");
    std::fs::write(dir.join("s.bin"), b"keep me").unwrap();

    let mut task = make_task(&server.url("s.bin"), &dir, payload.len(), "s.bin", 1);
    task.resumable = false;
    let done = run(&fast_engine(), task).await.unwrap();

    assert_eq!(done.filename, "s (1).bin");
    assert_eq!(std::fs::read(dir.join("s.bin")).unwrap(), b"keep me");
    let _ = std::fs::remove_dir_all(&dir);
}
