//! The download engine.
//!
//! Resumable downloads run on a *range engine*:
//!
//! * One target file (`name.nexpart`), preallocated once and written at the
//!   correct offsets, so there is nothing to merge afterwards.
//! * A dedicated writer thread receives large batched writes; network tasks
//!   never block on the disk.
//! * A *done map* of byte ranges known to be on disk is saved next to the file
//!   (`name.nexpart.map`), which is exactly what resume needs.
//! * **Dynamic segmentation**: when a connection runs out of work it takes the
//!   upper half of the largest range still being downloaded, so no connection
//!   idles while another is still slogging through its share.
//! * Each range retries transient failures with exponential backoff; a
//!   throttling server (429/503) makes the engine use fewer connections.
//! * Connections ramp up gradually instead of all opening at once.
//!
//! Servers that cannot do ranges fall back to a plain single-stream download.

use std::collections::VecDeque;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use parking_lot::Mutex;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_RANGE, RANGE, RETRY_AFTER, USER_AGENT};
use reqwest::StatusCode;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use crate::db::models::{DownloadProgressEvent, DownloadSegment, DownloadStatus, DownloadTask};
use crate::engine::client::ClientPool;
use crate::engine::auth::build_header_list;
use crate::engine::fileio::{free_space, open_preallocated, write_all_at};
use crate::engine::probe::reserve_unique_path;
use crate::engine::ranges::{load_map, save_map, split_gaps, steal_point, DoneMap, Range};
use crate::engine::speed::{RateLimiter, SpeedCalculator};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
/// Received data is handed to the disk writer in pieces at least this big.
const WRITE_BATCH_BYTES: usize = 512 * 1024;
/// ...or after this long, so progress is never held back by a slow connection.
const WRITE_BATCH_MAX_AGE: Duration = Duration::from_millis(250);
const MAP_SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// A range gives up after this many failures in a row that made no progress.
const MAX_CONSECUTIVE_FAILURES: u32 = 6;
/// Connections opened immediately; the rest ramp up while the server cooperates.
const INITIAL_CONNECTIONS: usize = 8;
const RAMP_INTERVAL: Duration = Duration::from_millis(500);
const RAMP_STEP: usize = 2;
/// After a 429/503, don't open more connections for this long.
const THROTTLE_COOLDOWN: Duration = Duration::from_secs(15);
/// Never split off a range smaller than this.
const MIN_STEAL_BYTES: u64 = 256 * 1024;

#[derive(Clone)]
pub struct DownloadEngine {
    pool: Arc<ClientPool>,
    global_limit: Arc<RateLimiter>,
    retry_base: Duration,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    pub fn new() -> Self {
        Self {
            pool: Arc::new(ClientPool::new()),
            global_limit: Arc::new(RateLimiter::new(0)),
            retry_base: Duration::from_secs(1),
        }
    }

    /// The shared client pool (the manager uses it for probing too).
    pub fn clients(&self) -> Arc<ClientPool> {
        self.pool.clone()
    }

    /// Bandwidth cap shared by every running download, in bytes/s (0 = none).
    pub fn set_global_speed_limit(&self, bytes_per_sec: u64) {
        self.global_limit.set_rate(bytes_per_sec);
    }

    /// Base delay for retry backoff (tests use a tiny one).
    pub fn with_retry_base(mut self, base: Duration) -> Self {
        self.retry_base = base;
        self
    }

    /// Runs a download to completion, pause, or failure.
    ///
    /// * `Ok(task)` with `status == Completed` — the final file is in place.
    /// * `Ok(task)` with `status == Paused` — stopped on request; everything
    ///   received so far is on disk and a later call resumes from it.
    /// * `Err(_)` — the transfer failed; partial data is kept for a retry.
    pub async fn run_download<F>(
        &self,
        task: DownloadTask,
        pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        self.run_download_with(task, pause_rx, Vec::new(), on_progress).await
    }

    /// Like [`run_download`](Self::run_download), with extra request headers
    /// (for example from a saved site login). They are sent to the original
    /// host only, never to mirrors, and are merged under the task's own headers.
    pub async fn run_download_with<F>(
        &self,
        task: DownloadTask,
        pause_rx: watch::Receiver<bool>,
        extra_headers: Vec<(String, String)>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let headers = build_header_list(&task.custom_headers, &extra_headers);
        let dest_dir = Path::new(&task.destination_path);
        if !dest_dir.exists() {
            tokio::fs::create_dir_all(dest_dir)
                .await
                .map_err(|e| format!("Cannot create destination folder: {e}"))?;
        }

        let final_dest = dest_dir.join(&task.filename);
        let client = self
            .pool
            .download(task.proxy_mode.as_deref(), task.custom_proxy_url.as_deref());

        if task.resumable && task.total_bytes > 0 {
            self.run_ranged(task, final_dest, client, headers, pause_rx, on_progress).await
        } else {
            let part_file = dest_dir.join(format!("{}.nexpart", task.filename));
            self.run_single_stream(task, final_dest, part_file, client, headers, pause_rx, on_progress)
                .await
        }
    }

    // ───────────────────────── single stream (no ranges) ─────────────────────────

    async fn run_single_stream<F>(
        &self,
        mut task: DownloadTask,
        final_dest: PathBuf,
        part_file: PathBuf,
        client: reqwest::Client,
        headers: Vec<(HeaderName, HeaderValue)>,
        mut pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        if task.total_bytes > 0 {
            check_disk_space(final_dest.parent().unwrap_or(Path::new(".")), task.total_bytes as u64)?;
        }

        // Without range support a partial file can't be continued: start over.
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part_file)
            .await
            .map_err(|e| format!("Failed to create download file: {e}"))?;
        task.downloaded_bytes = 0;

