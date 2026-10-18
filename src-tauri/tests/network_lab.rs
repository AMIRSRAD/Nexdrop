//! Network lab: realistic speed and failure scenarios for the download engine.
//!
//!   cargo test --release --test network_lab -- --ignored --nocapture --test-threads=1
//!
//! A local HTTP/1.1 server (keep-alive, HEAD, ranges, redirects) that can cap
//! each connection's speed, add latency, cut responses, return 503s, stall,
//! go offline, ignore Range, and hand out signed links that expire. Each
//! scenario checks the downloaded bytes exactly and prints a `LAB` line.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use nexdrop_lib::db::models::{DownloadStatus, DownloadTask, FileCategory};
use nexdrop_lib::engine::calculate_segments_with_chunk_size;
use nexdrop_lib::engine::client::create_http_client;
use nexdrop_lib::engine::downloader::DownloadEngine;
use nexdrop_lib::engine::probe::{probe_url_with, ProbeOptions};

const MIB: usize = 1024 * 1024;

// ───────────────────────────── lab server ─────────────────────────────

#[derive(Clone, Default)]
struct Behavior {
    /// Speed cap for each TCP connection (bytes/s); `None` = as fast as possible.
    per_conn_bps: Option<u64>,
    /// Delay before every response, like a round trip to a distant server.
    latency: Duration,
    /// Answer every GET with 200 and the whole file.
    ignore_range: bool,
    /// HEAD answers without `Accept-Ranges` (common on real servers that do support ranges).
    head_hides_ranges: bool,
    /// HEAD isn't allowed at all (405).
    no_head: bool,
    /// Per-mille chance that a response body is cut off partway.
    drop_permille: u64,
    /// Per-mille chance of a 503 instead of data.
    busy_permille: u64,
    /// The first response covering this offset stops sending (but keeps the socket open).
    stall_at: Option<u64>,
    /// Starting this long after the first request, the server is unreachable for this long.
    outage: Option<(Duration, Duration)>,
    /// `/file` redirects to `/real?sig=N`.
    redirect: bool,
    /// A signed link works for this many requests, then returns 403 (expired).
    sig_uses: Option<usize>,
}

#[derive(Default)]
struct Stats {
    requests: AtomicUsize,
    redirects: AtomicUsize,
    open_conns: AtomicUsize,
    max_open_conns: AtomicUsize,
    full_body_bytes: AtomicU64,
}

struct Lab {
    base: String,
    stats: Arc<Stats>,
}

struct ServerState {
    payload: Arc<Vec<u8>>,
    b: Behavior,
    stats: Arc<Stats>,
    started: Mutex<Option<Instant>>,
    stalled: AtomicBool,
    rng: AtomicU64,
    sig_counter: AtomicUsize,
    sig_uses: Mutex<HashMap<usize, usize>>,
}
use parking_lot::Mutex;

impl ServerState {
    fn roll(&self) -> u64 {
        // xorshift-ish, deterministic per run
        let mut x = self.rng.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x % 1000
    }
    fn offline(&self) -> bool {
        let Some((after, len)) = self.b.outage else { return false };
        let Some(t0) = *self.started.lock() else { return false };
        let e = t0.elapsed();
        e >= after && e < after + len
    }
}

fn payload(len: usize) -> Arc<Vec<u8>> {
    // Position-dependent, so any byte written at the wrong offset is caught.
    Arc::new((0..len).map(|i| ((i * 31 + i / 4099) % 251) as u8).collect())
}

