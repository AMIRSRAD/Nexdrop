//! Streaming video (HLS / DASH) through the real engine, against a local server.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::client::create_http_client;
use nexdrop_lib::engine::downloader::DownloadEngine;
use nexdrop_lib::engine::stream::{self, Fetch};

// ───────────────────────────── server ─────────────────────────────

#[derive(Default)]
struct Site {
    files: Mutex<HashMap<String, Vec<u8>>>,
    /// path → number of requests that still get a 503
    flaky: Mutex<HashMap<String, usize>>,
    /// Milliseconds to wait before answering a request for a path containing this text.
    slow: Mutex<Option<(String, u64)>>,
    hits: AtomicUsize,
}

struct Server {
    base: String,
    site: Arc<Site>,
}

impl Server {
    fn put(&self, path: &str, data: impl Into<Vec<u8>>) {
        self.site.files.lock().unwrap().insert(path.to_string(), data.into());
    }
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

async fn start() -> Server {
    let site = Arc::new(Site::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let s = site.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { break };
            let site = s.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                loop {
                    let mut tmp = [0u8; 4096];
                    let Ok(n) = sock.read(&mut tmp).await else { return };
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
                    buf.drain(..pos + 4);
                    site.hits.fetch_add(1, Ordering::SeqCst);

                    let mut lines = head.lines();
                    let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap().to_string();
                    let range = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("range")).map(|(_, v)| v.trim().to_string());

                    let slow = site.slow.lock().unwrap().clone();
                    if let Some((needle, ms)) = slow {
                        if path.contains(&needle) {
                            tokio::time::sleep(Duration::from_millis(ms)).await;
                        }
                    }
                    let fail = {
                        let mut flaky = site.flaky.lock().unwrap();
                        match flaky.get_mut(&path) {
                            Some(n) if *n > 0 => {
                                *n -= 1;
                                true
                            }
                            _ => false,
                        }
                    };
                    if fail {
                        let _ = sock.write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 0\r\nContent-Length: 0\r\n\r\n").await;
                        continue;
                    }
                    let data = site.files.lock().unwrap().get(&path).cloned();
                    let Some(data) = data else {
                        let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                        continue;
                    };
                    let (status, body, extra) = match range.as_deref().and_then(|r| r.strip_prefix("bytes=")) {
                        Some(r) => {
                            let (a, b) = r.split_once('-').unwrap_or((r, ""));
                            let start: usize = a.parse().unwrap_or(0);
                            let end: usize = b.parse().unwrap_or(data.len() - 1).min(data.len() - 1);
                            ("206 Partial Content", data[start..=end].to_vec(), format!("Content-Range: bytes {start}-{end}/{}\r\n", data.len()))
                        }
                        None => ("200 OK", data, String::new()),
                    };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}\r\n", body.len());
                    if sock.write_all(head.as_bytes()).await.is_err() || sock.write_all(&body).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    Server { base, site }
}

// ───────────────────────────── builders ─────────────────────────────

fn blob(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| ((i * 31 + seed as usize * 7) % 251) as u8).collect()
}

fn media_playlist(segments: &[&str], extra_head: &str, endlist: bool) -> String {
    let mut s = format!("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:4\n#EXT-X-MEDIA-SEQUENCE:0\n{extra_head}");
    for seg in segments {
        s.push_str(&format!("#EXTINF:4.0,\n{seg}\n"));
    }
    if endlist {
        s.push_str("#EXT-X-ENDLIST\n");
    }
    s
}

