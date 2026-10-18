//! Streaming video: HLS (`.m3u8`) and MPEG-DASH (`.mpd`).
//!
//! A stream is a list of short segments. Nexdrop fetches them in parallel,
//! decrypts AES-128 protected ones, appends them in order to one file and, when
//! a quality has separate video and audio tracks, joins the two into a single
//! MP4 without re-encoding. Downloads resume from the last whole segment.

pub mod dash;
pub mod fmp4;
pub mod hls;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use futures_util::StreamExt;
use reqwest::header::{HeaderName, HeaderValue, RANGE, RETRY_AFTER, USER_AGENT};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::{watch, Mutex as AsyncMutex};
use url::Url;

use crate::db::models::{DownloadProgressEvent, DownloadStatus, DownloadTask};
use crate::engine::auth::build_header_list;
use crate::engine::downloader::{
    backoff_delay, check_disk_space, file_name_of, finish_task, paused, same_host, wait_paused, DownloadEngine,
};
use crate::engine::probe::{reserve_unique_path, sanitize_filename};
use crate::engine::speed::RateLimiter;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Nexdrop/1.0";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const SIDECAR_INTERVAL: Duration = Duration::from_secs(2);
const SEGMENT_ATTEMPTS: u32 = 6;
const MAX_CONNECTIONS: usize = 32;

// ───────────────────────────── model ─────────────────────────────

#[derive(Debug, Clone)]
pub struct KeyInfo {
    pub uri: String,
    /// `None`: the IV is the segment's sequence number.
    pub iv: Option<[u8; 16]>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub url: String,
    /// (offset, length) within the resource.
    pub range: Option<(u64, u64)>,
    pub duration: f64,
    pub key: Option<Arc<KeyInfo>>,
    pub seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Ts,
    Fmp4,
    Aac,
    Other,
}

#[derive(Debug, Clone)]
pub struct Track {
    /// "video", "audio" or "muxed" (video with its audio inside).
    pub role: &'static str,
    pub init: Option<Segment>,
    pub segments: Vec<Segment>,
    pub duration: f64,
    pub bandwidth: u64,
    pub container: Container,
}

impl Track {
    fn total_pieces(&self) -> usize {
        self.segments.len() + self.init.is_some() as usize
    }
    fn estimated_bytes(&self) -> u64 {
        if self.bandwidth > 0 && self.duration > 0.0 {
            (self.bandwidth as f64 * self.duration / 8.0) as u64
        } else {
            self.segments.len() as u64 * 1024 * 1024
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub title: String,
    pub kind: &'static str,
    pub duration: f64,
    pub tracks: Vec<Track>,
    /// Id of the quality that was picked (as listed by `probe`); `None` for a single-quality stream.
    pub variant: Option<String>,
}

impl Resolved {
    pub fn estimated_bytes(&self) -> u64 {
        self.tracks.iter().map(Track::estimated_bytes).sum()
    }

    /// Both tracks are fragmented MP4: they can be joined into one file.
    fn can_join(&self) -> bool {
        self.tracks.len() == 2 && self.tracks.iter().all(|t| t.container == Container::Fmp4 && t.init.is_some())
    }

    /// File extension of the finished download.
    pub fn extension(&self) -> &'static str {
        let main = &self.tracks[0];
        if self.can_join() {
            return "mp4";
        }
        match (main.container, main.role) {
            (Container::Ts, _) => "ts",
            (Container::Aac, _) => "aac",
            (Container::Fmp4, "audio") => "m4a",
            _ => "mp4",
        }
    }
}

/// One selectable quality, as shown in the Add dialog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamVariantInfo {
    pub id: String,
    pub label: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub separate_audio: bool,
    pub codecs: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamProbe {
    pub url: String,
    /// "hls" or "dash"
    pub kind: String,
    pub title: String,
    pub filename: String,
    pub duration_secs: f64,
    pub variants: Vec<StreamVariantInfo>,
    /// Why this stream can't be downloaded (live, DRM...), if it can't.
    pub unsupported: Option<String>,
}

// ───────────────────────────── requests ─────────────────────────────

/// How to talk to the stream's servers.
pub struct Fetch {
    client: reqwest::Client,
    /// Cookies/logins: only sent to the host the stream was opened from.
    headers: Vec<(HeaderName, HeaderValue)>,
    origin: String,
    referer: Option<String>,
    user_agent: String,
    retry_base: Duration,
    keys: AsyncMutex<HashMap<String, [u8; 16]>>,
}

impl Fetch {
    pub fn new(
        client: reqwest::Client,
        headers: Vec<(HeaderName, HeaderValue)>,
        origin: &str,
        referer: Option<String>,
        user_agent: Option<String>,
        retry_base: Duration,
    ) -> Self {
        Self {
            client,
            headers,
            origin: origin.to_string(),
            referer,
            user_agent: user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string()),
            retry_base,
            keys: AsyncMutex::new(HashMap::new()),
        }
    }

