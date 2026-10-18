//! Joining a video track and an audio track into one fragmented MP4.
//!
//! Streaming sites deliver video and audio as two separate fragmented-MP4
//! tracks. Putting them in one file doesn't need re-encoding, only box
//! shuffling:
//!
//! * the video's init segment gets the audio `trak` (and its `trex`) added,
//!   with the audio given a track id of its own;
//! * the two tracks' fragments (`moof` + `mdat`) are interleaved by decode
//!   time, and the audio fragments are re-labelled with the new track id.
//!
//! Fragment payloads are copied through untouched and streamed, so even
//! multi-gigabyte files only cost a few kilobytes of memory.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

#[derive(Debug, Clone, Copy)]
struct Header {
    kind: [u8; 4],
    /// Offset of the box start in the stream.
    start: u64,
    /// Total size including the header.
    size: u64,
    header_len: u64,
}

fn read_header<R: Read + Seek>(r: &mut R, file_len: u64) -> io::Result<Option<Header>> {
    let start = r.stream_position()?;
    if start + 8 > file_len {
        return Ok(None);
    }
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let size32 = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
    let kind = [h[4], h[5], h[6], h[7]];
    let (size, header_len) = match size32 {
        0 => (file_len - start, 8),
        1 => {
            let mut big = [0u8; 8];
            r.read_exact(&mut big)?;
            (u64::from_be_bytes(big), 16)
        }
        n => (n, 8),
    };
    if size < header_len || start + size > file_len {
        return Err(err("A box in the video file is cut short"));
    }
    Ok(Some(Header { kind, start, size, header_len }))
}

fn make_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() as u32 + 8).to_be_bytes()));
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

/// Child boxes of an in-memory box payload: (kind, whole box bytes).
fn children(payload: &[u8]) -> io::Result<Vec<([u8; 4], Vec<u8>)>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= payload.len() {
        let size32 = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = payload[pos + 4..pos + 8].try_into().unwrap();
        let size = if size32 == 0 { payload.len() - pos } else if size32 == 1 {
            if pos + 16 > payload.len() {
                return Err(err("A box in the video file is cut short"));
            }
            u64::from_be_bytes(payload[pos + 8..pos + 16].try_into().unwrap()) as usize
        } else {
            size32
        };
        if size < 8 || pos + size > payload.len() {
            return Err(err("A box in the video file is cut short"));
        }
        out.push((kind, payload[pos..pos + size].to_vec()));
        pos += size;
    }
    Ok(out)
}

fn payload_of(whole: &[u8]) -> &[u8] {
    let size32 = u32::from_be_bytes(whole[0..4].try_into().unwrap());
    if size32 == 1 {
        &whole[16..]
    } else {
        &whole[8..]
    }
}

fn find<'a>(boxes: &'a [([u8; 4], Vec<u8>)], kind: &[u8; 4]) -> Option<&'a Vec<u8>> {
    boxes.iter().find(|(k, _)| k == kind).map(|(_, b)| b)
}

// ───────────────────────────── init segments ─────────────────────────────

struct Init {
    ftyp: Option<Vec<u8>>,
    /// Children of `moov`.
    moov: Vec<([u8; 4], Vec<u8>)>,
}

fn read_init(path: &Path, init_len: u64) -> io::Result<Init> {
    let file = File::open(path)?;
    let mut r = BufReader::new(file);
    let mut ftyp = None;
    let mut moov = None;
    while r.stream_position()? < init_len {
        let Some(h) = read_header(&mut r, init_len)? else { break };
        let mut body = vec![0u8; (h.size - h.header_len) as usize];
        r.read_exact(&mut body)?;
        match &h.kind {
            b"ftyp" => ftyp = Some(make_box(b"ftyp", &body)),
            b"moov" => moov = Some(children(&body)?),
            _ => {}
        }
    }
    Ok(Init { ftyp, moov: moov.ok_or_else(|| err("The video track has no header (moov)"))? })
}

fn traks(init: &Init) -> Vec<&Vec<u8>> {
    init.moov.iter().filter(|(k, _)| k == b"trak").map(|(_, b)| b).collect()
}

fn tkhd_id_offset(tkhd_payload: &[u8]) -> usize {
    if tkhd_payload.first() == Some(&1) { 20 } else { 12 }
}