async fn start_lab(len: usize, b: Behavior) -> (Lab, Arc<Vec<u8>>) {
    let data = payload(len);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let stats = Arc::new(Stats::default());
    let state = Arc::new(ServerState {
        payload: data.clone(),
        b,
        stats: stats.clone(),
        started: Mutex::new(None),
        stalled: AtomicBool::new(false),
        rng: AtomicU64::new(42),
        sig_counter: AtomicUsize::new(0),
        sig_uses: Mutex::new(HashMap::new()),
    });
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { break };
            let state = state.clone();
            tokio::spawn(async move {
                let now = state.stats.open_conns.fetch_add(1, Ordering::SeqCst) + 1;
                state.stats.max_open_conns.fetch_max(now, Ordering::SeqCst);
                let _ = serve_conn(socket, state.clone()).await;
                state.stats.open_conns.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    (Lab { base: format!("http://{addr}"), stats }, data)
}

async fn read_head(socket: &mut TcpStream, buf: &mut Vec<u8>) -> Option<String> {
    loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            buf.drain(..pos + 4);
            return Some(head);
        }
        let mut tmp = [0u8; 4096];
        let n = socket.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

async fn serve_conn(mut socket: TcpStream, st: Arc<ServerState>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    loop {
        let Some(head) = read_head(&mut socket, &mut buf).await else { return Ok(()) };
        st.started.lock().get_or_insert_with(Instant::now);
        st.stats.requests.fetch_add(1, Ordering::SeqCst);
        if st.offline() {
            return Ok(()); // hang up: looks like a dead network to the client
        }
        if !st.b.latency.is_zero() {
            tokio::time::sleep(st.b.latency).await;
        }

        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or("");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("GET").to_string();
        let path = parts.next().unwrap_or("/").to_string();
        let headers: HashMap<String, String> = lines
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();

        // Redirects and signed links
        if st.b.redirect && path.starts_with("/file") {
            st.stats.redirects.fetch_add(1, Ordering::SeqCst);
            let sig = st.sig_counter.fetch_add(1, Ordering::SeqCst);
            let resp = format!("HTTP/1.1 302 Found\r\nLocation: /real?sig={sig}\r\nContent-Length: 0\r\n\r\n");
            socket.write_all(resp.as_bytes()).await?;
            continue;
        }
        if let (Some(limit), Some(sig)) = (st.b.sig_uses, path.split("sig=").nth(1).and_then(|s| s.parse::<usize>().ok())) {
            let used = {
                let mut m = st.sig_uses.lock();
                let e = m.entry(sig).or_insert(0);
                *e += 1;
                *e
            };
            if used > limit {
                socket.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 7\r\n\r\nexpired").await?;
                continue;
            }
        }

        let total = st.payload.len();
        if method == "HEAD" {
            if st.b.no_head {
                socket.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n").await?;
                continue;
            }
            let ranges = if st.b.head_hides_ranges || st.b.ignore_range { "" } else { "Accept-Ranges: bytes\r\n" };
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\n{ranges}ETag: \"lab\"\r\n\r\n");
            socket.write_all(resp.as_bytes()).await?;
            continue;
        }

        if st.roll() < st.b.busy_permille {
            socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n").await?;
            continue;
        }

        let range = headers.get("range").and_then(|r| r.strip_prefix("bytes=")).map(|r| {
            let (s, e) = r.split_once('-').unwrap_or((r, ""));
            let s: usize = s.trim().parse().unwrap_or(0);
            let e: usize = e.trim().parse().unwrap_or(total - 1).min(total - 1);
            (s, e)
        });
        let (start, end, head) = match range {
            Some((s, e)) if !st.b.ignore_range => (
                s,
                e,
                format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {s}-{e}/{total}\r\nContent-Length: {}\r\nETag: \"lab\"\r\n\r\n",
                    e - s + 1
                ),
            ),
            _ => (0, total - 1, format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nETag: \"lab\"\r\n\r\n")),
        };
        if start == 0 && end == total - 1 {
            st.stats.full_body_bytes.fetch_add(0, Ordering::Relaxed);
        }
        socket.write_all(head.as_bytes()).await?;

        let body = &st.payload[start..=end];
        let cut_at = if st.roll() < st.b.drop_permille { Some(body.len() * st.roll() as usize / 1000) } else { None };
        let stall_here = st
            .b
            .stall_at
            .map(|o| (start as u64..=end as u64).contains(&o) && !st.stalled.swap(true, Ordering::SeqCst))
            .unwrap_or(false);

        let piece = 64 * 1024;
        let mut sent = 0usize;
        let t0 = Instant::now();
        while sent < body.len() {
            if st.offline() {
                return Ok(());
            }
            if let Some(cut) = cut_at {
                if sent >= cut {
                    return Ok(()); // connection drops mid-body
                }
            }
            if stall_here && sent >= body.len() / 4 {
                // Keep the socket open but never send another byte.
                tokio::time::sleep(Duration::from_secs(3600)).await;
                return Ok(());
            }
            let n = piece.min(body.len() - sent);
            socket.write_all(&body[sent..sent + n]).await?;
            sent += n;
            if let Some(bps) = st.b.per_conn_bps {
                let due = Duration::from_secs_f64(sent as f64 / bps as f64);
                let el = t0.elapsed();
                if due > el {
                    tokio::time::sleep(due - el).await;
                }
            }
        }
    }
}

// ───────────────────────────── harness ─────────────────────────────

