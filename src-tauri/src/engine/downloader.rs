use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, RANGE, USER_AGENT};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch};

use crate::db::models::{DownloadProgressEvent, DownloadSegment, DownloadStatus, DownloadTask};
use crate::engine::probe::resolve_unique_path;
use crate::engine::speed::{SpeedCalculator, SpeedLimiter};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0";
/// How long a connection may sit idle (no bytes received) before it is dropped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

pub enum WorkerMessage {
    /// The worker inspected its segment file; `already` bytes are safely on disk.
    Started { segment_index: usize, already: u64 },
    /// Absolute number of bytes now stored for the segment. Lossy by design:
    /// a newer value always supersedes an older one.
    Progress { segment_index: usize, bytes_downloaded: u64 },
    Finished { segment_index: usize },
    Failed { segment_index: usize, error: String },
}

#[derive(Clone)]
pub struct DownloadEngine {
    client: reqwest::Client,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadEngine {
    pub fn new() -> Self {
        let client = crate::engine::create_download_client_with_proxy(IDLE_TIMEOUT, None, None);
        Self { client }
    }

    /// Runs a download to completion, pause, or failure.
    ///
    /// * `Ok(task)` with `status == Completed` — the final file is in place.
    /// * `Ok(task)` with `status == Paused` — stopped on request; every byte
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
        let dest_dir = Path::new(&task.destination_path);
        if !dest_dir.exists() {
            tokio::fs::create_dir_all(dest_dir)
                .await
                .map_err(|e| format!("Cannot create destination folder: {e}"))?;
        }

        let final_dest = dest_dir.join(&task.filename);
        let part_file = dest_dir.join(format!("{}.nexpart", task.filename));

        let client = if task.proxy_mode.is_some() || task.custom_proxy_url.is_some() {
            crate::engine::client::create_download_client_with_proxy(
                IDLE_TIMEOUT,
                task.proxy_mode.as_deref().or(Some("system")),
                task.custom_proxy_url.as_deref(),
            )
        } else {
            self.client.clone()
        };

        // Fallback or 1-connection download
        if !task.resumable || task.total_bytes <= 0 || task.connections <= 1 || task.segments.len() <= 1 {
            return self
                .run_single_stream(task, final_dest, part_file, client, pause_rx, on_progress)
                .await;
        }

        self.run_segmented_download(task, final_dest, client, pause_rx, on_progress)
            .await
    }

    async fn run_single_stream<F>(
        &self,
        mut task: DownloadTask,
        final_dest: PathBuf,
        part_file: PathBuf,
        client: reqwest::Client,
        mut pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&part_file)
            .await
            .map_err(|e| format!("Failed to create download file: {e}"))?;

        let existing = file.metadata().await.map(|m| m.len()).unwrap_or(0);
        // Only continue a partial file if the server can actually resume.
        let mut offset = if task.resumable { existing } else { 0 };

        let ua = task.user_agent.as_deref().unwrap_or(DEFAULT_USER_AGENT);
        let range = (offset > 0).then(|| format!("bytes={offset}-"));
        let req = build_request(&client, &task.url, ua, task.referer.as_deref(), range);

        let res = match send_or_pause(req, &mut pause_rx).await? {
            Some(res) => res,
            None => return Ok(paused(task)),
        };
        let status = res.status();

        if !status.is_success() {
            return Err(format!("Server returned HTTP status: {status}"));
        }
        // Asked for a range but got the whole body: start over rather than
        // appending a full copy of the file after the partial data.
        if offset > 0 && status != reqwest::StatusCode::PARTIAL_CONTENT {
            offset = 0;
        }

        file.set_len(offset)
            .await
            .map_err(|e| format!("Failed to prepare download file: {e}"))?;
        file.seek(SeekFrom::Start(offset))
            .await
            .map_err(|e| format!("Failed to seek download file: {e}"))?;
        task.downloaded_bytes = offset as i64;

        let mut stream = res.bytes_stream();
        let mut speed_calc = SpeedCalculator::new(2);
        let mut speed_limiter = SpeedLimiter::new(task.speed_limit);
        let mut last_progress_emit = Instant::now();