fn track_id_of(trak: &[u8]) -> io::Result<u32> {
    let kids = children(payload_of(trak))?;
    let tkhd = find(&kids, b"tkhd").ok_or_else(|| err("A track has no header (tkhd)"))?;
    let p = payload_of(tkhd);
    let o = tkhd_id_offset(p);
    Ok(u32::from_be_bytes(p.get(o..o + 4).ok_or_else(|| err("A track header is too short"))?.try_into().unwrap()))
}

fn timescale_of(trak: &[u8]) -> io::Result<u64> {
    let mdia = children(payload_of(trak))?;
    let mdia = find(&mdia, b"mdia").ok_or_else(|| err("A track has no media header (mdia)"))?;
    let kids = children(payload_of(mdia))?;
    let mdhd = find(&kids, b"mdhd").ok_or_else(|| err("A track has no media header (mdhd)"))?;
    let p = payload_of(mdhd);
    let o = if p.first() == Some(&1) { 20 } else { 12 };
    Ok(u32::from_be_bytes(p.get(o..o + 4).ok_or_else(|| err("A media header is too short"))?.try_into().unwrap()) as u64)
}

fn with_track_id(trak: &[u8], new_id: u32) -> io::Result<Vec<u8>> {
    let kids = children(payload_of(trak))?;
    let mut out = Vec::new();
    for (kind, whole) in kids {
        if &kind == b"tkhd" {
            let mut p = payload_of(&whole).to_vec();
            let o = tkhd_id_offset(&p);
            p[o..o + 4].copy_from_slice(&new_id.to_be_bytes());
            out.extend(make_box(b"tkhd", &p));
        } else {
            out.extend(whole);
        }
    }
    Ok(make_box(b"trak", &out))
}

/// The video's `moov` with the audio track added.
fn merged_moov(video: &Init, audio: &Init) -> io::Result<(Vec<u8>, u32)> {
    let video_traks = traks(video);
    let audio_traks = traks(audio);
    if video_traks.is_empty() || audio_traks.is_empty() {
        return Err(err("A track has no media in it"));
    }
    let mut max_id = 0;
    for t in &video_traks {
        max_id = max_id.max(track_id_of(t)?);
    }
    let new_id = max_id + 1;
    let audio_trak = with_track_id(audio_traks[0], new_id)?;
    let audio_trex_src = audio.moov.iter().find(|(k, _)| k == b"mvex").map(|(_, b)| children(payload_of(b))).transpose()?;
    let audio_trex = audio_trex_src.as_ref().and_then(|c| find(c, b"trex")).cloned();

    let mut out = Vec::new();
    for (kind, whole) in &video.moov {
        match kind {
            b"mvhd" => {
                let mut p = payload_of(whole).to_vec();
                let o = if p.first() == Some(&1) { 108 } else { 96 };
                if p.len() < o + 4 {
                    return Err(err("The movie header is too short"));
                }
                p[o..o + 4].copy_from_slice(&(new_id + 1).to_be_bytes());
                out.extend(make_box(b"mvhd", &p));
            }
            b"mvex" => {
                let mut mvex = Vec::new();
                for (k, b) in children(payload_of(whole))? {
                    if &k != b"mehd" {
                        mvex.extend(b);
                    }
                }
                if let Some(trex) = &audio_trex {
                    let mut p = payload_of(trex).to_vec();
                    if p.len() < 8 {
                        return Err(err("A track extension header is too short"));
                    }
                    p[4..8].copy_from_slice(&new_id.to_be_bytes());
                    mvex.extend(make_box(b"trex", &p));
                } else {
                    return Err(err("The audio track has no fragment defaults (trex)"));
                }
                out.extend(make_box(b"mvex", &mvex));
            }
            _ => out.extend(whole),
        }
        if kind == b"trak" && std::ptr::eq(whole, *video_traks.last().unwrap()) {
            out.extend(&audio_trak);
        }
    }
    Ok((make_box(b"moov", &out), new_id))
}

// ───────────────────────────── fragments ─────────────────────────────

struct Frag {
    time: f64,
    moof: Vec<u8>,
    mdat_pos: u64,
    mdat_size: u64,
}

struct Frags {
    r: BufReader<File>,
    len: u64,
    timescale: f64,
    /// Used when a fragment has no decode time.
    fallback_time: f64,
}