        let ua = task.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let req = build_request(&client, &task.url, ua, task.referer.as_deref(), None, &headers);

        let res = match send_or_pause(req, &mut pause_rx).await? {
            Some(res) => res,
            None => return Ok(paused(task)),
        };
        if !res.status().is_success() {
            return Err(format!("Server returned HTTP status: {}", res.status()));
        }

        let mut stream = res.bytes_stream();
        let mut speed_calc = SpeedCalculator::new(2);
        let task_limit = RateLimiter::new(task.speed_limit);
        let mut last_progress_emit = Instant::now();

        loop {
            let next = tokio::select! {
                biased;
                _ = wait_paused(&mut pause_rx) => {
                    // Nothing can be resumed without range support, so a pause
                    // simply discards the partial file.
                    drop(file);
                    let _ = tokio::fs::remove_file(&part_file).await;
                    task.downloaded_bytes = 0;
                    return Ok(paused(task));
                }
                next = stream.next() => next,
            };

            let Some(chunk_res) = next else { break };
            let chunk = chunk_res.map_err(|e| format!("Network read error: {e}"))?;
            let chunk_len = chunk.len();

            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Disk write error: {e}"))?;

            task.downloaded_bytes += chunk_len as i64;
            speed_calc.record_bytes(chunk_len as u64);

            let limited = async {
                task_limit.acquire(chunk_len as u64).await;
                self.global_limit.acquire(chunk_len as u64).await;
            };
            tokio::select! {
                biased;
                _ = wait_paused(&mut pause_rx) => {
                    drop(file);
                    let _ = tokio::fs::remove_file(&part_file).await;
                    task.downloaded_bytes = 0;
                    return Ok(paused(task));
                }
                _ = limited => {}
            }

            if last_progress_emit.elapsed() >= PROGRESS_INTERVAL {
                let speed_bps = speed_calc.current_speed_bps();
                let remaining = (task.total_bytes - task.downloaded_bytes).max(0) as u64;
                let eta = SpeedCalculator::calculate_eta(remaining, speed_bps);
                task.speed_bps = speed_bps;
                task.eta_seconds = eta;

                on_progress(DownloadProgressEvent {
                    id: task.id.clone(),
                    downloaded_bytes: task.downloaded_bytes,
                    total_bytes: task.total_bytes,
                    speed_bps,
                    eta_seconds: eta,
                    status: DownloadStatus::Downloading,
                    segments: vec![DownloadSegment {
                        segment_index: 0,
                        start_byte: 0,
                        end_byte: if task.total_bytes > 0 { (task.total_bytes - 1) as u64 } else { 0 },
                        downloaded_bytes: task.downloaded_bytes.max(0) as u64,
                        total_bytes: task.total_bytes.max(0) as u64,
                        status: "downloading".to_string(),
                    }],
                });
                last_progress_emit = Instant::now();
            }
        }

        file.flush().await.map_err(|e| format!("Failed to flush file: {e}"))?;
        drop(file);

        // A connection that closes early must not be mistaken for a finished file.
        if task.total_bytes > 0 && task.downloaded_bytes < task.total_bytes {
            return Err(format!(
                "Connection closed early ({} of {} bytes received)",
                task.downloaded_bytes, task.total_bytes
            ));
        }

        let final_path = reserve_unique_path(final_dest.parent().unwrap_or(Path::new(".")), &file_name_of(&final_dest))
            .map_err(|e| format!("Cannot create the final file: {e}"))?;
        if let Err(e) = tokio::fs::rename(&part_file, &final_path).await {
            let _ = tokio::fs::remove_file(&final_path).await;
            return Err(format!("Failed to rename completed file: {e}"));
        }

        finish_task(&mut task, &final_path);
        task.downloaded_bytes = task.downloaded_bytes.max(task.total_bytes);

        on_progress(DownloadProgressEvent {
            id: task.id.clone(),
            downloaded_bytes: task.downloaded_bytes,
            total_bytes: task.total_bytes,
            speed_bps: 0,
            eta_seconds: None,
            status: DownloadStatus::Completed,
            segments: vec![],
        });