        loop {
            let next = tokio::select! {
                biased;
                _ = wait_paused(&mut pause_rx) => {
                    file.flush().await.map_err(|e| format!("Failed to flush file: {e}"))?;
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
            speed_limiter.throttle(chunk_len).await;

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
                        downloaded_bytes: task.downloaded_bytes as u64,
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

        let final_path = resolve_unique_path(
            final_dest.parent().unwrap_or(Path::new(".")),
            &file_name_of(&final_dest),
        );
        tokio::fs::rename(&part_file, &final_path)
            .await
            .map_err(|e| format!("Failed to rename completed file: {e}"))?;

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

    async fn run_segmented_download<F>(
        &self,
        mut task: DownloadTask,
        final_dest: PathBuf,
        client: reqwest::Client,
        mut pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let dest_dir = final_dest.parent().unwrap_or(Path::new(".")).to_path_buf();
        let total_bytes = task.total_bytes as u64;
        let num_segments = task.segments.len();
        let (tx, mut rx) = mpsc::channel::<WorkerMessage>(256);
        let (cancel_tx, _) = watch::channel(false);

        // Mirror pool for multi-network distribution
        let mut mirror_pool: Vec<String> = vec![task.url.clone()];
        mirror_pool.extend(task.mirrors.iter().cloned());

        let user_agent = task.user_agent.clone().unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        let referer = task.referer.clone();

        // One worker per segment. Segments that are already complete on disk
        // finish immediately after checking their file, so we never have to
        // trust progress numbers that may be stale in the database.
        for seg in &task.segments {
            let ctx = SegmentContext {
                client: client.clone(),
                url: mirror_pool[seg.segment_index % mirror_pool.len()].clone(),
                user_agent: user_agent.clone(),
                referer: referer.clone(),
                segment: seg.clone(),
                file_total: total_bytes,
                path: dest_dir.join(format!("{}.nexpart_seg_{}", task.filename, seg.segment_index)),
                tx: tx.clone(),
                cancel: cancel_tx.subscribe(),
            };
            tauri::async_runtime::spawn(run_segment_worker(ctx));
        }
        // Drop our sender so `rx` closes once every worker has ended.
        drop(tx);

        let mut finished = vec![false; num_segments];
        let mut finished_count = 0usize;

        let mut ticker = tokio::time::interval(PROGRESS_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_tick = Instant::now();
        let mut last_total = 0u64;
        let mut smoothed_bps = 0.0f64;
        let mut baseline_set = false;

        let sum_downloaded = |segments: &[DownloadSegment]| -> u64 {
            segments.iter().map(|s| s.downloaded_bytes.min(s.total_bytes)).sum()
        };

        while finished_count < num_segments {
            tokio::select! {
                biased;
                _ = wait_paused(&mut pause_rx) => {
                    let _ = cancel_tx.send(true);
                    drain_workers(&mut rx).await;
                    task.status = DownloadStatus::Paused;
                    task.downloaded_bytes = sum_downloaded(&task.segments) as i64;
                    task.speed_bps = 0;
                    task.eta_seconds = None;
                    return Ok(task);
                }
                msg = rx.recv() => {
                    match msg {
                        Some(WorkerMessage::Started { segment_index, already }) => {
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.downloaded_bytes = already;
                                seg.status = "downloading".to_string();
                            }
                            if !baseline_set {
                                // Baseline so resumed data isn't counted as instant speed.
                                last_total = sum_downloaded(&task.segments);
                                baseline_set = true;
                            }
                        }
                        Some(WorkerMessage::Progress { segment_index, bytes_downloaded }) => {
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.downloaded_bytes = bytes_downloaded;
                                seg.status = "downloading".to_string();
                            }
                        }
                        Some(WorkerMessage::Finished { segment_index }) => {
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.status = "completed".to_string();
                                seg.downloaded_bytes = seg.total_bytes;
                            }
                            if !finished[segment_index] {
                                finished[segment_index] = true;
                                finished_count += 1;
                            }
                        }
                        Some(WorkerMessage::Failed { segment_index, error }) => {
                            let _ = cancel_tx.send(true);
                            drain_workers(&mut rx).await;
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.status = "failed".to_string();
                            }
                            return Err(error);
                        }
                        None => {
                            // Every worker ended without reporting completion for all segments.
                            if finished_count < num_segments {
                                return Err("Download workers stopped unexpectedly".to_string());
                            }
                        }
                    }
                }
                _ = ticker.tick() => {
                    let total = sum_downloaded(&task.segments);
                    let now = Instant::now();
                    let dt = now.duration_since(last_tick).as_secs_f64().max(0.001);
                    let instant_bps = total.saturating_sub(last_total) as f64 / dt;
                    smoothed_bps = if smoothed_bps == 0.0 { instant_bps } else { 0.35 * instant_bps + 0.65 * smoothed_bps };
                    last_tick = now;
                    last_total = total;

                    let speed_bps = smoothed_bps as u64;
                    let eta = SpeedCalculator::calculate_eta(total_bytes.saturating_sub(total), speed_bps);
                    task.downloaded_bytes = total as i64;
                    task.speed_bps = speed_bps;
                    task.eta_seconds = eta;

                    on_progress(DownloadProgressEvent {
                        id: task.id.clone(),
                        downloaded_bytes: task.downloaded_bytes,
                        total_bytes: task.total_bytes,
                        speed_bps,
                        eta_seconds: eta,
                        status: DownloadStatus::Downloading,
                        segments: task.segments.clone(),
                    });
                }
            }
        }

        // ── Merge segment files ────────────────────────────────────────────
        // Verify every piece first so a short file can never be stitched in.
        let seg_paths: Vec<PathBuf> = (0..num_segments)
            .map(|i| dest_dir.join(format!("{}.nexpart_seg_{}", task.filename, i)))
            .collect();
        for (seg, path) in task.segments.iter().zip(&seg_paths) {
            let len = tokio::fs::metadata(path).await.map(|m| m.len()).unwrap_or(0);
            if len != seg.total_bytes {
                return Err(format!(
                    "Segment {} is incomplete on disk ({} of {} bytes); retry to resume it",
                    seg.segment_index + 1,
                    len,
                    seg.total_bytes
                ));
            }
        }

        let merge_path = dest_dir.join(format!("{}.nexmerge", task.filename));
        if let Err(e) = merge_segments(&seg_paths, &merge_path, total_bytes).await {
            let _ = tokio::fs::remove_file(&merge_path).await;
            return Err(e);
        }

        let final_path = resolve_unique_path(&dest_dir, &file_name_of(&final_dest));
        if let Err(e) = tokio::fs::rename(&merge_path, &final_path).await {
            let _ = tokio::fs::remove_file(&merge_path).await;
            return Err(format!("Failed to move merged file into place: {e}"));
        }
        for path in &seg_paths {
            let _ = tokio::fs::remove_file(path).await;
        }

        finish_task(&mut task, &final_path);
        task.downloaded_bytes = task.total_bytes;
        for seg in &mut task.segments {
            seg.status = "completed".to_string();
            seg.downloaded_bytes = seg.total_bytes;
        }

        on_progress(DownloadProgressEvent {
            id: task.id.clone(),
            downloaded_bytes: task.total_bytes,
            total_bytes: task.total_bytes,
            speed_bps: 0,
            eta_seconds: None,
            status: DownloadStatus::Completed,
            segments: task.segments.clone(),
        });

        Ok(task)
    }
}

