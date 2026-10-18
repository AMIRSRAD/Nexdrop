//! HLS (`.m3u8`) playlist parsing.
//!
//! Two kinds of playlist exist. A **master** playlist lists the qualities
//! (variants) of a video and any separate audio tracks; a **media** playlist
//! lists the segments of one rendition. Everything returned here has absolute
//! URLs, so the downloader never has to think about relative paths.

use std::collections::HashMap;
use std::sync::Arc;

use url::Url;

use super::{KeyInfo, Segment};

/// One quality listed in a master playlist.
#[derive(Debug, Clone)]
pub struct HlsVariant {
    pub url: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: Option<String>,
    pub audio_group: Option<String>,
}

/// An `#EXT-X-MEDIA` entry (audio, subtitles...).
#[derive(Debug, Clone)]
pub struct MediaTag {
    pub kind: String,
    pub group: String,
    pub name: String,
    pub language: Option<String>,
    pub uri: Option<String>,
    pub default: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Master {
    pub variants: Vec<HlsVariant>,
    pub media: Vec<MediaTag>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaPlaylist {
    pub segments: Vec<Segment>,
    pub init: Option<Segment>,
    /// `#EXT-X-ENDLIST` is present (or it's a VOD playlist): nothing more will be added.
    pub complete: bool,
    pub duration: f64,
    /// Set when the playlist uses an encryption method we can't undo.
    pub unsupported_encryption: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Playlist {
    Master(Master),
    Media(MediaPlaylist),
}

pub fn looks_like_hls(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().starts_with("#EXTM3U")
}

/// Splits `KEY=value,KEY2="quoted, value"` into a map (keys upper-cased).
pub fn attributes(list: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut chars = list.chars().peekable();
    loop {
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' || c == ',' {
                break;
            }
            key.push(c);
            chars.next();
        }
        if chars.peek() != Some(&'=') {
            if chars.next().is_none() {
                break;
            }
            continue;
        }
        chars.next(); // '='
        let mut value = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                value.push(c);
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c == ',' {
                    break;
                }
                value.push(c);
                chars.next();
            }
        }
        out.insert(key.trim().to_ascii_uppercase(), value);
        if chars.peek() == Some(&',') {
            chars.next();
        }
    }
    out
}

fn resolve(base: &Url, reference: &str) -> Result<String, String> {
    base.join(reference.trim())
        .map(|u| u.to_string())
        .map_err(|e| format!("Bad address in the playlist ({reference}): {e}"))
}

/// `"1234@5678"` → (start 5678, length 1234); without an offset the range follows `previous_end`.
fn byte_range(value: &str, previous_end: u64) -> Option<(u64, u64)> {
    let (len, off) = match value.split_once('@') {
        Some((l, o)) => (l.trim().parse::<u64>().ok()?, o.trim().parse::<u64>().ok()?),
        None => (value.trim().parse::<u64>().ok()?, previous_end),
    };
    Some((off, len))
}

fn parse_iv(text: &str) -> Option<[u8; 16]> {
    let hex = text.trim().trim_start_matches("0x").trim_start_matches("0X");
    if hex.is_empty() || hex.len() > 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let padded = format!("{hex:0>32}");
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&padded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn parse(text: &str, base: &Url) -> Result<Playlist, String> {
    if !looks_like_hls(text) {
        return Err("That isn't a valid HLS playlist".to_string());
    }
    let text = text.trim_start_matches('\u{feff}');
    if text.contains("#EXT-X-STREAM-INF") {
        parse_master(text, base).map(Playlist::Master)
    } else {
        parse_media(text, base).map(Playlist::Media)
    }
}

fn parse_master(text: &str, base: &Url) -> Result<Master, String> {
    let mut master = Master::default();
    let mut pending: Option<HashMap<String, String>> = None;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            pending = Some(attributes(rest));
        } else if let Some(rest) = line.strip_prefix("#EXT-X-MEDIA:") {
            let a = attributes(rest);
            master.media.push(MediaTag {
                kind: a.get("TYPE").cloned().unwrap_or_default().to_ascii_uppercase(),
                group: a.get("GROUP-ID").cloned().unwrap_or_default(),
                name: a.get("NAME").cloned().unwrap_or_default(),
                language: a.get("LANGUAGE").cloned(),
                uri: match a.get("URI") {
                    Some(u) if !u.is_empty() => Some(resolve(base, u)?),
                    _ => None,
                },
                default: a.get("DEFAULT").map(|v| v.eq_ignore_ascii_case("YES")).unwrap_or(false),
            });
        } else if !line.is_empty() && !line.starts_with('#') {
            if let Some(a) = pending.take() {
                let (width, height) = a
                    .get("RESOLUTION")
                    .and_then(|r| r.split_once(['x', 'X']))
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .unwrap_or((0, 0));
                let bandwidth = a
                    .get("AVERAGE-BANDWIDTH")
                    .or_else(|| a.get("BANDWIDTH"))
                    .and_then(|b| b.parse().ok())
                    .unwrap_or(0);
                master.variants.push(HlsVariant {
                    url: resolve(base, line)?,
                    bandwidth,
                    width,
                    height,
                    codecs: a.get("CODECS").cloned(),
                    audio_group: a.get("AUDIO").cloned(),
                });
            }
        }
    }
    if master.variants.is_empty() {
        return Err("The playlist lists no video qualities".to_string());
    }
    Ok(master)
}