        Ok(task)
    }

    // ───────────────────────────── range engine ─────────────────────────────

    async fn run_ranged<F>(
        &self,
        mut task: DownloadTask,
        final_dest: PathBuf,
        client: reqwest::Client,
        headers: Vec<(HeaderName, HeaderValue)>,
        mut pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        // If the server reveals that the file changed since we started, the
        // partial data is worthless: restart once from scratch.
        let mut fresh = false;
        for round in 0..2 {
            match self
                .ranged_round(&mut task, &final_dest, &client, &headers, &mut pause_rx, &on_progress, fresh)
                .await
            {
                Ok(()) => return Ok(task),
                Err(RoundFailure::FileChanged) if round == 0 => fresh = true,
                Err(RoundFailure::FileChanged) => {
                    return Err("The file on the server changed while it was being downloaded".to_string())
                }
                Err(RoundFailure::Failed(msg)) => return Err(msg),
            }
        }
        Err("Download failed".to_string())
    }

    async fn ranged_round<F>(
        &self,
        task: &mut DownloadTask,
        final_dest: &Path,
        client: &reqwest::Client,
        headers: &[(HeaderName, HeaderValue)],
        pause_rx: &mut watch::Receiver<bool>,
        on_progress: &F,
        force_fresh: bool,
    ) -> Result<(), RoundFailure>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let dest_dir = final_dest.parent().unwrap_or(Path::new(".")).to_path_buf();
        let total = task.total_bytes as u64;
        let part_path = dest_dir.join(format!("{}.nexpart", task.filename));
        let map_path = dest_dir.join(format!("{}.nexpart.map", task.filename));

        if force_fresh {
            let _ = tokio::fs::remove_file(&part_path).await;
            let _ = tokio::fs::remove_file(&map_path).await;
        }

        // Open / preallocate the target (blocking calls, so off the async threads).
        let open_path = part_path.clone();
        let (file, fresh) = tokio::task::spawn_blocking(move || open_preallocated(&open_path, total))
            .await
            .map_err(|e| RoundFailure::Failed(format!("Internal error: {e}")))?
            .map_err(|e| RoundFailure::Failed(format!("Cannot prepare download file: {e}")))?;
        let file = Arc::new(file);

        let done = if fresh {
            let _ = tokio::fs::remove_file(&map_path).await;
            DoneMap::default()
        } else {
            load_map(&map_path, total).unwrap_or_default()
        };
        let already = done.covered();
        let resumed = already > 0;

        // Fail early and clearly instead of hitting "disk full" halfway through.
        check_disk_space(&dest_dir, total.saturating_sub(already)).map_err(RoundFailure::Failed)?;

        let connections = task.connections.clamp(1, 32);
        let min_piece = task.min_chunk_size.max(MIN_STEAL_BYTES);
        let todo: VecDeque<Range> = split_gaps(&done.gaps(total), connections, min_piece).into();
        let initial_workers = connections.min(INITIAL_CONNECTIONS).min(todo.len().max(1));

        let mut urls = vec![task.url.clone()];
        urls.extend(task.mirrors.iter().filter(|m| !m.trim().is_empty()).cloned());

        let done = Arc::new(Mutex::new(done));
        let (writer_tx, writer_rx) = mpsc::channel::<WriterMsg>(32);
        let (cancel_tx, _) = watch::channel(false);
        let writer = spawn_writer(file.clone(), done.clone(), writer_rx);

        let shared = Arc::new(Shared {
            client: client.clone(),
            urls,
            user_agent: task.user_agent.clone().unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
            referer: task.referer.clone(),
            headers: headers.to_vec(),
            validator: resume_validator(task.etag.as_deref(), task.last_modified.as_deref()),
            resumed,
            total,
            min_steal: min_piece,
            done: done.clone(),
            plan: Mutex::new(Plan { todo, slots: Vec::new() }),
            received: AtomicU64::new(already),
            writer_tx,
            task_limit: RateLimiter::new(task.speed_limit),
            global_limit: self.global_limit.clone(),
            cancel: cancel_tx,
            fatal: Mutex::new(None),
            range_ok: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            cap: AtomicUsize::new(connections),
            next_slot_id: AtomicUsize::new(0),
            clock: Instant::now(),
            last_throttle_ms: AtomicU64::new(u64::MAX),
            retry_base: self.retry_base,
        });

        let mut workers: JoinSet<()> = JoinSet::new();
        for _ in 0..initial_workers {
            workers.spawn(worker(shared.clone()));
        }

        // ── coordinator ────────────────────────────────────────────────────
        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_tick = Instant::now();
        let mut last_received = already;
        let mut smoothed_bps = 0.0f64;
        let mut last_ramp = Instant::now();
        let mut last_save = Instant::now();
        let mut save_inflight: Option<tokio::task::JoinHandle<()>> = None;

        enum End {
            Done,
            Paused,
            Failed(RoundFailure),
        }

        let end = loop {
            tokio::select! {
                biased;
                _ = wait_paused(pause_rx) => break End::Paused,
                _ = workers.join_next(), if !workers.is_empty() => {}
                _ = ticker.tick() => {
                    let now = Instant::now();
                    let received = shared.received.load(Ordering::Relaxed);
                    let dt = now.duration_since(last_tick).as_secs_f64().max(0.001);
                    let instant_bps = received.saturating_sub(last_received) as f64 / dt;
                    smoothed_bps = if smoothed_bps == 0.0 { instant_bps } else { 0.35 * instant_bps + 0.65 * smoothed_bps };
                    last_tick = now;
                    last_received = received;

                    let speed_bps = smoothed_bps as u64;
                    let eta = SpeedCalculator::calculate_eta(total.saturating_sub(received), speed_bps);
                    task.downloaded_bytes = received.min(total) as i64;
                    task.speed_bps = speed_bps;
                    task.eta_seconds = eta;

                    on_progress(DownloadProgressEvent {
                        id: task.id.clone(),
                        downloaded_bytes: task.downloaded_bytes,
                        total_bytes: task.total_bytes,
                        speed_bps,
                        eta_seconds: eta,
                        status: DownloadStatus::Downloading,
                        segments: shared.snapshot_segments(),
                    });

                    // Persist what's safely on disk (the sidecar is what resume reads).
                    if now.duration_since(last_save) >= MAP_SAVE_INTERVAL
                        && save_inflight.as_ref().map(|h| h.is_finished()).unwrap_or(true)
                    {
                        last_save = now;
                        let snapshot = done.lock().clone();
                        let (f, p) = (file.clone(), map_path.clone());
                        save_inflight = Some(tokio::task::spawn_blocking(move || {
                            let _ = f.sync_data();
                            let _ = save_map(&p, total, &snapshot);
                        }));
                    }

                    // Ramp up: add connections while there is work to share and
                    // the server hasn't pushed back recently.
                    if now.duration_since(last_ramp) >= RAMP_INTERVAL
                        && shared.range_ok.load(Ordering::Relaxed)
                        && !shared.recently_throttled()
                    {
                        last_ramp = now;
                        let room = shared.cap.load(Ordering::Relaxed).saturating_sub(workers.len());
                        for _ in 0..room.min(RAMP_STEP) {
                            if !shared.can_feed_another_worker() {
                                break;
                            }
                            workers.spawn(worker(shared.clone()));
                        }
                    }
                }
            }

            if let Some(failure) = shared.fatal.lock().clone() {
                break End::Failed(failure);
            }
            if workers.is_empty() {
                if shared.has_unfinished_work() {
                    // Every worker left (e.g. gave ranges back after throttling): keep one going.
                    workers.spawn(worker(shared.clone()));
                } else {
                    break End::Done;
                }
            }
        };

        // ── wind down ──────────────────────────────────────────────────────
        if !matches!(end, End::Done) {
            let _ = shared.cancel.send_replace(true);
        }
        let drained = tokio::time::timeout(Duration::from_secs(10), async {
            while workers.join_next().await.is_some() {}
        })
        .await;
        if drained.is_err() {
            workers.abort_all();
        }
        if let Some(handle) = save_inflight.take() {
            let _ = handle.await;
        }

        let _ = shared.writer_tx.send(WriterMsg::Stop).await;
        let writer_result = tokio::task::spawn_blocking(move || writer.join())
            .await
            .map_err(|e| RoundFailure::Failed(format!("Internal error: {e}")))?;
        let writer_error = match writer_result {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(format!("Disk write error: {e}")),
            Err(_) => Some("Disk writer stopped unexpectedly".to_string()),
        };

        // Final durable snapshot of progress.
        let snapshot = done.lock().clone();
        {
            let (f, p, snap) = (file.clone(), map_path.clone(), snapshot.clone());
            let _ = tokio::task::spawn_blocking(move || {
                let _ = f.sync_data();
                let _ = save_map(&p, total, &snap);
            })
            .await;
        }
        let covered = snapshot.covered();
        task.segments = segments_from_done(&snapshot);
        task.downloaded_bytes = covered.min(total) as i64;
        task.speed_bps = 0;
        task.eta_seconds = None;

        if let Some(msg) = writer_error {
            return Err(RoundFailure::Failed(msg));
        }

        match end {
            End::Failed(failure) => Err(failure),
            End::Paused => {
                task.status = DownloadStatus::Paused;
                Ok(())
            }
            End::Done => {
                if !snapshot.is_complete(total) {
                    return Err(RoundFailure::Failed(format!(
                        "Download incomplete: {} bytes are missing",
                        total.saturating_sub(covered)
                    )));
                }
                drop(shared);
                drop(file);

                let final_path = reserve_unique_path(&dest_dir, &file_name_of(final_dest))
                    .map_err(|e| RoundFailure::Failed(format!("Cannot create the final file: {e}")))?;
                if let Err(e) = tokio::fs::rename(&part_path, &final_path).await {
                    let _ = tokio::fs::remove_file(&final_path).await;
                    return Err(RoundFailure::Failed(format!("Failed to move finished file into place: {e}")));
                }
                let _ = tokio::fs::remove_file(&map_path).await;

                finish_task(task, &final_path);
                task.downloaded_bytes = task.total_bytes;
                task.segments = vec![DownloadSegment {
                    segment_index: 0,
                    start_byte: 0,
                    end_byte: total.saturating_sub(1),
                    downloaded_bytes: total,
                    total_bytes: total,
                    status: "completed".to_string(),
                }];

                on_progress(DownloadProgressEvent {
                    id: task.id.clone(),
                    downloaded_bytes: task.total_bytes,
                    total_bytes: task.total_bytes,
                    speed_bps: 0,
                    eta_seconds: None,
                    status: DownloadStatus::Completed,
                    segments: task.segments.clone(),
                });
                Ok(())
            }
        }
    }
}