// ───────────────────────────── segment worker ─────────────────────────────

struct SegmentContext {
    client: reqwest::Client,
    url: String,
    user_agent: String,
    referer: Option<String>,
    segment: DownloadSegment,
    /// Size of the whole file, used to recognise a request that covers all of it.
    file_total: u64,
    path: PathBuf,
    tx: mpsc::Sender<WorkerMessage>,
    cancel: watch::Receiver<bool>,
}

enum SegmentEnd {
    Cancelled,
    Failed(String),
}

async fn run_segment_worker(mut ctx: SegmentContext) {
    let index = ctx.segment.segment_index;
    let tx = ctx.tx.clone();
    match transfer_segment(&mut ctx).await {
        Ok(()) => {
            let _ = tx.send(WorkerMessage::Finished { segment_index: index }).await;
        }
        Err(SegmentEnd::Cancelled) => {}
        Err(SegmentEnd::Failed(error)) => {
            let _ = tx.send(WorkerMessage::Failed { segment_index: index, error }).await;
        }
    }
}

async fn transfer_segment(ctx: &mut SegmentContext) -> Result<(), SegmentEnd> {
    let index = ctx.segment.segment_index;
    let total = ctx.segment.total_bytes;
    let fail = |msg: String| SegmentEnd::Failed(msg);

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&ctx.path)
        .await
        .map_err(|e| fail(format!("Cannot open segment file: {e}")))?;

    // The file on disk is the truth about what we have: bytes are appended
    // strictly in order, so its length is exactly how far this segment got.
    let disk_len = file
        .metadata()
        .await
        .map_err(|e| fail(format!("Cannot inspect segment file: {e}")))?
        .len();
    let already = disk_len.min(total);
    if disk_len != already {
        file.set_len(already)
            .await
            .map_err(|e| fail(format!("Cannot trim segment file: {e}")))?;
    }
    file.seek(SeekFrom::Start(already))
        .await
        .map_err(|e| fail(format!("Cannot seek segment file: {e}")))?;

    if ctx.tx.send(WorkerMessage::Started { segment_index: index, already }).await.is_err() {
        return Err(SegmentEnd::Cancelled);
    }
    if already >= total {
        return Ok(());
    }

    let start_offset = ctx.segment.start_byte + already;
    let end_offset = ctx.segment.end_byte;

    let req = build_request(
        &ctx.client,
        &ctx.url,
        &ctx.user_agent,
        ctx.referer.as_deref(),
        Some(format!("bytes={start_offset}-{end_offset}")),
    );
    let resp = match send_or_pause(req, &mut ctx.cancel).await {
        Ok(Some(resp)) => resp,
        Ok(None) => return Err(SegmentEnd::Cancelled),
        Err(e) => return Err(fail(e)),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        // The server must be answering the range we asked for.
        if let Some(first) = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range_start)
        {
            if first != start_offset {
                return Err(fail(format!(
                    "Server returned bytes starting at {first} but {start_offset} was requested"
                )));
            }
        }
    } else if status == reqwest::StatusCode::OK && start_offset == 0 && end_offset + 1 >= ctx.file_total {
        // Whole file requested and whole file returned: fine.
    } else if status == reqwest::StatusCode::OK {
        return Err(fail(
            "Server ignored the Range request (HTTP 200), so this file can't be downloaded in parts. \
             Set connections to 1 and retry."
                .to_string(),
        ));
    } else {
        return Err(fail(format!("HTTP error: {status}")));
    }

    let mut stream = resp.bytes_stream();
    let mut received = already;

    while received < total {
        let next = tokio::select! {
            biased;
            _ = wait_paused(&mut ctx.cancel) => {
                let _ = file.flush().await;
                return Err(SegmentEnd::Cancelled);
            }
            next = stream.next() => next,
        };

        let Some(chunk_res) = next else { break };
        let chunk = chunk_res.map_err(|e| fail(format!("Stream read error: {e}")))?;

        // Never write past the end of this segment, whatever the server sends.
        let take = (chunk.len() as u64).min(total - received) as usize;
        if take > 0 {
            file.write_all(&chunk[..take])
                .await
                .map_err(|e| fail(format!("Disk write error: {e}")))?;
            received += take as u64;
            // Progress is a lossy absolute counter; dropping one when the
            // channel is busy is harmless and keeps workers from stalling.
            let _ = ctx.tx.try_send(WorkerMessage::Progress {
                segment_index: index,
                bytes_downloaded: received,
            });
        }
    }

    file.flush()
        .await
        .map_err(|e| fail(format!("Failed to flush segment file: {e}")))?;

    if received < total {
        return Err(fail(format!(
            "Connection closed early: segment {} received {} of {} bytes",
            index + 1,
            received,
            total
        )));
    }
    Ok(())
}