    fn get(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.get(url).header(USER_AGENT, &self.user_agent);
        if let Some(r) = &self.referer {
            req = req.header("Referer", r);
        }
        if same_host(url, &self.origin) {
            for (n, v) in &self.headers {
                req = req.header(n.clone(), v.clone());
            }
        }
        req
    }

    /// A playlist or manifest (small text), retried a few times.
    pub async fn text(&self, url: &str) -> Result<String, String> {
        let mut last = String::new();
        for attempt in 1..=3u32 {
            match self.get(url).send().await {
                Ok(r) if r.status().is_success() => {
                    return r.text().await.map_err(|e| format!("Couldn't read the playlist: {e}"));
                }
                Ok(r) if r.status().is_client_error() && r.status() != StatusCode::TOO_MANY_REQUESTS => {
                    return Err(format!("The server refused the playlist (HTTP {})", r.status()));
                }
                Ok(r) => last = format!("The server answered HTTP {}", r.status()),
                Err(e) => last = format!("Couldn't reach the server: {e}"),
            }
            tokio::time::sleep(backoff_delay(self.retry_base, attempt, None)).await;
        }
        Err(last)
    }

    async fn key(&self, uri: &str) -> Result<[u8; 16], String> {
        if let Some(k) = self.keys.lock().await.get(uri) {
            return Ok(*k);
        }
        let bytes = self.fetch_once(uri, None).await?;
        let key: [u8; 16] = bytes.as_ref().try_into().map_err(|_| "The stream's encryption key has an unexpected size".to_string())?;
        self.keys.lock().await.insert(uri.to_string(), key);
        Ok(key)
    }

    async fn fetch_once(&self, url: &str, range: Option<(u64, u64)>) -> Result<bytes::Bytes, String> {
        let mut req = self.get(url);
        if let Some((start, len)) = range {
            req = req.header(RANGE, format!("bytes={}-{}", start, start + len.saturating_sub(1)));
        }
        let resp = req.send().await.map_err(|e| format!("Request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }
        let body = resp.bytes().await.map_err(|e| format!("Connection error: {e}"))?;
        // A server that ignores Range sends the whole resource: cut out our part.
        if let (Some((start, len)), StatusCode::OK) = (range, status) {
            let (s, e) = (start as usize, (start + len) as usize);
            return body.get(s..e).map(bytes::Bytes::copy_from_slice).ok_or_else(|| "The server ignored the byte range".to_string());
        }
        Ok(body)
    }

    /// A segment, fully downloaded, retried, and decrypted.
    async fn segment(&self, seg: &Segment) -> Result<Vec<u8>, String> {
        let mut failures = 0u32;
        let data = loop {
            let mut req = self.get(&seg.url);
            if let Some((start, len)) = seg.range {
                req = req.header(RANGE, format!("bytes={}-{}", start, start + len.saturating_sub(1)));
            }
            let outcome: Result<Vec<u8>, (String, bool, Option<Duration>)> = match req.send().await {
                Err(e) => Err((format!("Request failed: {e}"), true, None)),
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        match resp.bytes().await {
                            Err(e) => Err((format!("Connection error: {e}"), true, None)),
                            Ok(body) => {
                                if let (Some((start, len)), StatusCode::OK) = (seg.range, status) {
                                    body.get(start as usize..(start + len) as usize)
                                        .map(|b| b.to_vec())
                                        .ok_or_else(|| ("The server ignored the byte range".to_string(), false, None))
                                } else {
                                    Ok(body.to_vec())
                                }
                            }
                        }
                    } else {
                        let retry_after = resp
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse::<u64>().ok())
                            .map(Duration::from_secs);
                        let transient = status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::REQUEST_TIMEOUT;
                        Err((format!("HTTP {status}"), transient, retry_after))
                    }
                }
            };
            match outcome {
                Ok(data) => break data,
                Err((msg, transient, retry_after)) => {
                    failures += 1;
                    if !transient || failures >= SEGMENT_ATTEMPTS {
                        return Err(format!("{msg} while fetching a video segment ({})", short_url(&seg.url)));
                    }
                    tokio::time::sleep(backoff_delay(self.retry_base, failures, retry_after)).await;
                }
            }
        };

        match &seg.key {
            None => Ok(data),
            Some(info) => {
                let key = self.key(&info.uri).await?;
                let iv = info.iv.unwrap_or_else(|| {
                    let mut iv = [0u8; 16];
                    iv[8..].copy_from_slice(&seg.seq.to_be_bytes());
                    iv
                });
                decrypt_aes128(&key, &iv, data)
            }
        }
    }
}

fn short_url(url: &str) -> String {
    let tail = url.rsplit('/').next().unwrap_or(url);
    tail.chars().take(60).collect()
}