fn task(url: &str, final_url: &str, total: usize, connections: usize, resumable: bool) -> DownloadTask {
    let dir = std::env::temp_dir().join(format!("nexdrop_lab_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let segments = calculate_segments_with_chunk_size(total as i64, connections, MIB as u64);
    DownloadTask {
        id: "lab".into(),
        url: url.into(),
        final_url: Some(final_url.into()),
        filename: "lab.bin".into(),
        destination_path: dir.to_string_lossy().to_string(),
        temp_path: String::new(),
        total_bytes: if resumable || total > 0 { total as i64 } else { -1 },
        downloaded_bytes: 0,
        status: DownloadStatus::Downloading,
        resumable,
        etag: Some("\"lab\"".into()),
        last_modified: None,
        mime_type: None,
        category: FileCategory::Other,
        connections,
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
        proxy_mode: Some("direct".into()),
        custom_proxy_url: None,
        antivirus_status: None,
        expected_checksum: None,
        custom_headers: Default::default(),
        kind: "http".into(),
        info_hash: None,
        uploaded_bytes: 0,
        peers: 0,
        seeding: false,
        torrent_files: vec![],
    }
}

struct Outcome {
    ok: bool,
    secs: f64,
    error: Option<String>,
}

/// Probes `url` the way the app does, then downloads it, then checks every byte.
async fn run(lab: &Lab, data: &[u8], path: &str, connections: usize, limit: Duration) -> Outcome {
    let url = format!("{}{path}", lab.base);
    let client = create_http_client(Duration::from_secs(15));
    let started = Instant::now();
    let probe = match probe_url_with(&client, &url, &ProbeOptions::default()).await {
        Ok(p) => p,
        Err(e) => return Outcome { ok: false, secs: started.elapsed().as_secs_f64(), error: Some(format!("probe: {e}")) },
    };
    let conns = if probe.resumable && probe.total_bytes > 0 { connections } else { 1 };
    let mut t = task(&url, &probe.final_url, data.len(), conns, probe.resumable && probe.total_bytes > 0);
    t.total_bytes = probe.total_bytes;
    let dir = t.destination_path.clone();
    let (_tx, rx) = watch::channel(false);
    let engine = DownloadEngine::new();
    let res = tokio::time::timeout(limit, engine.run_download(t, rx, |_| {})).await;
    let secs = started.elapsed().as_secs_f64();
    let out = match res {
        Err(_) => Outcome { ok: false, secs, error: Some(format!("timed out after {}s", limit.as_secs())) },
        Ok(Err(e)) => Outcome { ok: false, secs, error: Some(e) },
        Ok(Ok(done)) => {
            let file = std::path::Path::new(&done.destination_path).join(&done.filename);
            let got = std::fs::read(&file).unwrap_or_default();
            if got.len() != data.len() {
                Outcome { ok: false, secs, error: Some(format!("size {} != {}", got.len(), data.len())) }
            } else if got != data {
                let at = got.iter().zip(data).position(|(a, b)| a != b).unwrap_or(0);
                Outcome { ok: false, secs, error: Some(format!("corrupt from byte {at}")) }
            } else {
                Outcome { ok: true, secs, error: None }
            }
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    out
}

fn report(name: &str, mib: usize, o: &Outcome, extra: &str) {
    if o.ok {
        println!("LAB {name:<44} OK   {:>6.2}s  {:>7.1} MiB/s  {extra}", o.secs, mib as f64 / o.secs);
    } else {
        println!("LAB {name:<44} FAIL {:>6.2}s  {}  {extra}", o.secs, o.error.as_deref().unwrap_or(""));
    }
}

// ───────────────────────────── scenarios ─────────────────────────────

/// Internet-like link: every connection is capped at 2 MiB/s with 40 ms latency.
/// More connections should mean proportionally more speed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_speed_scaling() {
    for conns in [1usize, 4, 8, 16] {
        let mib = (6 * conns).max(24);
        let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(2 * MIB as u64), latency: Duration::from_millis(40), ..Default::default() }).await;
        let o = run(&lab, &data, "/f.bin", conns, Duration::from_secs(120)).await;
        report(&format!("speed: {conns} connection(s) @2MiB/s each"), mib, &o, &format!("max open conns {}", lab.stats.max_open_conns.load(Ordering::SeqCst)));
        assert!(o.ok);
    }
}

/// Real servers often support ranges but don't say so in their HEAD reply.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_head_hides_range_support() {
    let mib = 32;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(2 * MIB as u64), head_hides_ranges: true, ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(120)).await;
    let conns = lab.stats.max_open_conns.load(Ordering::SeqCst);
    report("probe: HEAD hides Accept-Ranges (8 conns)", mib, &o, &format!("max open conns {conns}"));
    assert!(o.ok);
    assert!(conns >= 4, "range support wasn't detected, so only {conns} connection(s) were used");
}