impl Frags {
    fn open(path: &Path, start: u64, timescale: u64) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut r = BufReader::new(file);
        r.seek(SeekFrom::Start(start))?;
        Ok(Self { r, len, timescale: timescale.max(1) as f64, fallback_time: 0.0 })
    }

    fn next(&mut self) -> io::Result<Option<Frag>> {
        let mut moof: Option<Vec<u8>> = None;
        loop {
            let Some(h) = read_header(&mut self.r, self.len)? else { return Ok(None) };
            match &h.kind {
                b"moof" => {
                    let mut body = vec![0u8; (h.size - h.header_len) as usize];
                    self.r.read_exact(&mut body)?;
                    moof = Some(make_box(b"moof", &body));
                }
                b"mdat" => {
                    self.r.seek(SeekFrom::Start(h.start + h.size))?;
                    let Some(moof) = moof else { continue };
                    let time = decode_time(&moof)?.map(|t| t as f64 / self.timescale).unwrap_or(self.fallback_time);
                    self.fallback_time = time + 1.0;
                    return Ok(Some(Frag { time, moof, mdat_pos: h.start, mdat_size: h.size }));
                }
                _ => {
                    // styp, sidx, emsg, prft, free...: not carried over.
                    self.r.seek(SeekFrom::Start(h.start + h.size))?;
                }
            }
        }
    }
}

/// `tfdt` base media decode time of the first track fragment, if present.
fn decode_time(moof: &[u8]) -> io::Result<Option<u64>> {
    for (kind, whole) in children(payload_of(moof))? {
        if &kind != b"traf" {
            continue;
        }
        for (k, b) in children(payload_of(&whole))? {
            if &k == b"tfdt" {
                let p = payload_of(&b);
                return Ok(Some(match p.first() {
                    Some(1) if p.len() >= 12 => u64::from_be_bytes(p[4..12].try_into().unwrap()),
                    _ if p.len() >= 8 => u32::from_be_bytes(p[4..8].try_into().unwrap()) as u64,
                    _ => return Ok(None),
                }));
            }
        }
    }
    Ok(None)
}

/// A copy of `moof` whose track fragments carry `track_id` instead.
fn relabel(moof: &[u8], track_id: u32) -> io::Result<Vec<u8>> {
    let mut out = Vec::with_capacity(moof.len());
    for (kind, whole) in children(payload_of(moof))? {
        if &kind != b"traf" {
            out.extend(whole);
            continue;
        }
        let mut traf = Vec::new();
        for (k, b) in children(payload_of(&whole))? {
            if &k == b"tfhd" {
                let mut p = payload_of(&b).to_vec();
                if p.len() < 8 {
                    return Err(err("A fragment header is too short"));
                }
                let flags = u32::from_be_bytes([0, p[1], p[2], p[3]]);
                if flags & 0x1 != 0 {
                    return Err(err("Fragments use absolute file offsets, which can't be moved into a joined file"));
                }
                p[4..8].copy_from_slice(&track_id.to_be_bytes());
                traf.extend(make_box(b"tfhd", &p));
            } else {
                traf.extend(b);
            }
        }
        out.extend(make_box(b"traf", &traf));
    }
    Ok(make_box(b"moof", &out))
}

fn copy_range(src: &mut File, pos: u64, len: u64, out: &mut impl Write) -> io::Result<()> {
    src.seek(SeekFrom::Start(pos))?;
    let mut remaining = len;
    let mut buf = vec![0u8; 256 * 1024];
    while remaining > 0 {
        let n = buf.len().min(remaining as usize);
        src.read_exact(&mut buf[..n])?;
        out.write_all(&buf[..n])?;
        remaining -= n as u64;
    }
    Ok(())
}

/// Size in bytes of the `ftyp` + `moov` header at the start of a track file.
pub fn init_length(path: &Path) -> io::Result<u64> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut r = BufReader::new(file);
    let mut end = 0;
    while let Some(h) = read_header(&mut r, len)? {
        if &h.kind == b"moof" || &h.kind == b"mdat" || &h.kind == b"styp" || &h.kind == b"sidx" {
            break;
        }
        end = h.start + h.size;
        r.seek(SeekFrom::Start(end))?;
    }
    if end == 0 {
        return Err(err("The track has no header"));
    }
    Ok(end)
}