pub fn decrypt_aes128(key: &[u8; 16], iv: &[u8; 16], mut data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.is_empty() || data.len() % 16 != 0 {
        return Err("A video segment is damaged (its size doesn't fit the encryption)".to_string());
    }
    let len = cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut data)
        .map_err(|_| "A video segment couldn't be decrypted (wrong key?)".to_string())?
        .len();
    data.truncate(len);
    Ok(data)
}

// ───────────────────────────── resolving ─────────────────────────────

pub fn is_stream_url(url: &str) -> bool {
    Url::parse(url.trim())
        .map(|u| {
            let p = u.path().to_ascii_lowercase();
            p.ends_with(".m3u8") || p.ends_with(".m3u") || p.ends_with(".mpd")
        })
        .unwrap_or(false)
}

pub fn is_stream_mime(mime: &str) -> bool {
    let m = mime.to_ascii_lowercase();
    m.contains("mpegurl") || m.contains("dash+xml")
}

fn title_from_url(url: &str) -> String {
    let Ok(u) = Url::parse(url) else { return "video".to_string() };
    let generic = ["master", "playlist", "index", "manifest", "stream", "main", "video", "prog_index", "chunklist", "hls", "dash"];
    let mut parts: Vec<String> = u
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).map(|p| p.to_string()).collect())
        .unwrap_or_default();
    while let Some(last) = parts.pop() {
        let stem = last.rsplit_once('.').map(|(s, _)| s).unwrap_or(&last).to_string();
        if !generic.contains(&stem.to_ascii_lowercase().as_str()) && !stem.is_empty() {
            return percent_decode(&stem);
        }
    }
    u.host_str().unwrap_or("video").to_string()
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(s.as_bytes()).map(|(k, v)| if v.is_empty() { k.to_string() } else { format!("{k}={v}") }).collect::<Vec<_>>().join("")
}

fn container_of(init: &Option<Segment>, segments: &[Segment]) -> Container {
    if init.is_some() {
        return Container::Fmp4;
    }
    let ext = segments
        .first()
        .and_then(|s| Url::parse(&s.url).ok())
        .map(|u| u.path().rsplit('.').next().unwrap_or("").to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "ts" | "m2ts" | "mts" => Container::Ts,
        "aac" | "adts" => Container::Aac,
        "m4s" | "mp4" | "m4v" | "m4a" | "cmfv" | "cmfa" => Container::Fmp4,
        _ => Container::Ts,
    }
}

fn label_for(width: u32, height: u32, bandwidth: u64) -> String {
    let rate = if bandwidth > 0 { format!("{:.1} Mbit/s", bandwidth as f64 / 1_000_000.0) } else { String::new() };
    match (height, rate.is_empty()) {
        (0, true) => "Original quality".to_string(),
        (0, false) => rate,
        (h, true) => format!("{h}p"),
        (h, false) => {
            let _ = width;
            format!("{h}p · {rate}")
        }
    }
}

fn variant_id(bandwidth: u64, width: u32, height: u32) -> String {
    format!("{bandwidth}:{width}x{height}")
}

enum Manifest {
    Hls(hls::Playlist),
    Dash(dash::Dash),
}

async fn load(f: &Fetch, url: &str) -> Result<Manifest, String> {
    let text = f.text(url).await?;
    if hls::looks_like_hls(&text) {
        let base = Url::parse(url).map_err(|e| format!("Invalid address: {e}"))?;
        return Ok(Manifest::Hls(hls::parse(&text, &base)?));
    }
    if dash::looks_like_dash(&text) {
        return Ok(Manifest::Dash(dash::parse(&text, url)?));
    }
    Err("That address isn't a video stream (it isn't an HLS playlist or a DASH manifest)".to_string())
}

fn sorted_best_first(mut v: Vec<StreamVariantInfo>) -> Vec<StreamVariantInfo> {
    v.sort_by(|a, b| b.height.cmp(&a.height).then(b.bandwidth.cmp(&a.bandwidth)));
    v.dedup_by(|a, b| a.id == b.id);
    v
}