/// Some servers refuse HEAD entirely.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_no_head() {
    let mib = 16;
    let (lab, data) = start_lab(mib * MIB, Behavior { no_head: true, ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(60)).await;
    report("probe: HEAD not allowed", mib, &o, "");
    assert!(o.ok);
}

/// A server that advertises ranges but then ignores them.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_server_ignores_range() {
    let mib = 16;
    let (lab, data) = start_lab(mib * MIB, Behavior { ignore_range: true, head_hides_ranges: false, ..Default::default() }).await;
    // HEAD claims ranges (ignore_range hides it, so force the claim by probing a normal HEAD first)
    let mut t = task(&format!("{}/f.bin", lab.base), &format!("{}/f.bin", lab.base), data.len(), 8, true);
    t.total_bytes = data.len() as i64;
    let dir = t.destination_path.clone();
    let (_tx, rx) = watch::channel(false);
    let started = Instant::now();
    let res = tokio::time::timeout(Duration::from_secs(60), DownloadEngine::new().run_download(t, rx, |_| {})).await;
    let o = match res {
        Ok(Ok(done)) => {
            let got = std::fs::read(std::path::Path::new(&done.destination_path).join(&done.filename)).unwrap_or_default();
            Outcome { ok: got == *data, secs: started.elapsed().as_secs_f64(), error: (got != *data).then(|| "bytes differ".into()) }
        }
        Ok(Err(e)) => Outcome { ok: false, secs: started.elapsed().as_secs_f64(), error: Some(e) },
        Err(_) => Outcome { ok: false, secs: 60.0, error: Some("timeout".into()) },
    };
    let _ = std::fs::remove_dir_all(&dir);
    report("server ignores Range with 8 conns", mib, &o, "");
    assert!(o.ok);
}

/// Link that redirects (like GitHub releases / CDNs) with a 150 ms redirect hop.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_redirect_overhead() {
    let mib = 48;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(4 * MIB as u64), latency: Duration::from_millis(150), redirect: true, ..Default::default() }).await;
    let o = run(&lab, &data, "/file", 8, Duration::from_secs(120)).await;
    report("redirected link (150 ms per hop)", mib, &o, &format!("redirect hops {}", lab.stats.redirects.load(Ordering::SeqCst)));
    assert!(o.ok);
}

/// Signed CDN link that expires after a few uses: the engine must fetch a fresh one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_signed_link_expires() {
    let mib = 32;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(4 * MIB as u64), redirect: true, sig_uses: Some(3), ..Default::default() }).await;
    let o = run(&lab, &data, "/file", 8, Duration::from_secs(120)).await;
    report("signed link expires after 3 uses", mib, &o, &format!("redirect hops {}", lab.stats.redirects.load(Ordering::SeqCst)));
    assert!(o.ok);
}

/// Flaky server: 40% of responses are cut off, 15% are 503 busy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_flaky_server() {
    let mib = 48;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(4 * MIB as u64), drop_permille: 400, busy_permille: 150, ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(180)).await;
    report("flaky: 40% cut off, 15% busy", mib, &o, &format!("requests {}", lab.stats.requests.load(Ordering::SeqCst)));
    assert!(o.ok);
}

/// One connection stops sending without closing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_stalled_connection() {
    let mib = 32;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(4 * MIB as u64), stall_at: Some(1024), ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(120)).await;
    report("one connection stalls forever", mib, &o, "");
    assert!(o.ok);
}

/// Network goes away for 90 s mid-download (Wi-Fi drop, VPN reconnect, router reboot).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_network_outage() {
    let mib = 32;
    let (lab, data) = start_lab(mib * MIB, Behavior { per_conn_bps: Some(2 * MIB as u64), outage: Some((Duration::from_secs(1), Duration::from_secs(90))), ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(240)).await;
    report("network outage of 90 s", mib, &o, "");
    assert!(o.ok);
}

/// Server without range support whose connection drops partway.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_single_stream_drop() {
    let mib = 16;
    let (lab, data) = start_lab(mib * MIB, Behavior { ignore_range: true, drop_permille: 500, ..Default::default() }).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(120)).await;
    report("no ranges + 50% of connections drop", mib, &o, &format!("requests {}", lab.stats.requests.load(Ordering::SeqCst)));
    assert!(o.ok);
}

/// Raw engine overhead on loopback, no caps (disk + bookkeeping bound).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn lab_loopback_throughput() {
    let mib = 512;
    let (lab, data) = start_lab(mib * MIB, Behavior::default()).await;
    let o = run(&lab, &data, "/f.bin", 8, Duration::from_secs(120)).await;
    report("loopback, uncapped, 8 conns", mib, &o, "");
    assert!(o.ok);
}