// ───────────────────────────── shared state ─────────────────────────────

#[derive(Clone)]
enum RoundFailure {
    /// The server's file no longer matches what we started downloading.
    FileChanged,
    Failed(String),
}

enum WriterMsg {
    Write { offset: u64, data: Vec<u8> },
    Stop,
}

#[derive(Clone, Copy)]
struct SlotState {
    pos: u64,
    end: u64,
}

/// One in-flight range. `end` can shrink when another connection steals the
/// tail, so `pos`/`end` are only ever touched together under the lock.
struct Slot {
    id: usize,
    start: u64,
    url_idx: AtomicUsize,
    state: Mutex<SlotState>,
}

struct Plan {
    todo: VecDeque<Range>,
    slots: Vec<Arc<Slot>>,
}

struct Shared {
    client: reqwest::Client,
    urls: Vec<String>,
    user_agent: String,
    referer: Option<String>,
    /// Extra headers (cookies, auth). Only ever sent to the original URL.
    headers: Vec<(HeaderName, HeaderValue)>,
    /// ETag / Last-Modified sent as `If-Range` when resuming.
    validator: Option<String>,
    resumed: bool,
    total: u64,
    min_steal: u64,
    plan: Mutex<Plan>,
    /// Ranges confirmed on disk (shared with the writer thread).
    done: Arc<Mutex<DoneMap>>,
    /// Bytes accepted from the network (including data not yet flushed).
    received: AtomicU64,
    writer_tx: mpsc::Sender<WriterMsg>,
    task_limit: RateLimiter,
    global_limit: Arc<RateLimiter>,
    cancel: watch::Sender<bool>,
    fatal: Mutex<Option<RoundFailure>>,
    /// Set once the server has answered a range request with 206.
    range_ok: AtomicBool,
    active: AtomicUsize,
    /// Maximum simultaneous connections (lowered when the server pushes back).
    cap: AtomicUsize,
    next_slot_id: AtomicUsize,
    clock: Instant,
    last_throttle_ms: AtomicU64,
    retry_base: Duration,
}

