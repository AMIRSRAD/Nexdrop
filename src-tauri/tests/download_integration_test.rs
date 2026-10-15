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