/// Looks inside a stream without downloading it: its qualities and any reason it can't be saved.
pub async fn probe(f: &Fetch, url: &str) -> Result<StreamProbe, String> {
    let manifest = load(f, url).await?;
    let title = title_from_url(url);
    match manifest {
        Manifest::Hls(hls::Playlist::Master(m)) => {
            let has_audio_track = |group: &Option<String>| {
                group.as_ref().map(|g| m.media.iter().any(|t| t.kind == "AUDIO" && &t.group == g && t.uri.is_some())).unwrap_or(false)
            };
            let variants = sorted_best_first(
                m.variants
                    .iter()
                    .map(|v| StreamVariantInfo {
                        id: variant_id(v.bandwidth, v.width, v.height),
                        label: label_for(v.width, v.height, v.bandwidth),
                        bandwidth: v.bandwidth,
                        width: v.width,
                        height: v.height,
                        separate_audio: has_audio_track(&v.audio_group),
                        codecs: v.codecs.clone(),
                    })
                    .collect(),
            );
            // Duration and "is it live?" come from one media playlist.
            let best = m.variants.iter().max_by_key(|v| (v.height, v.bandwidth)).unwrap();
            let (duration, unsupported) = match hls::parse(&f.text(&best.url).await?, &Url::parse(&best.url).map_err(|e| e.to_string())?)? {
                hls::Playlist::Media(p) => (p.duration, media_problem(&p)),
                hls::Playlist::Master(_) => (0.0, None),
            };
            let ext = resolve_hls_container(f, &m, &best.url).await;
            Ok(StreamProbe { url: url.to_string(), kind: "hls".into(), filename: format!("{}.{ext}", sanitize_filename(&title)), title, duration_secs: duration, variants, unsupported })
        }
        Manifest::Hls(hls::Playlist::Media(p)) => {
            let ext = match container_of(&p.init, &p.segments) {
                Container::Ts => "ts",
                Container::Aac => "aac",
                _ => "mp4",
            };
            Ok(StreamProbe {
                url: url.to_string(),
                kind: "hls".into(),
                filename: format!("{}.{ext}", sanitize_filename(&title)),
                title,
                duration_secs: p.duration,
                variants: vec![StreamVariantInfo { id: "single".into(), label: "Original quality".into(), bandwidth: 0, width: 0, height: 0, separate_audio: false, codecs: None }],
                unsupported: media_problem(&p),
            })
        }
        Manifest::Dash(d) => {
            let videos: Vec<&dash::DashRep> = d.reps.iter().filter(|r| r.content == "video").collect();
            let pool: Vec<&dash::DashRep> = if videos.is_empty() { d.reps.iter().filter(|r| r.content == "audio").collect() } else { videos };
            let has_audio = d.reps.iter().any(|r| r.content == "audio") && !pool.iter().all(|r| r.content == "audio");
            let variants = sorted_best_first(
                pool.iter()
                    .map(|r| StreamVariantInfo {
                        id: r.id.clone(),
                        label: label_for(r.width, r.height, r.bandwidth),
                        bandwidth: r.bandwidth,
                        width: r.width,
                        height: r.height,
                        separate_audio: has_audio,
                        codecs: r.codecs.clone(),
                    })
                    .collect(),
            );
            if variants.is_empty() {
                return Err("The manifest has no video or audio to download".to_string());
            }
            let unsupported = if d.drm {
                Some("This stream is protected by DRM and can't be downloaded".to_string())
            } else if d.live {
                Some("Live streams can't be downloaded yet".to_string())
            } else {
                None
            };
            let ext = if has_audio { "mp4" } else if pool[0].content == "audio" { "m4a" } else { "mp4" };
            Ok(StreamProbe { url: url.to_string(), kind: "dash".into(), filename: format!("{}.{ext}", sanitize_filename(&title)), title, duration_secs: d.duration, variants, unsupported })
        }
    }
}

async fn resolve_hls_container(f: &Fetch, m: &hls::Master, best_url: &str) -> &'static str {
    let _ = m;
    match f.text(best_url).await.ok().and_then(|t| Url::parse(best_url).ok().and_then(|b| hls::parse(&t, &b).ok())) {
        Some(hls::Playlist::Media(p)) => match container_of(&p.init, &p.segments) {
            Container::Ts => "ts",
            Container::Aac => "aac",
            _ => "mp4",
        },
        _ => "mp4",
    }
}

fn media_problem(p: &hls::MediaPlaylist) -> Option<String> {
    if let Some(method) = &p.unsupported_encryption {
        return Some(format!("This stream uses {method} encryption, which can't be downloaded"));
    }
    if !p.complete {
        return Some("Live streams can't be downloaded yet".to_string());
    }
    None
}

async fn hls_track(f: &Fetch, role: &'static str, url: &str, bandwidth: u64) -> Result<Track, String> {
    let base = Url::parse(url).map_err(|e| format!("Invalid address: {e}"))?;
    let hls::Playlist::Media(p) = hls::parse(&f.text(url).await?, &base)? else {
        return Err("Expected a media playlist but found another playlist list".to_string());
    };
    if let Some(problem) = media_problem(&p) {
        return Err(problem);
    }
    Ok(Track { role, container: container_of(&p.init, &p.segments), init: p.init, segments: p.segments, duration: p.duration, bandwidth })
}