impl Shared {
    fn fail(&self, failure: RoundFailure) {
        let mut fatal = self.fatal.lock();
        if fatal.is_none() {
            *fatal = Some(failure);
        }
        let _ = self.cancel.send_replace(true);
    }

    fn note_throttle(&self) {
        self.last_throttle_ms
            .store(self.clock.elapsed().as_millis() as u64, Ordering::Relaxed);
        let _ = self
            .cap
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |c| Some(c.saturating_sub(1).max(1)));
    }

    fn recently_throttled(&self) -> bool {
        let at = self.last_throttle_ms.load(Ordering::Relaxed);
        at != u64::MAX && (self.clock.elapsed().as_millis() as u64).saturating_sub(at) < THROTTLE_COOLDOWN.as_millis() as u64
    }

    /// Hands the next piece of work to a worker: queued ranges first, otherwise
    /// the upper half of the biggest range that's still being downloaded.
    fn take_range(&self) -> Option<Arc<Slot>> {
        let mut plan = self.plan.lock();
        let id = self.next_slot_id.fetch_add(1, Ordering::Relaxed);

        let range = if let Some(range) = plan.todo.pop_front() {
            Some(range)
        } else if self.range_ok.load(Ordering::Relaxed) {
            // Try the victims with the most data left first.
            let mut order: Vec<(u64, usize)> = plan
                .slots
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let st = s.state.lock();
                    (st.end.saturating_sub(st.pos), i)
                })
                .collect();
            order.sort_unstable_by(|a, b| b.cmp(a));

            let mut stolen = None;
            for (_, i) in order {
                let mut st = plan.slots[i].state.lock();
                if let Some(mid) = steal_point(st.pos, st.end, self.min_steal) {
                    let old_end = st.end;
                    st.end = mid;
                    stolen = Some((mid, old_end));
                    break;
                }
            }
            stolen
        } else {
            None
        };

        let (start, end) = range?;
        let slot = Arc::new(Slot {
            id,
            start,
            url_idx: AtomicUsize::new(id % self.urls.len()),
            state: Mutex::new(SlotState { pos: start, end }),
        });
        plan.slots.push(slot.clone());
        Some(slot)
    }

    fn finish_slot(&self, slot: &Slot) {
        self.plan.lock().slots.retain(|s| s.id != slot.id);
    }

    /// Puts whatever is left of `slot` back in the queue for another worker.
    fn give_back(&self, slot: &Slot) {
        let mut plan = self.plan.lock();
        plan.slots.retain(|s| s.id != slot.id);
        let st = *slot.state.lock();
        if st.pos < st.end {
            plan.todo.push_front((st.pos, st.end));
        }
    }

    fn has_unfinished_work(&self) -> bool {
        let plan = self.plan.lock();
        !plan.todo.is_empty() || !plan.slots.is_empty()
    }

    /// Is there anything a brand-new worker could start on right now?
    fn can_feed_another_worker(&self) -> bool {
        let plan = self.plan.lock();
        if !plan.todo.is_empty() {
            return true;
        }
        plan.slots.iter().any(|s| {
            let st = s.state.lock();
            steal_point(st.pos, st.end, self.min_steal).is_some()
        })
    }

    /// Finished ranges plus the ranges currently in flight, for the UI's byte map.
    fn snapshot_segments(&self) -> Vec<DownloadSegment> {
        let mut segments = segments_from_done(&self.done.lock());
        let base = segments.len();
        let plan = self.plan.lock();
        for (i, s) in plan.slots.iter().enumerate() {
            let st = *s.state.lock();
            segments.push(DownloadSegment {
                segment_index: base + i,
                start_byte: s.start,
                end_byte: st.end.saturating_sub(1),
                downloaded_bytes: st.pos.saturating_sub(s.start),
                total_bytes: st.end.saturating_sub(s.start),
                status: "downloading".to_string(),
            });
        }
        segments
    }
}

fn segments_from_done(done: &DoneMap) -> Vec<DownloadSegment> {
    done.ranges()
        .iter()
        .enumerate()
        .map(|(i, &(s, e))| DownloadSegment {
            segment_index: i,
            start_byte: s,
            end_byte: e - 1,
            downloaded_bytes: e - s,
            total_bytes: e - s,
            status: "completed".to_string(),
        })
        .collect()
}

