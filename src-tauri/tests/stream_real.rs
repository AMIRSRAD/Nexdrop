//! Downloads real public test streams and leaves the results in
//! `target/real_streams/` for `scripts/verify_streams.py` to decode.
//!
//!   cargo test --release --test stream_real -- --ignored --nocapture --test-threads=1
//!
//! Needs internet access; the lowest quality of each stream is used.

use std::time::{Duration, Instant};

use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::client::create_http_client;
use nexdrop_lib::engine::downloader::DownloadEngine;
use nexdrop_lib::engine::stream::{self, Fetch};

fn task(url: &str, dir: &std::path::Path, name: &str, variant: &str) -> DownloadTask {
    DownloadTask {
        id: "real".into(),
        url: url.into(),
        final_url: None,
        filename: name.into(),
        destination_path: dir.to_string_lossy().to_string(),
        temp_path: String::new(),
        total_bytes: 0,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: Some(variant.into()),
        last_modified: None,
        mime_type: None,
        category: FileCategory::Videos,
        connections: 16,
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
        min_chunk_size: 1024 * 1024,
        proxy_mode: Some("system".into()),
        custom_proxy_url: None,
        antivirus_status: None,
        expected_checksum: None,
        custom_headers: Default::default(),
        kind: "stream".into(),
        info_hash: None,
        uploaded_bytes: 0,
        peers: 0,
        seeding: false,
        torrent_files: vec![],
        queue_id: "main".into(),
    }
}

async fn grab(url: &str, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("real_streams");
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join(name));

    let fetch = Fetch::new(create_http_client(Duration::from_secs(20)), Vec::new(), url, None, None, Duration::from_millis(200));
    let probe = stream::probe(&fetch, url).await.expect("probe");
    println!("STREAM {name}: {} · {:.0}s · {} qualities", probe.kind, probe.duration_secs, probe.variants.len());
    for v in &probe.variants {
        println!("   {} {} {}", v.id, v.label, if v.separate_audio { "(separate audio)" } else { "" });
    }
    assert!(probe.unsupported.is_none(), "{:?}", probe.unsupported);
    let lowest = probe.variants.iter().filter(|v| v.height > 0).min_by_key(|v| v.bandwidth).unwrap_or(&probe.variants[0]).id.clone();

    let started = Instant::now();
    let (_tx, rx) = watch::channel(false);
    let done = DownloadEngine::new()
        .run_stream(task(url, &dir, name, &lowest), rx, Vec::new(), |_| {})
        .await
        .unwrap_or_else(|e| panic!("{name} failed: {e}"));
    let size = std::fs::metadata(dir.join(&done.filename)).unwrap().len();
    println!("DONE {} -> {} ({:.1} MB in {:.1}s)", name, done.filename, size as f64 / 1e6, started.elapsed().as_secs_f64());
    assert!(size > 100_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn apple_hls_fmp4_with_separate_audio() {
    grab("https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8", "apple_fmp4.mp4").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn apple_hls_transport_stream() {
    grab("https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/bipbop_4x3_variant.m3u8", "apple_ts.ts").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn akamai_dash_big_buck_bunny() {
    grab("https://dash.akamaized.net/akamai/bbb_30fps/bbb_30fps.mpd", "bbb_dash.mp4").await;
}