/// Reads the manifest and picks the requested quality (`variant`, as listed by [`probe`]) or the best one.
pub async fn resolve(f: &Fetch, url: &str, variant: Option<&str>) -> Result<Resolved, String> {
    let title = title_from_url(url);
    match load(f, url).await? {
        Manifest::Hls(hls::Playlist::Master(m)) => {
            let chosen = variant
                .and_then(|id| m.variants.iter().find(|v| variant_id(v.bandwidth, v.width, v.height) == id))
                .or_else(|| m.variants.iter().max_by_key(|v| (v.height, v.bandwidth)))
                .ok_or("The playlist lists no video qualities")?;
            let audio_tag = chosen.audio_group.as_ref().and_then(|g| {
                let in_group: Vec<_> = m.media.iter().filter(|t| t.kind == "AUDIO" && &t.group == g && t.uri.is_some()).collect();
                in_group.iter().find(|t| t.default).or_else(|| in_group.first()).copied()
            });
            let video = hls_track(f, if audio_tag.is_some() { "video" } else { "muxed" }, &chosen.url, chosen.bandwidth).await?;
            let mut tracks = vec![video];
            if let Some(tag) = audio_tag {
                tracks.push(hls_track(f, "audio", tag.uri.as_ref().unwrap(), 128_000).await?);
            }
            let duration = tracks[0].duration;
            Ok(Resolved { title, kind: "hls", duration, tracks, variant: Some(variant_id(chosen.bandwidth, chosen.width, chosen.height)) })
        }
        Manifest::Hls(hls::Playlist::Media(p)) => {
            if let Some(problem) = media_problem(&p) {
                return Err(problem);
            }
            let track = Track { role: "muxed", container: container_of(&p.init, &p.segments), init: p.init, segments: p.segments, duration: p.duration, bandwidth: 0 };
            Ok(Resolved { title, kind: "hls", duration: track.duration, tracks: vec![track], variant: None })
        }
        Manifest::Dash(d) => {
            if d.drm {
                return Err("This stream is protected by DRM and can't be downloaded".to_string());
            }
            if d.live {
                return Err("Live streams can't be downloaded yet".to_string());
            }
            let videos: Vec<&dash::DashRep> = d.reps.iter().filter(|r| r.content == "video").collect();
            let audios: Vec<&dash::DashRep> = d.reps.iter().filter(|r| r.content == "audio").collect();
            let to_track = |r: &dash::DashRep, role: &'static str| Track {
                role,
                container: if r.init.is_some() { Container::Fmp4 } else { Container::Other },
                init: r.init.clone(),
                segments: r.segments.clone(),
                duration: d.duration,
                bandwidth: r.bandwidth,
            };
            let main = if videos.is_empty() {
                variant.and_then(|id| audios.iter().find(|r| r.id == id)).or_else(|| audios.iter().max_by_key(|r| r.bandwidth)).copied()
            } else {
                variant.and_then(|id| videos.iter().find(|r| r.id == id)).or_else(|| videos.iter().max_by_key(|r| (r.height, r.bandwidth))).copied()
            }
            .ok_or("The manifest has no video or audio to download")?;
            let mut tracks = vec![to_track(main, if videos.is_empty() { "audio" } else if audios.is_empty() { "muxed" } else { "video" })];
            if !videos.is_empty() {
                if let Some(a) = audios.iter().max_by_key(|r| r.bandwidth) {
                    tracks.push(to_track(a, "audio"));
                }
            }
            Ok(Resolved { title, kind: "dash", duration: d.duration, tracks, variant: Some(main.id.clone()) })
        }
    }
}

// ───────────────────────────── downloading ─────────────────────────────

#[derive(Serialize, Deserialize)]
struct Sidecar {
    total: usize,
    next: usize,
    bytes: u64,
}

enum TrackEnd {
    Done,
    Paused,
}

struct Limits {
    task: RateLimiter,
    group: Option<Arc<RateLimiter>>,
    global: Arc<RateLimiter>,
}

impl Limits {
    async fn acquire(&self, bytes: u64) {
        self.task.acquire(bytes).await;
        if let Some(g) = &self.group {
            g.acquire(bytes).await;
        }
        self.global.acquire(bytes).await;
    }
}

fn track_paths(dir: &Path, filename: &str, i: usize) -> (PathBuf, PathBuf) {
    (dir.join(format!("{filename}.nexpart.t{i}")), dir.join(format!("{filename}.nexpart.t{i}.json")))
}

/// Removes every partial file of a stream download.
pub fn remove_partials(dir: &Path, filename: &str) {
    for i in 0..4 {
        let (part, side) = track_paths(dir, filename, i);
        let _ = std::fs::remove_file(part);
        let _ = std::fs::remove_file(side);
    }
    let _ = std::fs::remove_file(dir.join(format!("{filename}.nexpart")));
}

struct Counters {
    /// Pieces (segments) committed per track.
    pieces_done: AtomicU64,
    bytes: AtomicU64,
}