fn spawn_writer(
    file: Arc<std::fs::File>,
    done: Arc<Mutex<DoneMap>>,
    mut rx: mpsc::Receiver<WriterMsg>,
) -> std::thread::JoinHandle<std::io::Result<()>> {
    std::thread::Builder::new()
        .name("nexdrop-writer".to_string())
        .spawn(move || {
            while let Some(msg) = rx.blocking_recv() {
                match msg {
                    WriterMsg::Stop => break,
                    WriterMsg::Write { offset, data } => {
                        write_all_at(&file, &data, offset)?;
                        done.lock().insert(offset, offset + data.len() as u64);
                    }
                }
            }
            file.sync_data()
        })
        .expect("failed to start disk writer thread")
}

// ──────────────────────────────── workers ────────────────────────────────

struct WriteBuffer {
    offset: u64,
    data: Vec<u8>,
    since: Instant,
}

impl WriteBuffer {
    fn new() -> Self {
        Self { offset: 0, data: Vec::with_capacity(WRITE_BATCH_BYTES + 64 * 1024), since: Instant::now() }
    }
    fn push(&mut self, offset: u64, bytes: &[u8]) {
        if self.data.is_empty() {
            self.offset = offset;
            self.since = Instant::now();
        }
        debug_assert_eq!(self.offset + self.data.len() as u64, offset, "write buffer must stay contiguous");
        self.data.extend_from_slice(bytes);
    }
    fn due(&self) -> bool {
        self.data.len() >= WRITE_BATCH_BYTES || (!self.data.is_empty() && self.since.elapsed() >= WRITE_BATCH_MAX_AGE)
    }
    fn take(&mut self) -> Option<WriterMsg> {
        if self.data.is_empty() {
            return None;
        }
        let data = std::mem::replace(&mut self.data, Vec::with_capacity(WRITE_BATCH_BYTES + 64 * 1024));
        Some(WriterMsg::Write { offset: self.offset, data })
    }
}

async fn flush(shared: &Shared, buf: &mut WriteBuffer) -> bool {
    match buf.take() {
        None => true,
        Some(msg) => shared.writer_tx.send(msg).await.is_ok(),
    }
}

enum SlotEnd {
    Done,
    Cancelled,
    /// Throttled: this connection leaves and its range goes back to the queue.
    GiveBack,
    Fatal(RoundFailure),
}

enum AttemptEnd {
    Complete,
    Cancelled,
    Fatal(RoundFailure),
    Retry {
        reason: String,
        made_progress: bool,
        retry_after: Option<Duration>,
        throttled: bool,
    },
}

fn retry(reason: String, made_progress: bool) -> AttemptEnd {
    AttemptEnd::Retry { reason, made_progress, retry_after: None, throttled: false }
}

struct ActiveGuard(Arc<Shared>);
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn worker(shared: Arc<Shared>) {
    shared.active.fetch_add(1, Ordering::Relaxed);
    let _active = ActiveGuard(shared.clone());
    let mut cancel = shared.cancel.subscribe();
    let mut buf = WriteBuffer::new();

    while let Some(slot) = shared.take_range() {
        let outcome = run_slot(&shared, &slot, &mut buf, &mut cancel).await;
        // Whatever was received is handed to the writer in every case, so a
        // pause or a failure never throws away data that has already arrived.
        if !flush(&shared, &mut buf).await && !matches!(outcome, SlotEnd::Cancelled) {
            shared.fail(RoundFailure::Failed("Disk write failed".to_string()));
            return;
        }
        match outcome {
            SlotEnd::Done => shared.finish_slot(&slot),
            SlotEnd::Cancelled => return,
            SlotEnd::GiveBack => {
                shared.give_back(&slot);
                return;
            }
            SlotEnd::Fatal(failure) => {
                shared.fail(failure);
                return;
            }
        }
    }
}