/// Joins `video` and `audio` (fragmented MP4 files, each starting with its init segment) into `out`.
pub fn mux(video: &Path, audio: &Path, out: &Path) -> io::Result<()> {
    let v_len = init_length(video)?;
    let a_len = init_length(audio)?;
    let v_init = read_init(video, v_len)?;
    let a_init = read_init(audio, a_len)?;
    let (moov, audio_id) = merged_moov(&v_init, &a_init)?;
    let v_scale = timescale_of(traks(&v_init)[0])?;
    let a_scale = timescale_of(traks(&a_init)[0])?;

    let mut v = Frags::open(video, v_len, v_scale)?;
    let mut a = Frags::open(audio, a_len, a_scale)?;
    let mut v_file = File::open(video)?;
    let mut a_file = File::open(audio)?;

    let mut w = BufWriter::with_capacity(1 << 20, File::create(out)?);
    match &v_init.ftyp {
        Some(f) => w.write_all(f)?,
        None => w.write_all(&make_box(b"ftyp", b"isom\0\0\x02\0isomiso2iso6mp41"))?,
    }
    w.write_all(&moov)?;

    let mut next_v = v.next()?;
    let mut next_a = a.next()?;
    loop {
        let take_video = match (&next_v, &next_a) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(x), Some(y)) => x.time <= y.time,
        };
        if take_video {
            let f = next_v.take().unwrap();
            w.write_all(&f.moof)?;
            copy_range(&mut v_file, f.mdat_pos, f.mdat_size, &mut w)?;
            next_v = v.next()?;
        } else {
            let f = next_a.take().unwrap();
            w.write_all(&relabel(&f.moof, audio_id)?)?;
            copy_range(&mut a_file, f.mdat_pos, f.mdat_size, &mut w)?;
            next_a = a.next()?;
        }
    }
    w.flush()?;
    Ok(())
}