impl DownloadEngine {
    /// Downloads an HLS or DASH stream. Same contract as `run_download_with`:
    /// `Completed`, `Paused` (resumable from the last whole segment), or `Err`.
    pub async fn run_stream<F>(
        &self,
        mut task: DownloadTask,
        mut pause_rx: watch::Receiver<bool>,
        extra_headers: Vec<(String, String)>,
        on_progress: F,
    ) -> Result<DownloadTask, String>
    where
        F: Fn(DownloadProgressEvent) + Send + Sync + 'static,
    {
        let dest_dir = PathBuf::from(&task.destination_path);
        tokio::fs::create_dir_all(&dest_dir).await.map_err(|e| format!("Cannot create destination folder: {e}"))?;

        let headers = build_header_list(&task.custom_headers, &extra_headers);
        let client = self.pool.download(task.proxy_mode.as_deref(), task.custom_proxy_url.as_deref());
        let fetch = Arc::new(Fetch::new(client, headers, &task.url, task.referer.clone(), task.user_agent.clone(), self.retry_base));

        // Fresh playlist every start: signed links in them stay valid this way.
        let variant = task.etag.clone().filter(|v| !v.is_empty());
        let resolved = tokio::select! {
            biased;
            _ = wait_paused(&mut pause_rx) => return Ok(paused(task)),
            r = resolve(&fetch, &task.url, variant.as_deref()) => r?,
        };

        let extension = resolved.extension();
        let stem = Path::new(&task.filename).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "video".to_string());
        let wanted_name = format!("{stem}.{extension}");
        if wanted_name != task.filename {
            // The container turned out different from the estimate made when it was added.
            remove_partials(&dest_dir, &task.filename);
            task.filename = wanted_name;
        }
        let filename = task.filename.clone();

        let estimate = resolved.estimated_bytes().max(1);
        check_disk_space(&dest_dir, estimate.saturating_mul(if resolved.tracks.len() > 1 { 2 } else { 1 }))?;

        let limits = Arc::new(Limits {
            task: RateLimiter::new(task.speed_limit),
            group: self.group_limit.clone(),
            global: self.global_limit.clone(),
        });
        let connections = task.connections.clamp(1, MAX_CONNECTIONS);
        let total_pieces: usize = resolved.tracks.iter().map(Track::total_pieces).sum::<usize>().max(1);
        let counters = Arc::new(Counters { pieces_done: AtomicU64::new(0), bytes: AtomicU64::new(0) });

        let started = Instant::now();
        let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
        let mut speed_window: std::collections::VecDeque<(Instant, u64)> = Default::default();
        let task_id = task.id.clone();
        let total_hint = task.total_bytes;
        let mut emit = |counters: &Counters, force: bool, status: DownloadStatus| {
            if !force && last_emit.elapsed() < PROGRESS_INTERVAL {
                return (0u64, 0u64);
            }
            last_emit = Instant::now();
            let bytes = counters.bytes.load(Ordering::Relaxed);
            let done = counters.pieces_done.load(Ordering::Relaxed);
            let now = Instant::now();
            speed_window.push_back((now, bytes));
            while speed_window.front().map(|(t, _)| now.duration_since(*t) > Duration::from_secs(3)).unwrap_or(false) {
                speed_window.pop_front();
            }
            let speed = match (speed_window.front(), speed_window.back()) {
                (Some((t0, b0)), Some((t1, b1))) if t1 > t0 => ((b1 - b0) as f64 / t1.duration_since(*t0).as_secs_f64()) as u64,
                _ => 0,
            };
            // Total size is only known at the end: extrapolate from the fraction done.
            let fraction = done as f64 / total_pieces as f64;
            let total = if fraction > 0.02 { (bytes as f64 / fraction) as u64 } else { total_hint.max(0) as u64 }.max(bytes);
            let eta = if speed > 0 { Some(total.saturating_sub(bytes) / speed) } else { None };
            on_progress(DownloadProgressEvent {
                id: task_id.clone(),
                downloaded_bytes: bytes as i64,
                total_bytes: total as i64,
                speed_bps: speed,
                eta_seconds: eta,
                status,
                segments: vec![],
            });
            (bytes, total)
        };

        let mut part_paths = Vec::new();
        for (i, track) in resolved.tracks.iter().enumerate() {
            let (part, side) = track_paths(&dest_dir, &filename, i);
            part_paths.push(part.clone());
            let end = self
                .download_track(&fetch, track, &part, &side, connections, &limits, &mut pause_rx, &counters, &mut |c| {
                    emit(c, false, DownloadStatus::Downloading);
                })
                .await?;
            if let TrackEnd::Paused = end {
                let (bytes, total) = emit(&counters, true, DownloadStatus::Paused);
                task.downloaded_bytes = bytes as i64;
                task.total_bytes = total as i64;
                return Ok(paused(task));
            }
        }
        let _ = started;