async fn run_slot(
    shared: &Arc<Shared>,
    slot: &Arc<Slot>,
    buf: &mut WriteBuffer,
    cancel: &mut watch::Receiver<bool>,
) -> SlotEnd {
    let mut failures = 0u32;

    loop {
        match attempt(shared, slot, buf, cancel).await {
            AttemptEnd::Complete => return SlotEnd::Done,
            AttemptEnd::Cancelled => return SlotEnd::Cancelled,
            AttemptEnd::Fatal(failure) => {
                // A bad mirror shouldn't sink the download: fall back to the original URL.
                if matches!(failure, RoundFailure::Failed(_)) && slot.url_idx.load(Ordering::Relaxed) != 0 {
                    slot.url_idx.store(0, Ordering::Relaxed);
                    failures += 1;
                    if failures > MAX_CONSECUTIVE_FAILURES {
                        return SlotEnd::Fatal(failure);
                    }
                    continue;
                }
                return SlotEnd::Fatal(failure);
            }
            AttemptEnd::Retry { reason, made_progress, retry_after, throttled } => {
                if made_progress {
                    failures = 0;
                }
                failures += 1;

                if throttled {
                    shared.note_throttle();
                    // Fewer connections: this one steps aside if others are running.
                    if shared.active.load(Ordering::Relaxed) > 1 {
                        return SlotEnd::GiveBack;
                    }
                }
                if failures > MAX_CONSECUTIVE_FAILURES {
                    return SlotEnd::Fatal(RoundFailure::Failed(format!(
                        "{reason} (gave up after {MAX_CONSECUTIVE_FAILURES} retries)"
                    )));
                }

                // Flush before sleeping so a long wait doesn't hold data back.
                if !flush(shared, buf).await {
                    return SlotEnd::Fatal(RoundFailure::Failed("Disk write failed".to_string()));
                }
                let delay = backoff_delay(shared.retry_base, failures, retry_after);
                tokio::select! {
                    biased;
                    _ = wait_paused(cancel) => return SlotEnd::Cancelled,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }
    }
}

/// One request/response cycle for a slot, resuming from wherever it got to.
async fn attempt(
    shared: &Arc<Shared>,
    slot: &Arc<Slot>,
    buf: &mut WriteBuffer,
    cancel: &mut watch::Receiver<bool>,
) -> AttemptEnd {
    let st = *slot.state.lock();
    if st.pos >= st.end {
        return AttemptEnd::Complete;
    }

    let url_idx = slot.url_idx.load(Ordering::Relaxed) % shared.urls.len();
    let mut req = shared
        .client
        .get(&shared.urls[url_idx])
        .header(USER_AGENT, &shared.user_agent)
        .header(RANGE, format!("bytes={}-{}", st.pos, st.end - 1));
    if let Some(referer) = &shared.referer {
        req = req.header("Referer", referer);
    }
    // Credentials and cookies belong to the original host; a mirror must never see them.
    if url_idx == 0 {
        for (name, value) in &shared.headers {
            req = req.header(name.clone(), value.clone());
        }
    }
    // When resuming, ask the server to answer with the *same* file or not at all.
    let sent_if_range = shared.resumed && url_idx == 0 && shared.validator.is_some();
    if sent_if_range {
        if let Some(v) = &shared.validator {
            req = req.header("If-Range", v);
        }
    }

    let resp = tokio::select! {
        biased;
        _ = wait_paused(cancel) => return AttemptEnd::Cancelled,
        r = req.send() => r,
    };
    let resp = match resp {
        Ok(r) => r,
        Err(e) => return retry(format!("Request failed: {e}"), false),
    };

    let status = resp.status();
    if status == StatusCode::PARTIAL_CONTENT {
        shared.range_ok.store(true, Ordering::Relaxed);
        if let Some((first, total)) = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range)
        {
            if first != st.pos {
                return AttemptEnd::Fatal(RoundFailure::Failed(format!(
                    "Server returned bytes starting at {first} but {} was requested",
                    st.pos
                )));
            }
            if let Some(total) = total {
                if total != shared.total {
                    return if url_idx == 0 {
                        AttemptEnd::Fatal(RoundFailure::FileChanged)
                    } else {
                        // This mirror serves a different file; use the original instead.
                        slot.url_idx.store(0, Ordering::Relaxed);
                        retry("Mirror serves a different file".to_string(), false)
                    };
                }
            }
        }
    } else if status == StatusCode::OK {
        if st.pos == 0 && st.end >= shared.total {
            // The whole file was requested and the whole file is coming: fine.
        } else if sent_if_range {
            // A full answer to a conditional range request means the file changed.
            return AttemptEnd::Fatal(RoundFailure::FileChanged);
        } else {
            return AttemptEnd::Fatal(RoundFailure::Failed(
                "Server ignored the Range request (HTTP 200), so this file can't be downloaded in parts. \
                 Set connections to 1 and retry."
                    .to_string(),
            ));
        }
    } else if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        return AttemptEnd::Retry {
            reason: format!("Server is busy (HTTP {status})"),
            made_progress: false,
            retry_after,
            throttled: true,
        };
    } else if status == StatusCode::REQUEST_TIMEOUT || status.is_server_error() {
        return retry(format!("Server error (HTTP {status})"), false);
    } else {
        return AttemptEnd::Fatal(RoundFailure::Failed(format!("HTTP error: {status}")));
    }

    let mut stream = resp.bytes_stream();
    let mut made_progress = false;
    let mut flush_tick = tokio::time::interval(WRITE_BATCH_MAX_AGE);
    flush_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        let next = tokio::select! {
            biased;
            _ = wait_paused(cancel) => return AttemptEnd::Cancelled,
            _ = flush_tick.tick() => {
                if buf.due() && !flush(shared, buf).await {
                    return AttemptEnd::Fatal(RoundFailure::Failed("Disk write failed".to_string()));
                }
                continue;
            }
            next = stream.next() => next,
        };

        let chunk = match next {
            None => break,
            Some(Err(e)) => return retry(format!("Connection error: {e}"), made_progress),
            Some(Ok(chunk)) => chunk,
        };

        // Reserve the bytes under the slot lock: this is what makes work
        // stealing exact, because `end` may have been lowered meanwhile.
        let (take, offset, finished) = {
            let mut s = slot.state.lock();
            let take = (chunk.len() as u64).min(s.end.saturating_sub(s.pos));
            let offset = s.pos;
            s.pos += take;
            (take, offset, s.pos >= s.end)
        };
        if take == 0 {
            return AttemptEnd::Complete;
        }

        buf.push(offset, &chunk[..take as usize]);
        shared.received.fetch_add(take, Ordering::Relaxed);
        made_progress = true;

        // Bandwidth limits (the wait is interruptible so Pause stays instant).
        let limited = async {
            shared.task_limit.acquire(take).await;
            shared.global_limit.acquire(take).await;
        };
        tokio::select! {
            biased;
            _ = wait_paused(cancel) => return AttemptEnd::Cancelled,
            _ = limited => {}
        }

        if (buf.due() || finished) && !flush(shared, buf).await {
            return AttemptEnd::Fatal(RoundFailure::Failed("Disk write failed".to_string()));
        }
        if finished {
            return AttemptEnd::Complete;
        }
    }

    let st = *slot.state.lock();
    if st.pos >= st.end {
        AttemptEnd::Complete
    } else {
        retry(
            format!("Connection closed early ({} bytes short)", st.end - st.pos),
            made_progress,
        )
    }
}

