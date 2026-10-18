use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadSegment, DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::downloader::DownloadEngine;

/// Helper function to start a local mock HTTP server that supports Range requests
async fn start_mock_range_server(payload: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = Arc::new(payload);

    tokio::spawn(async move {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };

            let payload_clone = payload.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 2048];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]);

                let total_len = payload_clone.len();

                // Check for Range header
                if let Some(range_line) = request.lines().find(|l| l.to_lowercase().starts_with("range:")) {
                    // Range: bytes=START-END
                    if let Some(range_val) = range_line.split('=').nth(1) {
                        let parts: Vec<&str> = range_val.split('-').collect();
                        let start: usize = parts[0].trim().parse().unwrap_or(0);
                        let end: usize = if parts.len() > 1 && !parts[1].trim().is_empty() {
                            parts[1].trim().parse().unwrap_or(total_len - 1)
                        } else {
                            total_len - 1
                        };

                        let end = end.min(total_len - 1);
                        let chunk = &payload_clone[start..=end];
                        let chunk_len = chunk.len();

                        let response_headers = format!(
                            "HTTP/1.1 206 Partial Content\r\n\
                             Content-Range: bytes {}-{}/{}\r\n\
                             Content-Length: {}\r\n\
                             Accept-Ranges: bytes\r\n\
                             Content-Type: application/octet-stream\r\n\
                             Connection: close\r\n\r\n",
                            start, end, total_len, chunk_len
                        );

                        let _ = socket.write_all(response_headers.as_bytes()).await;
                        let _ = socket.write_all(chunk).await;
                        let _ = socket.flush().await;
                        return;
                    }
                }

                // Standard 200 response
                let response_headers = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Length: {}\r\n\
                     Accept-Ranges: bytes\r\n\
                     Content-Type: application/octet-stream\r\n\
                     Connection: close\r\n\r\n",
                    total_len
                );
                let _ = socket.write_all(response_headers.as_bytes()).await;
                let _ = socket.write_all(&payload_clone).await;
                let _ = socket.flush().await;
            });
        }
    });

    format!("http://{}", addr)
}

#[tokio::test]
async fn test_concurrent_segmented_download_and_merge() {
    // 1. Create a 1MB payload of distinct pseudo-random bytes
    let mut payload = vec![0u8; 1024 * 1024];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = ((i * 31 + 17) % 256) as u8;
    }

    let server_url = start_mock_range_server(payload.clone()).await;

    // 2. Temp directory for test
    let temp_dir = std::env::temp_dir().join(format!("nexdrop_test_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let task = DownloadTask {
        id: "test-integration-1".to_string(),
        url: format!("{}/testfile.dat", server_url),
        final_url: Some(format!("{}/testfile.dat", server_url)),
        filename: "testfile.dat".to_string(),
        destination_path: temp_dir.to_string_lossy().to_string(),
        temp_path: temp_dir.join("testfile.dat.nexpart").to_string_lossy().to_string(),
        total_bytes: payload.len() as i64,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: None,
        last_modified: None,
        mime_type: Some("application/octet-stream".to_string()),
        category: FileCategory::Other,
        connections: 4,
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments: vec![
            DownloadSegment {
                segment_index: 0,
                start_byte: 0,
                end_byte: 262143,
                downloaded_bytes: 0,
                total_bytes: 262144,
                status: "pending".to_string(),
            },
            DownloadSegment {
                segment_index: 1,
                start_byte: 262144,
                end_byte: 524287,
                downloaded_bytes: 0,
                total_bytes: 262144,
                status: "pending".to_string(),
            },
            DownloadSegment {
                segment_index: 2,
                start_byte: 524288,
                end_byte: 786431,
                downloaded_bytes: 0,
                total_bytes: 262144,
                status: "pending".to_string(),
            },
            DownloadSegment {
                segment_index: 3,
                start_byte: 786432,
                end_byte: 1048575,
                downloaded_bytes: 0,
                total_bytes: 262144,
                status: "pending".to_string(),
            },
        ],
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
        min_chunk_size: 1024 * 1024,
        proxy_mode: None,
        custom_proxy_url: None,
        antivirus_status: None,
    };

    let engine = DownloadEngine::new();
    let (_pause_tx, pause_rx) = watch::channel(false);

    let result = engine
        .run_download(task, pause_rx, |_| {})
        .await
        .expect("Segmented download should succeed");

    assert_eq!(result.status, DownloadStatus::Completed);
    assert_eq!(result.downloaded_bytes, payload.len() as i64);

    // 3. Verify downloaded file on disk
    let downloaded_file_path = temp_dir.join(&result.filename);
    assert!(downloaded_file_path.exists(), "Downloaded file must exist on disk");

    let downloaded_content = tokio::fs::read(&downloaded_file_path).await.unwrap();
    assert_eq!(
        downloaded_content.len(),
        payload.len(),
        "Merged file length must match payload length"
    );

    // Verify byte-for-byte fidelity across all 4 merged segments
    assert_eq!(
        downloaded_content, payload,
        "Downloaded file bytes must exactly match server payload"
    );

    // Clean up
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}