        // ── put the pieces together
        let final_path = reserve_unique_path(&dest_dir, &file_name_of(&dest_dir.join(&filename))).map_err(|e| format!("Cannot create the final file: {e}"))?;
        if resolved.can_join() {
            let (v, a, out) = (part_paths[0].clone(), part_paths[1].clone(), final_path.clone());
            let joined = tokio::task::spawn_blocking(move || fmp4::mux(&v, &a, &out)).await.map_err(|e| format!("Internal error: {e}"))?;
            if let Err(e) = joined {
                let _ = tokio::fs::remove_file(&final_path).await;
                return Err(format!("Couldn't join the video and audio: {e}"));
            }
        } else {
            tokio::fs::rename(&part_paths[0], &final_path).await.map_err(|e| {
                format!("Failed to move the finished video into place: {e}")
            })?;
            if let Some(audio_part) = part_paths.get(1) {
                // Audio that can't be joined stays next to the video as its own file.
                let ext = match resolved.tracks[1].container {
                    Container::Aac => "aac",
                    Container::Ts => "ts",
                    _ => "m4a",
                };
                let stem = final_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if let Ok(audio_final) = reserve_unique_path(&dest_dir, &format!("{stem}.audio.{ext}")) {
                    let _ = tokio::fs::rename(audio_part, audio_final).await;
                }
            }
        }
        remove_partials(&dest_dir, &filename);