// ─────────────────────────────── helpers ───────────────────────────────

fn build_request(
    client: &reqwest::Client,
    url: &str,
    user_agent: &str,
    referer: Option<&str>,
    range: Option<String>,
) -> reqwest::RequestBuilder {
    let mut req = client.get(url).header(USER_AGENT, user_agent);
    if let Some(r) = referer {
        req = req.header("Referer", r);
    }
    if let Some(range) = range {
        req = req.header(RANGE, range);
    }
    req
}

/// Resolves when the watched flag becomes `true`. If the sender goes away the
/// flag can never change again, so this then waits forever (never "pauses").
async fn wait_paused(rx: &mut watch::Receiver<bool>) {
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

/// Waits (bounded) for workers to notice cancellation and drop their senders,
/// so nothing is still writing to the segment files when we return.
async fn drain_workers(rx: &mut mpsc::Receiver<WorkerMessage>) {
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        while rx.recv().await.is_some() {}
    })
    .await;
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

/// `bytes 100-199/1000` → 100
fn parse_content_range_start(value: &str) -> Option<u64> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let range = rest.split('/').next()?;
    range.split('-').next()?.trim().parse().ok()
}

async fn merge_segments(seg_paths: &[PathBuf], target: &Path, expected_len: u64) -> Result<(), String> {
    let mut out = File::create(target)
        .await
        .map_err(|e| format!("Failed to create merged file: {e}"))?;

    for (i, path) in seg_paths.iter().enumerate() {
        let src = File::open(path)
            .await
            .map_err(|e| format!("Failed to open segment file {}: {e}", i + 1))?;
        let mut reader = BufReader::with_capacity(1024 * 1024, src);
        tokio::io::copy_buf(&mut reader, &mut out)
            .await
            .map_err(|e| format!("Failed while merging segment {}: {e}", i + 1))?;
    }

    out.flush().await.map_err(|e| format!("Failed to flush merged file: {e}"))?;
    out.sync_all().await.map_err(|e| format!("Failed to sync merged file: {e}"))?;
    drop(out);

    let len = tokio::fs::metadata(target).await.map(|m| m.len()).unwrap_or(0);
    if len != expected_len {
        return Err(format!("Merged file is {len} bytes, expected {expected_len}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_content_range_start;

    #[test]
    fn parses_content_range() {
        assert_eq!(parse_content_range_start("bytes 100-199/1000"), Some(100));
        assert_eq!(parse_content_range_start("bytes 0-0/10"), Some(0));
        assert_eq!(parse_content_range_start("bytes */1000"), None);
        assert_eq!(parse_content_range_start("garbage"), None);
    }
}