#[tokio::test]
async fn test_pause_and_resume_segmented_download() {
    let mut payload = vec![0u8; 1024 * 1024];
    for (i, b) in payload.iter_mut().enumerate() {
        *b = ((i * 13 + 5) % 256) as u8;
    }

    let server_url = start_mock_range_server(payload.clone()).await;
    let temp_dir = std::env::temp_dir().join(format!("nexdrop_test_pause_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.unwrap();

    let task = DownloadTask {
        id: "test-pause-resume".to_string(),
        url: format!("{}/pause_test.dat", server_url),
        final_url: Some(format!("{}/pause_test.dat", server_url)),
        filename: "pause_test.dat".to_string(),
        destination_path: temp_dir.to_string_lossy().to_string(),
        temp_path: temp_dir.join("pause_test.dat.nexpart").to_string_lossy().to_string(),
        total_bytes: payload.len() as i64,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: None,
        last_modified: None,
        mime_type: Some("application/octet-stream".to_string()),
        category: FileCategory::Other,
        connections: 2,
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments: vec![
            DownloadSegment {
                segment_index: 0,
                start_byte: 0,
                end_byte: 524287,
                downloaded_bytes: 0,
                total_bytes: 524288,
                status: "pending".to_string(),
            },
            DownloadSegment {
                segment_index: 1,
                start_byte: 524288,
                end_byte: 1048575,
                downloaded_bytes: 0,
                total_bytes: 524288,
                status: "pending".to_string(),
            },
        ],
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
        min_chunk_size: 1024 * 1024,
        proxy_mode: None,
        custom_proxy_url: None,
        antivirus_status: None,
    };

    let engine = DownloadEngine::new();
    let (pause_tx, pause_rx) = watch::channel(false);

    // Trigger pause after 20ms
    let p_tx = pause_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        let _ = p_tx.send(true);
    });

    let paused_result = engine.run_download(task, pause_rx, |_| {}).await;

    // Paused result should be returned
    if let Ok(paused_task) = paused_result {
        if paused_task.status == DownloadStatus::Paused {
            // Resume with a fresh unpaused channel
            let (_new_tx, new_rx) = watch::channel(false);
            let resume_result = engine.run_download(paused_task, new_rx, |_| {}).await.unwrap();

            assert_eq!(resume_result.status, DownloadStatus::Completed);
            let downloaded_file = temp_dir.join(&resume_result.filename);
            assert!(downloaded_file.exists());
            let content = tokio::fs::read(&downloaded_file).await.unwrap();
            assert_eq!(content, payload);
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
}


// â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ Phase 1 regression tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[derive(Clone, Copy)]
enum Mode {
    /// Answers Range requests correctly with 206.
    Normal,
    /// Ignores Range and always answers 200 with the whole file.
    IgnoreRange,
    /// Answers correctly but closes the connection after half of each body.
    CutShort,
    /// Answers correctly but trickles the body out slowly.
    Slow { chunk: usize, delay_ms: u64 },
}

async fn start_server(payload: Vec<u8>, mode: Mode) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let payload = Arc::new(payload);

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let payload = payload.clone();
            tokio::spawn(async move {
                let mut buffer = [0u8; 4096];
                let n = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let total = payload.len();

                let range = request
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("range:"))
                    .and_then(|l| l.split('=').nth(1).map(|v| v.to_string()));

                if let (Some(range), false) = (range, matches!(mode, Mode::IgnoreRange)) {
                    let mut it = range.trim().split('-');
                    let start: usize = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    let end: usize = it
                        .next()
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(total - 1)
                        .min(total - 1);
                    let body = &payload[start..=end];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    match mode {
                        Mode::CutShort => {
                            let _ = socket.write_all(&body[..body.len() / 2]).await;
                        }
                        Mode::Slow { chunk, delay_ms } => {
                            for piece in body.chunks(chunk) {
                                if socket.write_all(piece).await.is_err() {
                                    return;
                                }
                                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                            }
                        }
                        _ => {
                            let _ = socket.write_all(body).await;
                        }
                    }
                } else {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&payload).await;
                }
                let _ = socket.flush().await;
            });
        }
    });

    format!("http://{addr}")
}