// ─────────────────────────────── helpers ───────────────────────────────

/// 1s, 2s, 4s ... capped at 30s, ±25% jitter; never shorter than a server's `Retry-After`.
fn backoff_delay(base: Duration, failures: u32, retry_after: Option<Duration>) -> Duration {
    let exp = failures.saturating_sub(1).min(5);
    let raw = base.saturating_mul(1u32 << exp).min(base.saturating_mul(30));
    let unit = (std::collections::hash_map::RandomState::new().build_hasher().finish() % 1000) as f64 / 1000.0;
    let jittered = raw.mul_f64(0.75 + 0.5 * unit);
    match retry_after {
        Some(ra) => jittered.max(ra.min(Duration::from_secs(60))),
        None => jittered,
    }
}

/// `bytes 100-199/1000` → (100, Some(1000)); `bytes 0-9/*` → (0, None)
fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let first = range.split('-').next()?.trim().parse().ok()?;
    Some((first, total.trim().parse().ok()))
}

/// A strong validator for `If-Range` (weak ETags aren't allowed there).
fn resume_validator(etag: Option<&str>, last_modified: Option<&str>) -> Option<String> {
    match etag {
        Some(etag) if !etag.starts_with("W/") && !etag.is_empty() => Some(etag.to_string()),
        _ => last_modified.filter(|m| !m.is_empty()).map(str::to_string),
    }
}

fn build_request(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
    referer: Option<&str>,
    range: Option<String>,
    headers: &[(HeaderName, HeaderValue)],
) -> reqwest::RequestBuilder {
    let mut req = client.get(url).header(USER_AGENT, user_agent);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    for (name, value) in headers {
        req = req.header(name.clone(), value.clone());
    }
    if let Some(range) = range {
        req = req.header(RANGE, range);
    }
    req
}

/// Resolves when the watched flag becomes `true`. If the sender goes away the
/// flag can never change again, so this then waits forever (never "pauses").
pub(crate) async fn wait_paused(rx: &mut watch::Receiver<bool>) {
    if rx.wait_for(|paused| *paused).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// Sends the request, but gives up early (returning `None`) if paused first.
async fn send_or_pause(
    req: reqwest::RequestBuilder,
    pause_rx: &mut watch::Receiver<bool>,
) -> Result<Option<reqwest::Response>, String> {
    tokio::select! {
        biased;
        _ = wait_paused(pause_rx) => Ok(None),
        res = req.send() => res.map(Some).map_err(|e| format!("HTTP request failed: {e}")),
    }
}

/// Room to spare beyond the file itself (temp files, other programs writing).
const DISK_SPACE_MARGIN: u64 = 32 * 1024 * 1024;

fn check_disk_space(dir: &Path, needed: u64) -> Result<(), String> {
    match free_space(dir) {
        Some(free) if free < needed.saturating_add(DISK_SPACE_MARGIN) => Err(format!(
            "Not enough free disk space: {} needed, {} available",
            human_size(needed),
            human_size(free)
        )),
        _ => Ok(()),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

fn paused(mut task: DownloadTask) -> DownloadTask {
    task.status = DownloadStatus::Paused;
    task.speed_bps = 0;
    task.eta_seconds = None;
    task
}

fn finish_task(task: &mut DownloadTask, final_path: &Path) {
    task.destination_path = final_path
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    task.filename = file_name_of(final_path);
    task.status = DownloadStatus::Completed;
    task.completed_at = Some(chrono::Utc::now().timestamp());
    task.speed_bps = 0;
    task.eta_seconds = None;
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download.bin".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_range() {
        assert_eq!(parse_content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(parse_content_range("bytes 0-0/10"), Some((0, Some(10))));
        assert_eq!(parse_content_range("bytes 5-9/*"), Some((5, None)));
        assert_eq!(parse_content_range("bytes */1000"), None);
        assert_eq!(parse_content_range("garbage"), None);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        let base = Duration::from_secs(1);
        let d1 = backoff_delay(base, 1, None);
        let d4 = backoff_delay(base, 4, None);
        let d20 = backoff_delay(base, 20, None);
        assert!(d1 >= Duration::from_millis(750) && d1 <= Duration::from_millis(1250), "{d1:?}");
        assert!(d4 >= Duration::from_secs(6) && d4 <= Duration::from_secs(10), "{d4:?}");
        assert!(d20 <= Duration::from_millis(37_500), "{d20:?}");
        // Retry-After wins when it's longer.
        assert!(backoff_delay(base, 1, Some(Duration::from_secs(20))) >= Duration::from_secs(20));
    }

    #[test]
    fn formats_sizes_for_error_messages() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    }

    #[test]
    fn disk_space_check_rejects_impossible_sizes_but_not_small_ones() {
        let dir = std::env::temp_dir();
        assert!(check_disk_space(&dir, 1024).is_ok());
        if crate::engine::fileio::free_space(&dir).is_some() {
            assert!(check_disk_space(&dir, u64::MAX / 2).unwrap_err().contains("Not enough free disk space"));
        }
    }

    #[test]
    fn weak_etags_are_not_used_for_if_range() {
        let lm = Some("Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(resume_validator(Some("W/\"abc\""), lm).as_deref(), lm);
        assert_eq!(resume_validator(Some("\"strong\""), lm).as_deref(), Some("\"strong\""));
        assert_eq!(resume_validator(None, None), None);
    }
}