fn task(url: &str, dir: &std::path::Path, name: &str, variant: Option<&str>) -> DownloadTask {
    DownloadTask {
        id: "stream".into(),
        url: url.into(),
        final_url: None,
        filename: name.into(),
        destination_path: dir.to_string_lossy().to_string(),
        temp_path: String::new(),
        total_bytes: 0,
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable: true,
        etag: variant.map(str::to_string),
        last_modified: None,
        mime_type: None,
        category: FileCategory::Videos,
        connections: 4,
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
        proxy_mode: Some("direct".into()),
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

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("nexdrop_stream_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn engine() -> DownloadEngine {
    DownloadEngine::new().with_retry_base(Duration::from_millis(20))
}

async fn run(t: DownloadTask) -> Result<DownloadTask, String> {
    let (_tx, rx) = watch::channel(false);
    engine().run_stream(t, rx, Vec::new(), |_| {}).await
}

fn fetch() -> Fetch {
    Fetch::new(create_http_client(Duration::from_secs(10)), Vec::new(), "http://127.0.0.1", None, None, Duration::from_millis(20))
}

fn read_final(dir: &std::path::Path, t: &DownloadTask) -> Vec<u8> {
    std::fs::read(std::path::Path::new(&t.destination_path).join(&t.filename)).unwrap_or_else(|_| panic!("missing output in {dir:?}"))
}

fn no_leftovers(dir: &std::path::Path) {
    let extra: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.contains(".nexpart"))
        .collect();
    assert!(extra.is_empty(), "temporary files left behind: {extra:?}");
}

// ───────────────────────────── HLS ─────────────────────────────

fn hls_site(s: &Server, segs: usize) -> Vec<Vec<u8>> {
    let hd: Vec<Vec<u8>> = (0..segs).map(|i| blob(40_000 + i * 17, i as u8)).collect();
    let sd: Vec<Vec<u8>> = (0..segs).map(|i| blob(10_000 + i * 11, 100 + i as u8)).collect();
    let names = |p: &str| (0..segs).map(|i| format!("{p}{i}.ts")).collect::<Vec<_>>();
    s.put("/show/master.m3u8", "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=1280x720\nhd/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=640x360\nsd/index.m3u8\n");
    s.put("/show/hd/index.m3u8", media_playlist(&names("seg").iter().map(String::as_str).collect::<Vec<_>>(), "", true));
    s.put("/show/sd/index.m3u8", media_playlist(&names("seg").iter().map(String::as_str).collect::<Vec<_>>(), "", true));
    for i in 0..segs {
        s.put(&format!("/show/hd/seg{i}.ts"), hd[i].clone());
        s.put(&format!("/show/sd/seg{i}.ts"), sd[i].clone());
    }
    // returns the HD concatenation first, SD second
    vec![hd.concat(), sd.concat()]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_master_downloads_the_best_quality_in_order() {
    let s = start().await;
    let expect = hls_site(&s, 12);
    let dir = temp_dir("best");
    let done = run(task(&s.url("/show/master.m3u8"), &dir, "show.ts", None)).await.unwrap();
    assert_eq!(done.status, DownloadStatus::Completed);
    assert_eq!(done.filename, "show.ts");
    assert_eq!(read_final(&dir, &done), expect[0], "segments must be joined in order, byte for byte");
    no_leftovers(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_chosen_quality_is_the_one_downloaded() {
    let s = start().await;
    let expect = hls_site(&s, 6);
    let dir = temp_dir("variant");
    let done = run(task(&s.url("/show/master.m3u8"), &dir, "show.ts", Some("500000:640x360"))).await.unwrap();
    assert_eq!(read_final(&dir, &done), expect[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn probe_lists_qualities_best_first() {
    let s = start().await;
    hls_site(&s, 3);
    let p = stream::probe(&fetch(), &s.url("/show/master.m3u8")).await.unwrap();
    assert_eq!(p.kind, "hls");
    assert_eq!(p.title, "show");
    assert_eq!(p.filename, "show.ts");
    assert_eq!(p.variants.iter().map(|v| v.height).collect::<Vec<_>>(), [720, 360]);
    assert_eq!(p.variants[0].id, "2000000:1280x720");
    assert!((p.duration_secs - 12.0).abs() < 1e-9);
    assert!(p.unsupported.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aes_128_segments_are_decrypted() {
    let s = start().await;
    let key = [0x42u8; 16];
    s.put("/enc/key.bin", key.to_vec());
    let plain: Vec<Vec<u8>> = (0..5).map(|i| blob(30_000 + i * 13, 50 + i as u8)).collect();
    let mut names = Vec::new();
    for (i, p) in plain.iter().enumerate() {
        // Segments 0-2 use the playlist's IV, the rest use their sequence number.
        let mut iv = [0u8; 16];
        if i < 3 {
            iv[15] = 0x77;
        } else {
            iv[8..].copy_from_slice(&(i as u64).to_be_bytes());
        }
        let mut buf = p.clone();
        buf.resize(p.len() + 16, 0);
        let enc = cbc::Encryptor::<aes::Aes128>::new((&key).into(), (&iv).into()).encrypt_padded_mut::<Pkcs7>(&mut buf, p.len()).unwrap().to_vec();
        s.put(&format!("/enc/s{i}.ts"), enc);
        names.push(format!("s{i}.ts"));
    }
    let mut pl = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x00000000000000000000000000000077\n");
    for (i, n) in names.iter().enumerate() {
        if i == 3 {
            pl.push_str("#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n");
        }
        pl.push_str(&format!("#EXTINF:4,\n{n}\n"));
    }
    pl.push_str("#EXT-X-ENDLIST\n");
    s.put("/enc/index.m3u8", pl);

    let dir = temp_dir("aes");
    let done = run(task(&s.url("/enc/index.m3u8"), &dir, "enc.ts", None)).await.unwrap();
    assert_eq!(read_final(&dir, &done), plain.concat());
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn byte_range_segments_from_one_file() {
    let s = start().await;
    let file = blob(90_000, 9);
    s.put("/br/all.ts", file.clone());
    let pl = "#EXTM3U\n#EXT-X-VERSION:4\n#EXTINF:4,\n#EXT-X-BYTERANGE:30000@0\nall.ts\n#EXTINF:4,\n#EXT-X-BYTERANGE:30000\nall.ts\n#EXTINF:4,\n#EXT-X-BYTERANGE:30000\nall.ts\n#EXT-X-ENDLIST\n";
    s.put("/br/index.m3u8", pl);
    let dir = temp_dir("range");
    let done = run(task(&s.url("/br/index.m3u8"), &dir, "br.ts", None)).await.unwrap();
    assert_eq!(read_final(&dir, &done), file);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transient_errors_are_retried_and_missing_segments_fail_clearly() {
    let s = start().await;
    let expect = hls_site(&s, 6);
    s.site.flaky.lock().unwrap().insert("/show/hd/seg2.ts".into(), 3);
    let dir = temp_dir("flaky");
    let done = run(task(&s.url("/show/master.m3u8"), &dir, "show.ts", None)).await.unwrap();
    assert_eq!(read_final(&dir, &done), expect[0]);

    // A segment that is gone for good (404) fails the download with a message that names the problem.
    s.site.files.lock().unwrap().remove("/show/hd/seg4.ts");
    let dir2 = temp_dir("missing");
    let err = run(task(&s.url("/show/master.m3u8"), &dir2, "show.ts", None)).await.unwrap_err();
    assert!(err.contains("404") && err.contains("seg4.ts"), "unexpected error: {err}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_and_unsupported_streams_are_refused_with_a_reason() {
    let s = start().await;
    s.put("/live/index.m3u8", media_playlist(&["a.ts", "b.ts"], "", false));
    s.put("/sample/index.m3u8", "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://k\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n");
    s.put("/junk/index.m3u8", "<html>Not a playlist</html>");

    let live = stream::probe(&fetch(), &s.url("/live/index.m3u8")).await.unwrap();
    assert!(live.unsupported.as_deref().unwrap().contains("Live"));
    let sample = stream::probe(&fetch(), &s.url("/sample/index.m3u8")).await.unwrap();
    assert!(sample.unsupported.as_deref().unwrap().contains("SAMPLE-AES"));
    assert!(stream::probe(&fetch(), &s.url("/junk/index.m3u8")).await.is_err());

    let dir = temp_dir("live");
    let err = run(task(&s.url("/live/index.m3u8"), &dir, "live.ts", None)).await.unwrap_err();
    assert!(err.contains("Live"), "unexpected error: {err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pausing_keeps_whole_segments_and_resuming_finishes_byte_exact() {
    let s = start().await;
    let expect = hls_site(&s, 24);
    *s.site.slow.lock().unwrap() = Some(("hd/seg".into(), 300));
    let dir = temp_dir("pause");
    let t = task(&s.url("/show/master.m3u8"), &dir, "show.ts", None);

    let (tx, rx) = watch::channel(false);
    let e = engine();
    let first = {
        let t = t.clone();
        tokio::spawn(async move { e.run_stream(t, rx, Vec::new(), |_| {}).await })
    };
    tokio::time::sleep(Duration::from_millis(600)).await;
    tx.send(true).unwrap();
    let paused = first.await.unwrap().unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(paused.downloaded_bytes > 0, "some segments were already saved");
    assert!(!dir.join("show.ts").exists(), "nothing is finished yet");

    // Resume: only the remaining segments are fetched.
    let hits_before = s.site.hits.load(Ordering::SeqCst);
    *s.site.slow.lock().unwrap() = None;
    let done = run(t).await.unwrap();
    assert_eq!(read_final(&dir, &done), expect[0]);
    let refetched = s.site.hits.load(Ordering::SeqCst) - hits_before;
    assert!(refetched < 3 + 24, "resume should skip finished segments, but made {refetched} requests");
    no_leftovers(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

// ───────────────────────────── fragmented MP4 + DASH ─────────────────────────────

fn mp4_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(payload.len() as u32 + 8).to_be_bytes());
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn mp4_init(track_id: u32, timescale: u32) -> Vec<u8> {
    let mut mvhd = vec![0u8; 100];
    mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[96..100].copy_from_slice(&(track_id + 1).to_be_bytes());
    let mut tkhd = vec![0u8; 84];
    tkhd[12..16].copy_from_slice(&track_id.to_be_bytes());
    let mut mdhd = vec![0u8; 24];
    mdhd[12..16].copy_from_slice(&timescale.to_be_bytes());
    let trak = mp4_box(b"trak", &[mp4_box(b"tkhd", &tkhd), mp4_box(b"mdia", &mp4_box(b"mdhd", &mdhd))].concat());
    let mut trex = vec![0u8; 24];
    trex[4..8].copy_from_slice(&track_id.to_be_bytes());
    let moov = mp4_box(b"moov", &[mp4_box(b"mvhd", &mvhd), trak, mp4_box(b"mvex", &mp4_box(b"trex", &trex))].concat());
    [mp4_box(b"ftyp", b"isomiso6"), moov].concat()
}

fn mp4_fragment(track_id: u32, time: u64, payload: &[u8]) -> Vec<u8> {
    let tfhd = mp4_box(b"tfhd", &[&[0u8, 2, 0, 0][..], &track_id.to_be_bytes()].concat());
    let mut tfdt = vec![1u8, 0, 0, 0];
    tfdt.extend_from_slice(&time.to_be_bytes());
    let traf = mp4_box(b"traf", &[tfhd, mp4_box(b"tfdt", &tfdt)].concat());
    let moof = mp4_box(b"moof", &[mp4_box(b"mfhd", &[0u8; 8]), traf].concat());
    [moof, mp4_box(b"mdat", payload)].concat()
}

/// Every top-level box kind, and the payloads of all `mdat` boxes.
fn top_level(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        out.push((String::from_utf8_lossy(&data[pos + 4..pos + 8]).to_string(), data[pos + 8..pos + size].to_vec()));
        pos += size;
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hls_fmp4_with_separate_audio_becomes_one_mp4() {
    let s = start().await;
    s.put("/f/master.m3u8", "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"en\",DEFAULT=YES,URI=\"audio/index.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1280x720,AUDIO=\"a\"\nvideo/index.m3u8\n");
    s.put("/f/video/init.mp4", mp4_init(1, 1000));
    s.put("/f/audio/init.mp4", mp4_init(1, 48000));
    let mut v_names = Vec::new();
    let mut a_names = Vec::new();
    for i in 0..4u64 {
        s.put(&format!("/f/video/s{i}.m4s"), mp4_fragment(1, i * 4000, format!("VIDEO{i}").as_bytes()));
        s.put(&format!("/f/audio/s{i}.m4s"), mp4_fragment(1, i * 4 * 48000, format!("AUDIO{i}").as_bytes()));
        v_names.push(format!("s{i}.m4s"));
        a_names.push(format!("s{i}.m4s"));
    }
    s.put("/f/video/index.m3u8", media_playlist(&v_names.iter().map(String::as_str).collect::<Vec<_>>(), "#EXT-X-MAP:URI=\"init.mp4\"\n", true));
    s.put("/f/audio/index.m3u8", media_playlist(&a_names.iter().map(String::as_str).collect::<Vec<_>>(), "#EXT-X-MAP:URI=\"init.mp4\"\n", true));

    let probe = stream::probe(&fetch(), &s.url("/f/master.m3u8")).await.unwrap();
    assert_eq!(probe.filename, "master.mp4".replace("master", "f"));
    assert!(probe.variants[0].separate_audio);

    let dir = temp_dir("fmp4");
    let done = run(task(&s.url("/f/master.m3u8"), &dir, "f.mp4", None)).await.unwrap();
    assert_eq!(done.filename, "f.mp4");
    let out = read_final(&dir, &done);
    let boxes = top_level(&out);
    let kinds: Vec<&str> = boxes.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(&kinds[..2], ["ftyp", "moov"]);
    assert_eq!(kinds.iter().filter(|k| **k == "moof").count(), 8, "four video and four audio fragments");
    let payloads: Vec<String> = boxes.iter().filter(|(k, _)| k == "mdat").map(|(_, p)| String::from_utf8_lossy(p).to_string()).collect();
    assert_eq!(payloads, ["VIDEO0", "AUDIO0", "VIDEO1", "AUDIO1", "VIDEO2", "AUDIO2", "VIDEO3", "AUDIO3"], "interleaved by time");
    no_leftovers(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dash_video_and_audio_are_joined() {
    let s = start().await;
    s.put(
        "/d/manifest.mpd",
        r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT12S">
 <Period>
  <AdaptationSet mimeType="video/mp4">
   <SegmentTemplate timescale="1000" duration="4000" startNumber="1" media="v$RepresentationID$/$Number$.m4s" initialization="v$RepresentationID$/init.mp4"/>
   <Representation id="hi" bandwidth="2000000" width="1920" height="1080"/>
   <Representation id="lo" bandwidth="400000" width="640" height="360"/>
  </AdaptationSet>
  <AdaptationSet mimeType="audio/mp4" lang="en">
   <SegmentTemplate timescale="48000" duration="192000" startNumber="1" media="a/$Number$.m4s" initialization="a/init.mp4"/>
   <Representation id="aud" bandwidth="128000"/>
  </AdaptationSet>
 </Period>
</MPD>"#,
    );
    for rep in ["hi", "lo"] {
        s.put(&format!("/d/v{rep}/init.mp4"), mp4_init(1, 1000));
        for i in 0..3u64 {
            s.put(&format!("/d/v{rep}/{}.m4s", i + 1), mp4_fragment(1, i * 4000, format!("{rep}-v{i}").as_bytes()));
        }
    }
    s.put("/d/a/init.mp4", mp4_init(1, 48000));
    for i in 0..3u64 {
        s.put(&format!("/d/a/{}.m4s", i + 1), mp4_fragment(1, i * 192000, format!("aud{i}").as_bytes()));
    }

    let probe = stream::probe(&fetch(), &s.url("/d/manifest.mpd")).await.unwrap();
    assert_eq!(probe.kind, "dash");
    assert_eq!(probe.variants.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["hi", "lo"]);
    assert!(probe.variants[0].separate_audio);

    let dir = temp_dir("dash");
    let done = run(task(&s.url("/d/manifest.mpd"), &dir, "d.mp4", Some("lo"))).await.unwrap();
    let boxes = top_level(&read_final(&dir, &done));
    let payloads: Vec<String> = boxes.iter().filter(|(k, _)| k == "mdat").map(|(_, p)| String::from_utf8_lossy(p).to_string()).collect();
    assert_eq!(payloads, ["lo-v0", "aud0", "lo-v1", "aud1", "lo-v2", "aud2"], "the chosen quality, with the audio joined in");
    no_leftovers(&dir);
    let _ = std::fs::remove_dir_all(&dir);
}
