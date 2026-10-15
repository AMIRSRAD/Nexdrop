use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{RANGE, USER_AGENT};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

use crate::db::models::{DownloadProgressEvent, DownloadSegment, DownloadStatus, DownloadTask};
use crate::engine::probe::resolve_unique_path;
use crate::engine::speed::{SpeedCalculator, SpeedLimiter};

pub enum WorkerMessage {
    Progress {
        segment_index: usize,
        bytes_downloaded: u64,
        incremental_bytes: u64,
    },
    SegmentFinished {
        segment_index: usize,
    },
    Failed {
        segment_index: usize,
        error: String,
    },
}

#[derive(Clone)]
pub struct DownloadEngine {
    client: reqwest::Client,
}

impl DownloadEngine {
    pub fn new() -> Self {
        let client = crate::engine::create_http_client(Duration::from_secs(30));
        Self { client }
    }

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
            let _ = tokio::fs::create_dir_all(dest_dir).await;
        }

        let final_dest = dest_dir.join(&task.filename);
        let part_file = dest_dir.join(format!("{}.nexpart", task.filename));

        let client = if task.proxy_mode.is_some() || task.custom_proxy_url.is_some() {
            crate::engine::client::create_http_client_with_proxy(
                Duration::from_secs(30),
                task.proxy_mode.as_deref().or(Some("system")),
                task.custom_proxy_url.as_deref(),
            )
        } else {
            self.client.clone()
        };

        // Fallback or 1-connection download
        if !task.resumable || task.total_bytes <= 0 || task.connections <= 1 {
            return self
                .run_single_stream(task, final_dest, part_file, client, pause_rx, on_progress)
                .await;
        }

        // Multi-segment range download
        self.run_segmented_download(task, final_dest, client, pause_rx, on_progress)
            .await
    }

    async fn run_single_stream<F>(
        &self,
        mut task: DownloadTask,
        final_dest: PathBuf,
        part_file: PathBuf,
        client: reqwest::Client,
        pause_rx: watch::Receiver<bool>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&part_file)
            .await
            .map_err(|e| format!("Failed to create download file: {e}"))?;

        let current_size = file
            .metadata()
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        task.downloaded_bytes = current_size as i64;

        let ua = task.user_agent.as_deref().unwrap_or("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0");
        let mut req = client
            .get(&task.url)
            .header(USER_AGENT, ua);

        if let Some(ref r) = task.referer {
            req = req.header("Referer", r);
        }

        if task.resumable && current_size > 0 {
            req = req.header(RANGE, format!("bytes={current_size}-"));
        }

        let res = req.send().await.map_err(|e| format!("HTTP request failed: {e}"))?;
        let status = res.status();

        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(format!("Server returned HTTP status: {status}"));
        }

        let mut stream = res.bytes_stream();
        let mut speed_calc = SpeedCalculator::new(2);
        let mut speed_limiter = SpeedLimiter::new(task.speed_limit);
        let mut last_progress_emit = Instant::now();

        while let Some(chunk_res) = stream.next().await {
            if *pause_rx.borrow() {
                task.status = DownloadStatus::Paused;
                return Ok(task);
            }

            let chunk = chunk_res.map_err(|e| format!("Network read error: {e}"))?;
            let chunk_len = chunk.len();

            file.write_all(&chunk)
                .await
                .map_err(|e| format!("Disk write error: {e}"))?;

            task.downloaded_bytes += chunk_len as i64;
            speed_calc.record_bytes(chunk_len as u64);
            speed_limiter.throttle(chunk_len).await;

            if last_progress_emit.elapsed() >= Duration::from_millis(250) {
                let speed_bps = speed_calc.current_speed_bps();
                let remaining = if task.total_bytes > task.downloaded_bytes {
                    (task.total_bytes - task.downloaded_bytes) as u64
                } else {
                    0
                };
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
                        total_bytes: if task.total_bytes > 0 { task.total_bytes as u64 } else { 0 },
                        status: "downloading".to_string(),
                    }],
                });
                last_progress_emit = Instant::now();
            }
        }

        file.flush().await.map_err(|e| format!("Failed to flush file: {e}"))?;
        drop(file);

        // Rename part file to final destination
        let final_path = resolve_unique_path(final_dest.parent().unwrap(), final_dest.file_name().unwrap().to_str().unwrap());
        tokio::fs::rename(&part_file, &final_path)
            .await
            .map_err(|e| format!("Failed to rename completed file: {e}"))?;

        task.destination_path = final_path.parent().unwrap().to_string_lossy().to_string();
        task.filename = final_path.file_name().unwrap().to_string_lossy().to_string();
        task.status = DownloadStatus::Completed;
        task.completed_at = Some(chrono::Utc::now().timestamp());
        task.speed_bps = 0;
        task.eta_seconds = None;

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
        let dest_dir = final_dest.parent().unwrap();
        let total_bytes = task.total_bytes as u64;
        let num_segments = task.segments.len();
        let (tx, mut rx) = mpsc::channel::<WorkerMessage>(100);

        // Spawn a worker for each segment that is not yet completed
        let is_cancelled = Arc::new(AtomicBool::new(false));

        // Prepare mirror pool for multi-network / mirror distribution
        let mirror_pool: Vec<String> = if task.mirrors.is_empty() {
            vec![task.url.clone()]
        } else {
            let mut pool = vec![task.url.clone()];
            pool.extend(task.mirrors.clone());
            pool
        };

        let user_agent = task.user_agent.clone().unwrap_or_else(|| "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0".to_string());
        let referer = task.referer.clone();

        for seg in &task.segments {
            if seg.downloaded_bytes >= seg.total_bytes && seg.total_bytes > 0 {
                continue;
            }

            let worker_client = client.clone();
            let seg_url = mirror_pool[seg.segment_index % mirror_pool.len()].clone();
            let seg_clone = seg.clone();
            let seg_file_path = dest_dir.join(format!("{}.nexpart_seg_{}", task.filename, seg.segment_index));
            let worker_tx = tx.clone();
            let cancelled = is_cancelled.clone();
            let ua = user_agent.clone();
            let ref_hdr = referer.clone();

            tauri::async_runtime::spawn(async move {
                let start_offset = seg_clone.start_byte + seg_clone.downloaded_bytes;
                let end_offset = seg_clone.end_byte;

                if start_offset > end_offset {
                    let _ = worker_tx.send(WorkerMessage::SegmentFinished { segment_index: seg_clone.segment_index }).await;
                    return;
                }

                let file_res = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .append(true)
                    .open(&seg_file_path)
                    .await;

                let mut file = match file_res {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = worker_tx.send(WorkerMessage::Failed {
                            segment_index: seg_clone.segment_index,
                            error: format!("File open error: {e}"),
                        }).await;
                        return;
                    }
                };

                // Double check existing file size on disk for resume safety
                if let Ok(meta) = file.metadata().await {
                    let disk_len = meta.len();
                    if disk_len > seg_clone.downloaded_bytes {
                        // File already has more data
                    }
                }

                let range_header = format!("bytes={}-{}", start_offset, end_offset);
                let mut req = worker_client
                    .get(&seg_url)
                    .header(USER_AGENT, ua)
                    .header(RANGE, range_header);

                if let Some(ref r) = ref_hdr {
                    req = req.header("Referer", r);
                }

                let resp_res = req.send().await;

                let resp = match resp_res {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = worker_tx.send(WorkerMessage::Failed {
                            segment_index: seg_clone.segment_index,
                            error: format!("Reqwest error: {e}"),
                        }).await;
                        return;
                    }
                };

                if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                    let _ = worker_tx.send(WorkerMessage::Failed {
                        segment_index: seg_clone.segment_index,
                        error: format!("HTTP error: {}", resp.status()),
                    }).await;
                    return;
                }

                let mut stream = resp.bytes_stream();
                let mut current_downloaded = seg_clone.downloaded_bytes;

                while let Some(chunk_res) = stream.next().await {
                    if cancelled.load(Ordering::Relaxed) {
                        let _ = file.flush().await;
                        return;
                    }

                    match chunk_res {
                        Ok(chunk) => {
                            let len = chunk.len() as u64;
                            if let Err(e) = file.write_all(&chunk).await {
                                let _ = worker_tx.send(WorkerMessage::Failed {
                                    segment_index: seg_clone.segment_index,
                                    error: format!("Write error: {e}"),
                                }).await;
                                return;
                            }

                            current_downloaded += len;
                            let _ = worker_tx.send(WorkerMessage::Progress {
                                segment_index: seg_clone.segment_index,
                                bytes_downloaded: current_downloaded,
                                incremental_bytes: len,
                            }).await;
                        }
                        Err(e) => {
                            let _ = worker_tx.send(WorkerMessage::Failed {
                                segment_index: seg_clone.segment_index,
                                error: format!("Stream read error: {e}"),
                            }).await;
                            return;
                        }
                    }
                }

                let _ = file.flush().await;
                let _ = worker_tx.send(WorkerMessage::SegmentFinished {
                    segment_index: seg_clone.segment_index,
                }).await;
            });
        }

        // Drop initial tx so rx closes when all worker branches finish
        drop(tx);

        let mut speed_calc = SpeedCalculator::new(2);
        let mut last_emit = Instant::now();
        let mut completed_segments = 0;

        for seg in &task.segments {
            if seg.downloaded_bytes >= seg.total_bytes && seg.total_bytes > 0 {
                completed_segments += 1;
            }
        }

        loop {
            tokio::select! {
                _ = pause_rx.changed() => {
                    if *pause_rx.borrow() {
                        is_cancelled.store(true, Ordering::Relaxed);
                        task.status = DownloadStatus::Paused;
                        task.speed_bps = 0;
                        task.eta_seconds = None;
                        return Ok(task);
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Some(WorkerMessage::Progress { segment_index, bytes_downloaded, incremental_bytes }) => {
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.downloaded_bytes = bytes_downloaded;
                                seg.status = "downloading".to_string();
                            }
                            task.downloaded_bytes += incremental_bytes as i64;
                            speed_calc.record_bytes(incremental_bytes);

                            if last_emit.elapsed() >= Duration::from_millis(250) {
                                let speed_bps = speed_calc.current_speed_bps();
                                let remaining = if total_bytes > task.downloaded_bytes as u64 {
                                    total_bytes - task.downloaded_bytes as u64
                                } else {
                                    0
                                };
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
                                    segments: task.segments.clone(),
                                });
                                last_emit = Instant::now();
                            }
                        }
                        Some(WorkerMessage::SegmentFinished { segment_index }) => {
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.status = "completed".to_string();
                                seg.downloaded_bytes = seg.total_bytes;
                            }
                            completed_segments += 1;
                            if completed_segments >= num_segments {
                                break;
                            }
                        }
                        Some(WorkerMessage::Failed { segment_index, error }) => {
                            is_cancelled.store(true, Ordering::Relaxed);
                            if let Some(seg) = task.segments.get_mut(segment_index) {
                                seg.status = "failed".to_string();
                            }
                            task.status = DownloadStatus::Failed;
                            task.error_message = Some(error.clone());
                            return Err(error);
                        }
                        None => {
                            // All workers terminated
                            break;
                        }
                    }
                }
            }
        }

        // Merge segment files into final destination
        let final_path = resolve_unique_path(final_dest.parent().unwrap(), final_dest.file_name().unwrap().to_str().unwrap());
        let mut final_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&final_path)
            .await
            .map_err(|e| format!("Failed to create final file: {e}"))?;

        for i in 0..num_segments {
            let seg_path = dest_dir.join(format!("{}.nexpart_seg_{}", task.filename, i));
            if seg_path.exists() {
                let mut seg_file = File::open(&seg_path)
                    .await
                    .map_err(|e| format!("Failed to open segment file {i}: {e}"))?;

                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = seg_file.read(&mut buf).await.map_err(|e| format!("Read segment error: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    final_file.write_all(&buf[..n]).await.map_err(|e| format!("Write to final file error: {e}"))?;
                }
                drop(seg_file);
                let _ = tokio::fs::remove_file(&seg_path).await;
            }
        }

        final_file.flush().await.map_err(|e| format!("Failed to flush final file: {e}"))?;
        drop(final_file);

        task.destination_path = final_path.parent().unwrap().to_string_lossy().to_string();
        task.filename = final_path.file_name().unwrap().to_string_lossy().to_string();
        task.status = DownloadStatus::Completed;
        task.downloaded_bytes = task.total_bytes;
        task.completed_at = Some(chrono::Utc::now().timestamp());
        task.speed_bps = 0;
        task.eta_seconds = None;

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