fn patterned(len: usize) -> Vec<u8> {
    (0..len).map(|i| ((i * 31 + 7) % 251) as u8).collect()
}

fn two_segment_task(url: &str, dir: &std::path::Path, len: usize, name: &str) -> DownloadTask {
    let half = len / 2;
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
        connections: 2,
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments: vec![
            DownloadSegment { segment_index: 0, start_byte: 0, end_byte: (half - 1) as u64, downloaded_bytes: 0, total_bytes: half as u64, status: "pending".into() },
            DownloadSegment { segment_index: 1, start_byte: half as u64, end_byte: (len - 1) as u64, downloaded_bytes: 0, total_bytes: (len - half) as u64, status: "pending".into() },
        ],
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
        min_chunk_size: 1024 * 1024,
        proxy_mode: None,
        custom_proxy_url: None,
        antivirus_status: None,
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("nexdrop_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The database may be stale (it used to store segment progress only at the very
/// end), but the bytes on disk are what count. Resuming must continue from the
/// real file length instead of appending duplicate data.
#[tokio::test]
async fn resume_trusts_disk_not_stale_progress() {
    let payload = patterned(1_000_000);
    let url = format!("{}/stale.bin", start_server(payload.clone(), Mode::Normal).await);
    let dir = temp_dir("stale");

    // Segment 0 already has its first 123_457 bytes on disk, but the task says 0.
    std::fs::write(dir.join("stale.bin.nexpart_seg_0"), &payload[..123_457]).unwrap();
    // Segment 1 has garbage beyond its length (e.g. from a previous bad run).
    let mut over = payload[500_000..].to_vec();
    over.extend_from_slice(&[0xAA; 4096]);
    std::fs::write(dir.join("stale.bin.nexpart_seg_1"), &over).unwrap();

    let task = two_segment_task(&url, &dir, payload.len(), "stale.bin");
    let (_tx, rx) = watch::channel(false);
    let done = DownloadEngine::new().run_download(task, rx, |_| {}).await.unwrap();

    assert_eq!(done.status, DownloadStatus::Completed);
    let out = std::fs::read(dir.join(&done.filename)).unwrap();
    assert_eq!(out.len(), payload.len());
    assert!(out == payload, "resumed file differs from the original");
    assert!(!dir.join("stale.bin.nexpart_seg_0").exists(), "segment files should be cleaned up");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server that ignores `Range` and returns 200 must not get its whole body
/// written into every segment.
#[tokio::test]
async fn server_ignoring_range_is_rejected_not_corrupted() {
    let payload = patterned(600_000);
    let url = format!("{}/norange.bin", start_server(payload, Mode::IgnoreRange).await);
    let dir = temp_dir("norange");

    let task = two_segment_task(&url, &dir, 600_000, "norange.bin");
    let (_tx, rx) = watch::channel(false);
    let res = DownloadEngine::new().run_download(task, rx, |_| {}).await;

    let err = res.expect_err("must fail rather than assemble a bad file");
    assert!(err.to_lowercase().contains("range"), "unexpected error: {err}");
    assert!(!dir.join("norange.bin").exists(), "no final file may be produced");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A connection that closes early must be a failure, never a "completed" file.
#[tokio::test]
async fn truncated_response_is_a_failure() {
    let payload = patterned(600_000);
    let url = format!("{}/short.bin", start_server(payload, Mode::CutShort).await);
    let dir = temp_dir("short");

    let task = two_segment_task(&url, &dir, 600_000, "short.bin");
    let (_tx, rx) = watch::channel(false);
    let res = DownloadEngine::new().run_download(task, rx, |_| {}).await;

    assert!(res.is_err(), "a short body must not complete");
    assert!(!dir.join("short.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Deterministic pause/resume: the server trickles data so the pause really
/// lands mid-transfer, then the resumed run must produce a byte-exact file.
#[tokio::test]
async fn pause_mid_transfer_then_resume_is_byte_exact() {
    let payload = patterned(1_000_000);
    let url = format!(
        "{}/slow.bin",
        start_server(payload.clone(), Mode::Slow { chunk: 16 * 1024, delay_ms: 15 }).await
    );
    let dir = temp_dir("slow");

    let task = two_segment_task(&url, &dir, payload.len(), "slow.bin");
    let engine = DownloadEngine::new();
    let (pause_tx, pause_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        let _ = pause_tx.send(true);
    });

    let paused = engine.run_download(task, pause_rx, |_| {}).await.unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused, "the transfer finished before the pause landed");
    assert!(paused.downloaded_bytes > 0 && paused.downloaded_bytes < payload.len() as i64);
    assert!(!dir.join("slow.bin").exists(), "a paused download must not produce a final file");

    // Resume from what's on disk (progress numbers deliberately reset to zero).
    let mut resumed = paused;
    resumed.status = DownloadStatus::Downloading;
    resumed.downloaded_bytes = 0;
    for seg in &mut resumed.segments {
        seg.downloaded_bytes = 0;
    }
    let (_tx, rx) = watch::channel(false);
    let done = engine.run_download(resumed, rx, |_| {}).await.unwrap();

    assert_eq!(done.status, DownloadStatus::Completed);
    let out = std::fs::read(dir.join(&done.filename)).unwrap();
    assert!(out == payload, "file differs after pause + resume");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Single-connection downloads must restart cleanly (not append a second copy)
/// when the server cannot honour the resume request.
#[tokio::test]
async fn single_stream_restarts_when_resume_is_refused() {
    let payload = patterned(300_000);
    let url = format!("{}/single.bin", start_server(payload.clone(), Mode::IgnoreRange).await);
    let dir = temp_dir("single");
    std::fs::write(dir.join("single.bin.nexpart"), &payload[..100_000]).unwrap();

    let mut task = two_segment_task(&url, &dir, payload.len(), "single.bin");
    task.connections = 1;
    task.segments.truncate(1);
    let (_tx, rx) = watch::channel(false);
    let done = DownloadEngine::new().run_download(task, rx, |_| {}).await.unwrap();

    let out = std::fs::read(dir.join(&done.filename)).unwrap();
    assert_eq!(out.len(), payload.len(), "partial data must not be kept in front of a full body");
    assert!(out == payload);
    let _ = std::fs::remove_dir_all(&dir);
}