        let size = tokio::fs::metadata(&final_path).await.map(|m| m.len()).unwrap_or(0);
        finish_task(&mut task, &final_path);
        task.total_bytes = size as i64;
        task.downloaded_bytes = size as i64;
        on_progress(DownloadProgressEvent {
            id: task.id.clone(),
            downloaded_bytes: size as i64,
            total_bytes: size as i64,
            speed_bps: 0,
            eta_seconds: None,
            status: DownloadStatus::Completed,
            segments: vec![],
        });
        Ok(task)
    }

    #[allow(clippy::too_many_arguments)]
    async fn download_track(
        &self,
        fetch: &Arc<Fetch>,
        track: &Track,
        part: &Path,
        side: &Path,
        connections: usize,
        limits: &Arc<Limits>,
        pause_rx: &mut watch::Receiver<bool>,
        counters: &Arc<Counters>,
        tick: &mut (dyn FnMut(&Counters) + Send),
    ) -> Result<TrackEnd, String> {
        // The init segment (if any) is piece 0, then the media segments.
        let mut pieces: Vec<Segment> = Vec::with_capacity(track.total_pieces());
        if let Some(init) = &track.init {
            pieces.push(init.clone());
        }
        pieces.extend(track.segments.iter().cloned());
        let total = pieces.len();

        // Resume from the last whole segment, if the earlier run left a matching record.
        let saved: Option<Sidecar> = std::fs::read_to_string(side).ok().and_then(|t| serde_json::from_str(&t).ok());
        let (start_at, start_bytes) = match saved {
            Some(s) if s.total == total && s.next <= total && std::fs::metadata(part).map(|m| m.len() >= s.bytes).unwrap_or(false) => (s.next, s.bytes),
            _ => (0, 0),
        };

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(start_at == 0)
            .open(part)
            .await
            .map_err(|e| format!("Failed to create the download file: {e}"))?;
        file.set_len(start_bytes).await.map_err(|e| format!("Disk error: {e}"))?;
        let mut file = tokio::io::BufWriter::with_capacity(1 << 20, file);
        {
            use tokio::io::AsyncSeekExt;
            file.seek(std::io::SeekFrom::Start(start_bytes)).await.map_err(|e| format!("Disk error: {e}"))?;
        }

        counters.pieces_done.fetch_add(start_at as u64, Ordering::Relaxed);
        counters.bytes.fetch_add(start_bytes, Ordering::Relaxed);

        let mut next = start_at;
        let mut bytes_in_file = start_bytes;
        let mut last_save = Instant::now();
        let save = |next: usize, bytes: u64| {
            let _ = std::fs::write(side, serde_json::to_string(&Sidecar { total, next, bytes }).unwrap_or_default());
        };

        let todo: Vec<(usize, Segment)> = pieces.into_iter().enumerate().skip(start_at).collect();
        let fetcher = fetch.clone();
        let mut stream = futures_util::stream::iter(todo)
            .map(move |(i, seg)| {
                let f = fetcher.clone();
                async move { f.segment(&seg).await.map(|b| (i, b)) }
            })
            .buffered(connections);

        loop {
            let item = tokio::select! {
                biased;
                _ = wait_paused(pause_rx) => {
                    file.flush().await.map_err(|e| format!("Disk error: {e}"))?;
                    save(next, bytes_in_file);
                    return Ok(TrackEnd::Paused);
                }
                item = stream.next() => item,
            };
            let Some(item) = item else { break };
            let (index, data) = item?;
            debug_assert_eq!(index, next);

            tokio::select! {
                biased;
                _ = wait_paused(pause_rx) => {
                    file.flush().await.map_err(|e| format!("Disk error: {e}"))?;
                    save(next, bytes_in_file);
                    return Ok(TrackEnd::Paused);
                }
                _ = limits.acquire(data.len() as u64) => {}
            }

            file.write_all(&data).await.map_err(|e| format!("Disk write error: {e}"))?;
            bytes_in_file += data.len() as u64;
            next += 1;
            counters.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            counters.pieces_done.fetch_add(1, Ordering::Relaxed);
            tick(counters);

            if last_save.elapsed() >= SIDECAR_INTERVAL {
                file.flush().await.map_err(|e| format!("Disk error: {e}"))?;
                save(next, bytes_in_file);
                last_save = Instant::now();
            }
        }

        file.flush().await.map_err(|e| format!("Failed to flush the file: {e}"))?;
        file.get_ref().sync_data().await.ok();
        save(total, bytes_in_file);
        Ok(TrackEnd::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_stream_addresses_and_mime_types() {
        assert!(is_stream_url("https://cdn.example.com/v/master.m3u8?token=abc"));
        assert!(is_stream_url("https://cdn.example.com/v/manifest.MPD"));
        assert!(!is_stream_url("https://example.com/video.mp4"));
        assert!(!is_stream_url("not a url"));
        assert!(is_stream_mime("application/vnd.apple.mpegurl"));
        assert!(is_stream_mime("application/x-mpegURL; charset=utf-8"));
        assert!(is_stream_mime("application/dash+xml"));
        assert!(!is_stream_mime("video/mp4"));
    }

    #[test]
    fn titles_skip_generic_names() {
        assert_eq!(title_from_url("https://cdn.example.com/shows/My%20Show/master.m3u8"), "My Show");
        assert_eq!(title_from_url("https://cdn.example.com/a/holiday.m3u8"), "holiday");
        assert_eq!(title_from_url("https://cdn.example.com/index.m3u8"), "cdn.example.com");
    }

    #[test]
    fn qualities_get_readable_labels() {
        assert_eq!(label_for(1920, 1080, 5_200_000), "1080p · 5.2 Mbit/s");
        assert_eq!(label_for(0, 0, 0), "Original quality");
        assert_eq!(label_for(0, 0, 800_000), "0.8 Mbit/s");
        assert_eq!(variant_id(800000, 640, 360), "800000:640x360");
    }

    #[test]
    fn aes_128_cbc_round_trips_and_rejects_bad_input() {
        use aes::cipher::BlockEncryptMut;
        let key = [7u8; 16];
        let iv = [9u8; 16];
        let plain = b"hello transport stream, longer than one block!".to_vec();
        let mut buf = plain.clone();
        buf.resize(plain.len() + 16, 0);
        let enc = cbc::Encryptor::<aes::Aes128>::new((&key).into(), (&iv).into()).encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len()).unwrap().to_vec();
        assert_eq!(decrypt_aes128(&key, &iv, enc.clone()).unwrap(), plain);
        assert!(decrypt_aes128(&key, &iv, vec![1, 2, 3]).is_err(), "not a whole number of blocks");
        assert!(decrypt_aes128(&[8u8; 16], &iv, enc).is_err(), "a wrong key breaks the padding");
    }

    #[test]
    fn joined_streams_become_mp4_and_single_ts_stays_ts() {
        let seg = |u: &str| Segment { url: u.into(), range: None, duration: 4.0, key: None, seq: 0 };
        let track = |role, container, init: bool| Track {
            role,
            init: init.then(|| seg("https://h/init.mp4")),
            segments: vec![seg("https://h/a.m4s")],
            duration: 4.0,
            bandwidth: 1_000_000,
            container,
        };
        let joined = Resolved { title: "x".into(), kind: "hls", duration: 4.0, tracks: vec![track("video", Container::Fmp4, true), track("audio", Container::Fmp4, true)], variant: None };
        assert!(joined.can_join());
        assert_eq!(joined.extension(), "mp4");
        let ts = Resolved { title: "x".into(), kind: "hls", duration: 4.0, tracks: vec![track("muxed", Container::Ts, false)], variant: None };
        assert!(!ts.can_join());
        assert_eq!(ts.extension(), "ts");
        let mixed = Resolved { title: "x".into(), kind: "hls", duration: 4.0, tracks: vec![track("video", Container::Ts, false), track("audio", Container::Aac, false)], variant: None };
        assert!(!mixed.can_join(), "a TS video and an AAC audio track can't be joined without re-muxing");
        assert_eq!(mixed.estimated_bytes(), 1_000_000);
    }
}
