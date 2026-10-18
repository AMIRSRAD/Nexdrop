//! Throughput benchmark for the download engine over loopback.
//!
//!   cargo test --release --test bench_engine -- --ignored --nocapture
//!
//! Loopback has no network latency, so this isolates the engine's own
//! overhead: disk writes, merging, progress bookkeeping.

use std::sync::Arc;
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::calculate_segments_with_chunk_size;
use nexdrop_lib::engine::downloader::DownloadEngine;

/// `uneven`: the connection that serves byte 0 is 8x slower than the rest, which
/// is how real servers/CDNs behave (per-connection speed varies a lot).
async fn serve(payload: Arc<Vec<u8>>, uneven: bool) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let payload = payload.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let total = payload.len();
                let (start, end) = req
                    .lines()
                    .find(|l| l.to_lowercase().starts_with("range:"))
                    .and_then(|l| l.split('=').nth(1))
                    .map(|r| {
                        let mut it = r.trim().split('-');
                        let s: usize = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                        let e: usize = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(total - 1).min(total - 1);
                        (s, e)
                    })
                    .unwrap_or((0, total - 1));
                let body = &payload[start..=end];
                let head = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                if uneven {
                    // ~8 MiB/s per connection, but 1 MiB/s for the one starting at byte 0
                    let delay_ms = if start == 0 { 250 } else { 31 };
                    for piece in body.chunks(256 * 1024) {
                        if socket.write_all(piece).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    }
                } else {
                    let _ = socket.write_all(body).await;
                }
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn bench_8_connections_384mib() {
    bench(384, false, "even connections").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn bench_uneven_connections_64mib() {
    bench(64, true, "one connection 8x slower").await;
}

async fn bench(mib: usize, uneven: bool, label: &str) {
    const MIB: usize = 1024 * 1024;
    let len = mib * MIB;
    let payload = Arc::new((0..len).map(|i| (i % 251) as u8).collect::<Vec<u8>>());
    let base = serve(payload.clone(), uneven).await;

    let dir = std::env::temp_dir().join(format!("nexdrop_bench_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let segments = calculate_segments_with_chunk_size(len as i64, 8, MIB as u64);
    let url = format!("{base}/big.bin");
    let task = DownloadTask {
        id: "bench".into(),
        url: url.clone(),
        final_url: Some(url),
        filename: "big.bin".into(),
        destination_path: dir.to_string_lossy().to_string(),
        temp_path: String::new(),
        total_bytes: len as i64,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: None,
        last_modified: None,
        mime_type: None,
        category: FileCategory::Other,
        connections: segments.len(),
        speed_limit: 0,
        priority: 0,
        speed_bps: 0,
        eta_seconds: None,
        segments,
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
        min_chunk_size: MIB as u64,
        proxy_mode: None,
        custom_proxy_url: None,
        antivirus_status: None,
        expected_checksum: None,
        custom_headers: Default::default(),
    };

    let (_tx, rx) = watch::channel(false);
    let started = Instant::now();
    let done = DownloadEngine::new().run_download(task, rx, |_| {}).await.unwrap();
    let secs = started.elapsed().as_secs_f64();

    assert_eq!(done.status, DownloadStatus::Completed);
    assert_eq!(std::fs::metadata(dir.join(&done.filename)).unwrap().len(), len as u64);
    println!(
        "BENCH[{label}]: {} MiB in {:.2}s = {:.0} MiB/s",
        len / MIB,
        secs,
        (len / MIB) as f64 / secs
    );
    let _ = std::fs::remove_dir_all(&dir);
}