/// Does this file look like a fragmented MP4 (starts with ftyp/styp/moov)?
pub fn looks_like_fmp4(head: &[u8]) -> bool {
    head.len() >= 8 && matches!(&head[4..8], b"ftyp" | b"styp" | b"moov" | b"moof" | b"sidx")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32b(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    fn full(version: u8, body: &[u8]) -> Vec<u8> {
        let mut v = vec![version, 0, 0, 0];
        v.extend_from_slice(body);
        v
    }

    fn init(track_id: u32, timescale: u32, with_mehd: bool) -> Vec<u8> {
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&u32b(1000)); // timescale
        mvhd[96..100].copy_from_slice(&u32b(track_id + 1)); // next track id
        let mut tkhd = vec![0u8; 84];
        tkhd[12..16].copy_from_slice(&u32b(track_id));
        let mut mdhd = vec![0u8; 24];
        mdhd[12..16].copy_from_slice(&u32b(timescale));
        let mdia = make_box(b"mdia", &make_box(b"mdhd", &mdhd));
        let trak = make_box(b"trak", &[make_box(b"tkhd", &tkhd), mdia].concat());
        let mut trex = vec![0u8; 24];
        trex[4..8].copy_from_slice(&u32b(track_id));
        let mut mvex = Vec::new();
        if with_mehd {
            mvex.extend(make_box(b"mehd", &full(0, &u32b(5000))));
        }
        mvex.extend(make_box(b"trex", &trex));
        let moov = make_box(b"moov", &[make_box(b"mvhd", &mvhd), trak, make_box(b"mvex", &mvex)].concat());
        [make_box(b"ftyp", b"isomiso6"), moov].concat()
    }

    fn fragment(track_id: u32, time: u64, payload: &[u8]) -> Vec<u8> {
        let tfhd = make_box(b"tfhd", &[&[0u8, 2, 0, 0][..], &u32b(track_id)].concat()); // default-base-is-moof
        let mut tfdt = vec![1u8, 0, 0, 0];
        tfdt.extend_from_slice(&time.to_be_bytes());
        let traf = make_box(b"traf", &[tfhd, make_box(b"tfdt", &tfdt)].concat());
        let moof = make_box(b"moof", &[make_box(b"mfhd", &[0u8; 8]), traf].concat());
        [make_box(b"styp", b"msdh"), moof, make_box(b"mdat", payload)].concat()
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nexdrop_fmp4_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    fn top_level(data: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        children(data).unwrap()
    }

    #[test]
    fn joins_tracks_interleaved_by_time_with_audio_relabelled() {
        let video = tmp("v.mp4");
        let audio = tmp("a.m4a");
        // Video: timescale 1000, fragments at 0 s and 4 s. Audio: timescale 48000, at 0, 2 s, 4 s.
        let mut v = init(1, 1000, true);
        v.extend(fragment(1, 0, b"V0"));
        v.extend(fragment(1, 4000, b"V4"));
        let mut a = init(1, 48000, false);
        a.extend(fragment(1, 0, b"A0"));
        a.extend(fragment(1, 96000, b"A2"));
        a.extend(fragment(1, 192000, b"A4"));
        std::fs::write(&video, v).unwrap();
        std::fs::write(&audio, a).unwrap();

        let out = tmp("out.mp4");
        mux(&video, &audio, &out).unwrap();
        let data = std::fs::read(&out).unwrap();
        let boxes = top_level(&data);
        let kinds: Vec<String> = boxes.iter().map(|(k, _)| String::from_utf8_lossy(k).to_string()).collect();
        assert_eq!(kinds, ["ftyp", "moov", "moof", "mdat", "moof", "mdat", "moof", "mdat", "moof", "mdat", "moof", "mdat"]);

        // Header: two tracks (ids 1 and 2), two trex entries, no mehd, next track id 3.
        let moov = children(payload_of(&boxes[1].1)).unwrap();
        assert_eq!(moov.iter().filter(|(k, _)| k == b"trak").count(), 2);
        let ids: Vec<u32> = moov.iter().filter(|(k, _)| k == b"trak").map(|(_, t)| track_id_of(t).unwrap()).collect();
        assert_eq!(ids, [1, 2]);
        let mvex = children(payload_of(find(&moov, b"mvex").unwrap())).unwrap();
        assert!(find(&mvex, b"mehd").is_none());
        assert_eq!(mvex.iter().filter(|(k, _)| k == b"trex").count(), 2);
        let mvhd = payload_of(find(&moov, b"mvhd").unwrap()).to_vec();
        assert_eq!(&mvhd[96..100], &u32b(3));
        // Audio's timescale is untouched.
        let audio_trak = moov.iter().filter(|(k, _)| k == b"trak").nth(1).unwrap();
        assert_eq!(timescale_of(&audio_trak.1).unwrap(), 48000);

        // Fragments by time: V0, A0, A2, V4, A4, with payloads intact and audio relabelled.
        let mut order = Vec::new();
        for pair in boxes[2..].chunks(2) {
            let traf = children(payload_of(&pair[0].1)).unwrap().into_iter().find(|(k, _)| k == b"traf").unwrap();
            let tfhd = find(&children(payload_of(&traf.1)).unwrap(), b"tfhd").cloned().unwrap();
            let id = u32::from_be_bytes(payload_of(&tfhd)[4..8].try_into().unwrap());
            order.push((id, String::from_utf8_lossy(payload_of(&pair[1].1)).to_string()));
        }
        assert_eq!(order, [(1, "V0".to_string()), (2, "A0".to_string()), (2, "A2".to_string()), (1, "V4".to_string()), (2, "A4".to_string())]);
        let _ = std::fs::remove_dir_all(out.parent().unwrap());
    }

    #[test]
    fn absolute_offsets_and_broken_files_fail_cleanly() {
        let bad = tmp("bad.mp4");
        std::fs::write(&bad, b"not an mp4 at all, just text").unwrap();
        assert!(init_length(&bad).is_err());

        let tfhd = make_box(b"tfhd", &[&[0u8, 0, 0, 1][..], &u32b(1), &[0u8; 8]].concat()); // base-data-offset present
        let moof = make_box(b"moof", &make_box(b"traf", &tfhd));
        assert!(relabel(&moof, 2).is_err());
    }

    #[test]
    fn detects_fragmented_mp4() {
        assert!(looks_like_fmp4(&make_box(b"ftyp", b"isom")));
        assert!(!looks_like_fmp4(&[0x47, 0x40, 0x00, 0x10, 0, 0, 0, 0]));
    }
}