fn parse_media(text: &str, base: &Url) -> Result<MediaPlaylist, String> {
    let mut playlist = MediaPlaylist::default();
    let mut media_sequence: u64 = 0;
    let mut seq_set = false;
    let mut key: Option<Arc<KeyInfo>> = None;
    let mut duration = 0.0f64;
    let mut range: Option<(u64, u64)> = None;
    let mut prev_end: u64 = 0;
    let mut vod = false;
    let mut count: u64 = 0;

    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(v) = rest.strip_prefix("EXT-X-MEDIA-SEQUENCE:") {
                media_sequence = v.trim().parse().unwrap_or(0);
                seq_set = true;
            } else if let Some(v) = rest.strip_prefix("EXT-X-PLAYLIST-TYPE:") {
                vod = v.trim().eq_ignore_ascii_case("VOD");
            } else if rest.starts_with("EXT-X-ENDLIST") {
                playlist.complete = true;
            } else if let Some(v) = rest.strip_prefix("EXTINF:") {
                duration = v.split(',').next().and_then(|d| d.trim().parse().ok()).unwrap_or(0.0);
            } else if let Some(v) = rest.strip_prefix("EXT-X-BYTERANGE:") {
                range = byte_range(v, prev_end);
            } else if let Some(v) = rest.strip_prefix("EXT-X-KEY:") {
                let a = attributes(v);
                let method = a.get("METHOD").map(|m| m.to_ascii_uppercase()).unwrap_or_default();
                key = match method.as_str() {
                    "NONE" | "" => None,
                    "AES-128" => {
                        let uri = a.get("URI").ok_or("The playlist's encryption key has no address")?;
                        Some(Arc::new(KeyInfo { uri: resolve(base, uri)?, iv: a.get("IV").and_then(|i| parse_iv(i)) }))
                    }
                    other => {
                        playlist.unsupported_encryption.get_or_insert_with(|| other.to_string());
                        None
                    }
                };
            } else if let Some(v) = rest.strip_prefix("EXT-X-MAP:") {
                let a = attributes(v);
                if let Some(uri) = a.get("URI") {
                    playlist.init = Some(Segment {
                        url: resolve(base, uri)?,
                        range: a.get("BYTERANGE").and_then(|r| byte_range(r, 0)),
                        duration: 0.0,
                        key: key.clone(),
                        seq: 0,
                    });
                }
            }
            continue;
        }

        // A segment address.
        let seg_range = range.take();
        if let Some((start, len)) = seg_range {
            prev_end = start + len;
        }
        playlist.segments.push(Segment {
            url: resolve(base, line)?,
            range: seg_range,
            duration,
            key: key.clone(),
            seq: if seq_set { media_sequence } else { 0 } + count,
        });
        playlist.duration += duration;
        count += 1;
        duration = 0.0;
    }

    if vod {
        playlist.complete = true;
    }
    if playlist.segments.is_empty() {
        return Err("The playlist has no video segments".to_string());
    }
    Ok(playlist)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/videos/show/master.m3u8").unwrap()
    }

    #[test]
    fn attribute_lists_handle_quotes_and_commas() {
        let a = attributes(r#"BANDWIDTH=800000,CODECS="avc1.4d401f,mp4a.40.2",RESOLUTION=640x360,NAME="a=b""#);
        assert_eq!(a["BANDWIDTH"], "800000");
        assert_eq!(a["CODECS"], "avc1.4d401f,mp4a.40.2");
        assert_eq!(a["RESOLUTION"], "640x360");
        assert_eq!(a["NAME"], "a=b");
    }

    #[test]
    fn master_lists_variants_with_absolute_urls() {
        let text = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",DEFAULT=YES,URI=\"audio/en.m3u8\"\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1280x720,AUDIO=\"aud\"\nhd/index.m3u8\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=400000,RESOLUTION=640x360\nhttps://other.example.org/sd.m3u8\n";
        let Playlist::Master(m) = parse(text, &base()).unwrap() else { panic!("expected a master playlist") };
        assert_eq!(m.variants.len(), 2);
        assert_eq!(m.variants[0].url, "https://cdn.example.com/videos/show/hd/index.m3u8");
        assert_eq!((m.variants[0].width, m.variants[0].height), (1280, 720));
        assert_eq!(m.variants[0].audio_group.as_deref(), Some("aud"));
        assert_eq!(m.variants[1].url, "https://other.example.org/sd.m3u8");
        assert_eq!(m.media[0].uri.as_deref(), Some("https://cdn.example.com/videos/show/audio/en.m3u8"));
        assert!(m.media[0].default);
    }

    #[test]
    fn media_playlist_with_keys_ranges_and_maps() {
        let text = "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-MEDIA-SEQUENCE:10\n\
                    #EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"720@0\"\n\
                    #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x0000000000000000000000000000ABCD\n\
                    #EXTINF:4.0,\n#EXT-X-BYTERANGE:1000@720\nmedia.mp4\n\
                    #EXTINF:4.5,\n#EXT-X-BYTERANGE:2000\nmedia.mp4\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(text, &base()).unwrap() else { panic!("expected a media playlist") };
        assert!(m.complete);
        assert_eq!(m.segments.len(), 2);
        assert_eq!(m.segments[0].range, Some((720, 1000)));
        assert_eq!(m.segments[1].range, Some((1720, 2000)), "a range without an offset follows the previous one");
        assert_eq!(m.segments[0].seq, 10);
        assert_eq!(m.segments[1].seq, 11);
        assert!((m.duration - 8.5).abs() < 1e-9);
        assert_eq!(m.init.as_ref().unwrap().url, "https://cdn.example.com/videos/show/init.mp4");
        assert_eq!(m.init.as_ref().unwrap().range, Some((0, 720)));
        let key = m.segments[0].key.as_ref().unwrap();
        assert_eq!(key.uri, "https://cdn.example.com/videos/show/key.bin");
        assert_eq!(key.iv.unwrap()[14..], [0xAB, 0xCD]);
    }

    #[test]
    fn live_playlists_are_not_complete_and_vod_is() {
        let live = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\na.ts\n#EXTINF:4,\nb.ts\n";
        let Playlist::Media(m) = parse(live, &base()).unwrap() else { panic!() };
        assert!(!m.complete);
        let vod = "#EXTM3U\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4,\na.ts\n";
        let Playlist::Media(m) = parse(vod, &base()).unwrap() else { panic!() };
        assert!(m.complete);
    }

    #[test]
    fn sample_aes_is_reported_not_silently_ignored() {
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://x\"\n#EXTINF:4,\na.ts\n#EXT-X-ENDLIST\n";
        let Playlist::Media(m) = parse(text, &base()).unwrap() else { panic!() };
        assert_eq!(m.unsupported_encryption.as_deref(), Some("SAMPLE-AES"));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse("<html>nope</html>", &base()).is_err());
        assert!(parse("#EXTM3U\n#EXT-X-ENDLIST\n", &base()).is_err(), "a playlist without segments is useless");
    }

    #[test]
    fn short_ivs_are_left_padded() {
        assert_eq!(parse_iv("0x1").unwrap()[15], 1);
        assert!(parse_iv("0xZZ").is_none());
        assert!(parse_iv("").is_none());
    }
}
