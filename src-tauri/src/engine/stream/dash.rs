//! MPEG-DASH (`.mpd`) manifest parsing.
//!
//! Covers what real on-demand streams use: `SegmentTemplate` (with a
//! `SegmentTimeline` or a fixed duration), `SegmentList`, and a plain
//! `BaseURL` file. Live manifests and DRM-protected streams are detected and
//! reported rather than half-downloaded.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;
use url::Url;

use super::Segment;

#[derive(Debug, Clone)]
pub struct DashRep {
    pub id: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: Option<String>,
    /// "video", "audio" or something else (subtitles, images).
    pub content: String,
    pub language: Option<String>,
    pub init: Option<Segment>,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Default)]
pub struct Dash {
    pub duration: f64,
    pub live: bool,
    pub drm: bool,
    pub reps: Vec<DashRep>,
}

// ───────────────────────────── tiny XML tree ─────────────────────────────

#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }
}

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    s.rsplit(':').next().unwrap_or("").to_string()
}

fn node_from(e: &quick_xml::events::BytesStart) -> Node {
    let mut node = Node { name: local(e.name().as_ref()), ..Default::default() };
    for a in e.attributes().flatten() {
        let key = local(a.key.as_ref());
        let value = a.unescape_value().map(|v| v.to_string()).unwrap_or_default();
        node.attrs.insert(key, value);
    }
    node
}

fn parse_xml(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(node_from(&e)),
            Ok(Event::Empty(e)) => {
                let node = node_from(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.unescape().map(|v| v.to_string()).unwrap_or_default());
                }
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().ok_or("Malformed manifest")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("The manifest isn't valid XML: {e}")),
            _ => {}
        }
    }
    root.ok_or_else(|| "The manifest is empty".to_string())
}

// ───────────────────────────── helpers ─────────────────────────────

pub fn looks_like_dash(text: &str) -> bool {
    let head: String = text.trim_start_matches('\u{feff}').trim_start().chars().take(400).collect();
    head.contains("<MPD")
}

/// `PT1H2M3.5S` → seconds.
pub fn iso_duration(text: &str) -> Option<f64> {
    let rest = text.trim().strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut total = 0.0;
    let mut number = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let n: f64 = number.parse().ok()?;
            number.clear();
            total += match c {
                'Y' => n * 365.0 * 86400.0,
                'M' => n * 30.0 * 86400.0,
                'W' => n * 7.0 * 86400.0,
                'D' => n * 86400.0,
                _ => return None,
            };
        }
    }
    for c in time.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
        } else {
            let n: f64 = number.parse().ok()?;
            number.clear();
            total += match c {
                'H' => n * 3600.0,
                'M' => n * 60.0,
                'S' => n,
                _ => return None,
            };
        }
    }
    Some(total)
}

/// Fills `$RepresentationID$`, `$Number$`, `$Time$`, `$Bandwidth$` (with optional `%05d` widths).
fn expand(template: &str, rep_id: &str, bandwidth: u64, number: u64, time: u64) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('$') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('$') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let token = &after[..end];
        let (name, width) = match token.split_once('%') {
            Some((n, f)) => (n, f.trim_start_matches('0').trim_end_matches('d').parse::<usize>().unwrap_or(0)),
            None => (token, 0),
        };
        let pad = |v: u64| if width > 0 { format!("{v:0width$}") } else { v.to_string() };
        match name {
            "" => out.push('$'),
            "RepresentationID" => out.push_str(rep_id),
            "Number" => out.push_str(&pad(number)),
            "Time" => out.push_str(&pad(time)),
            "Bandwidth" => out.push_str(&pad(bandwidth)),
            _ => {
                out.push('$');
                out.push_str(token);
                out.push('$');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn join(base: &str, reference: &str) -> Result<String, String> {
    let base = Url::parse(base).map_err(|e| format!("Bad address in the manifest: {e}"))?;
    base.join(reference.trim()).map(|u| u.to_string()).map_err(|e| format!("Bad address in the manifest ({reference}): {e}"))
}

fn range_attr(value: &str) -> Option<(u64, u64)> {
    let (a, b) = value.split_once('-')?;
    let (a, b): (u64, u64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (b >= a).then_some((a, b - a + 1))
}

/// The template that applies to a representation: attributes on the
/// representation's own `SegmentTemplate` win over the adaptation set's.
fn merged_template<'a>(adaptation: &'a Node, rep: &'a Node) -> Option<(HashMap<&'a str, &'a str>, Option<&'a Node>)> {
    let a = adaptation.child("SegmentTemplate");
    let r = rep.child("SegmentTemplate");
    if a.is_none() && r.is_none() {
        return None;
    }
    let mut attrs: HashMap<&str, &str> = HashMap::new();
    for node in [a, r].into_iter().flatten() {
        for (k, v) in &node.attrs {
            attrs.insert(k.as_str(), v.as_str());
        }
    }
    let timeline = r.and_then(|n| n.child("SegmentTimeline")).or_else(|| a.and_then(|n| n.child("SegmentTimeline")));
    Some((attrs, timeline))
}

// ───────────────────────────── parser ─────────────────────────────

pub fn parse(text: &str, manifest_url: &str) -> Result<Dash, String> {
    let root = parse_xml(text.trim_start_matches('\u{feff}'))?;
    if root.name != "MPD" {
        return Err("That isn't a valid DASH manifest".to_string());
    }
    let total = root.attr("mediaPresentationDuration").and_then(iso_duration);
    let mut dash = Dash {
        live: root.attr("type").map(|t| t.eq_ignore_ascii_case("dynamic")).unwrap_or(false) && total.is_none(),
        ..Default::default()
    };

    let mut base = manifest_url.to_string();
    if let Some(b) = root.child("BaseURL") {
        base = join(&base, &b.text)?;
    }

    // Use the longest period (the main content, not a short ad or intro).
    let periods: Vec<&Node> = root.children_named("Period").collect();
    let period_len = |p: &Node| p.attr("duration").and_then(iso_duration).or(total).unwrap_or(0.0);
    let Some(period) = periods.iter().copied().max_by(|a, b| period_len(a).partial_cmp(&period_len(b)).unwrap_or(std::cmp::Ordering::Equal)) else {
        return Err("The manifest has no content".to_string());
    };
    let duration = period_len(period);
    dash.duration = duration;
    let mut period_base = base.clone();
    if let Some(b) = period.child("BaseURL") {
        period_base = join(&period_base, &b.text)?;
    }

    for set in period.children_named("AdaptationSet") {
        if set.child("ContentProtection").is_some() {
            dash.drm = true;
        }
        let mut set_base = period_base.clone();
        if let Some(b) = set.child("BaseURL") {
            set_base = join(&set_base, &b.text)?;
        }
        let set_mime = set.attr("mimeType").unwrap_or("");
        let set_content = set.attr("contentType").map(str::to_string).unwrap_or_else(|| set_mime.split('/').next().unwrap_or("").to_string());

        for rep in set.children_named("Representation") {
            if rep.child("ContentProtection").is_some() {
                dash.drm = true;
            }
            let mime = rep.attr("mimeType").unwrap_or(set_mime);
            let content = if !set_content.is_empty() {
                set_content.clone()
            } else {
                mime.split('/').next().unwrap_or("").to_string()
            };
            let id = rep.attr("id").unwrap_or("0").to_string();
            let bandwidth: u64 = rep.attr("bandwidth").and_then(|b| b.parse().ok()).unwrap_or(0);
            let mut rep_base = set_base.clone();
            if let Some(b) = rep.child("BaseURL") {
                rep_base = join(&rep_base, &b.text)?;
            }

            let mut out = DashRep {
                id: id.clone(),
                bandwidth,
                width: rep.attr("width").or(set.attr("width")).and_then(|v| v.parse().ok()).unwrap_or(0),
                height: rep.attr("height").or(set.attr("height")).and_then(|v| v.parse().ok()).unwrap_or(0),
                codecs: rep.attr("codecs").or(set.attr("codecs")).map(str::to_string),
                content,
                language: set.attr("lang").map(str::to_string),
                init: None,
                segments: Vec::new(),
            };

            let seg = |url: String, range: Option<(u64, u64)>, secs: f64| Segment { url, range, duration: secs, key: None, seq: 0 };

            if let Some((attrs, timeline)) = merged_template(set, rep) {
                let timescale: f64 = attrs.get("timescale").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0).max(1.0);
                let start_number: u64 = attrs.get("startNumber").and_then(|v| v.parse().ok()).unwrap_or(1);
                if let Some(init) = attrs.get("initialization") {
                    out.init = Some(seg(join(&rep_base, &expand(init, &id, bandwidth, 0, 0))?, None, 0.0));
                }
                let media = attrs.get("media").ok_or("A segment template has no media address")?;
                if let Some(timeline) = timeline {
                    let mut time: u64 = 0;
                    let mut number = start_number;
                    for s in timeline.children_named("S") {
                        if let Some(t) = s.attr("t").and_then(|t| t.parse().ok()) {
                            time = t;
                        }
                        let d: u64 = s.attr("d").and_then(|d| d.parse().ok()).ok_or("A timeline entry has no duration")?;
                        let repeat: i64 = s.attr("r").and_then(|r| r.parse().ok()).unwrap_or(0);
                        let times = if repeat < 0 {
                            // Repeats until the end of the period.
                            let remaining = (duration * timescale) as u64;
                            (remaining.saturating_sub(time) / d.max(1)).max(1)
                        } else {
                            repeat as u64 + 1
                        };
                        for _ in 0..times {
                            out.segments.push(seg(join(&rep_base, &expand(media, &id, bandwidth, number, time))?, None, d as f64 / timescale));
                            time += d;
                            number += 1;
                        }
                    }
                } else {
                    let d: f64 = attrs.get("duration").and_then(|v| v.parse().ok()).ok_or("A segment template has no duration or timeline")?;
                    let seconds = d / timescale;
                    if seconds <= 0.0 || duration <= 0.0 {
                        return Err("The manifest doesn't say how long the video is".to_string());
                    }
                    let count = (duration / seconds).ceil() as u64;
                    for i in 0..count {
                        out.segments.push(seg(join(&rep_base, &expand(media, &id, bandwidth, start_number + i, i * d as u64))?, None, seconds));
                    }
                }
            } else if let Some(list) = rep.child("SegmentList").or_else(|| set.child("SegmentList")) {
                if let Some(init) = list.child("Initialization") {
                    let src = init.attr("sourceURL").map(str::to_string).unwrap_or_default();
                    out.init = Some(seg(if src.is_empty() { rep_base.clone() } else { join(&rep_base, &src)? }, init.attr("range").and_then(range_attr), 0.0));
                }
                let timescale: f64 = list.attr("timescale").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0).max(1.0);
                let d: f64 = list.attr("duration").and_then(|v| v.parse().ok()).map(|d: f64| d / timescale).unwrap_or(0.0);
                for u in list.children_named("SegmentURL") {
                    let url = match u.attr("media") {
                        Some(m) => join(&rep_base, m)?,
                        None => rep_base.clone(),
                    };
                    out.segments.push(seg(url, u.attr("mediaRange").and_then(range_attr), d));
                }
            } else if !rep.text.trim().is_empty() || rep.child("BaseURL").is_some() || rep.child("SegmentBase").is_some() {
                // One file for the whole track.
                out.segments.push(seg(rep_base.clone(), None, duration));
            }

            if !out.segments.is_empty() {
                dash.reps.push(out);
            }
        }
    }
    if dash.reps.is_empty() {
        return Err("The manifest lists no downloadable media".to_string());
    }
    Ok(dash)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT20S">
 <BaseURL>media/</BaseURL>
 <Period>
  <AdaptationSet mimeType="video/mp4" contentType="video">
   <SegmentTemplate timescale="1000" duration="4000" startNumber="1" media="$RepresentationID$/seg-$Number%03d$.m4s" initialization="$RepresentationID$/init.mp4"/>
   <Representation id="v720" bandwidth="2000000" width="1280" height="720" codecs="avc1.4d401f"/>
   <Representation id="v360" bandwidth="500000" width="640" height="360"/>
  </AdaptationSet>
  <AdaptationSet mimeType="audio/mp4" lang="en">
   <Representation id="a1" bandwidth="128000">
    <SegmentTemplate timescale="48000" media="a/$Time$.m4a" initialization="a/init.mp4">
     <SegmentTimeline><S t="0" d="96000" r="2"/><S d="48000"/></SegmentTimeline>
    </SegmentTemplate>
   </Representation>
  </AdaptationSet>
 </Period>
</MPD>"#;

    #[test]
    fn iso_durations() {
        assert_eq!(iso_duration("PT20S"), Some(20.0));
        assert_eq!(iso_duration("PT1H2M3.5S"), Some(3723.5));
        assert_eq!(iso_duration("P1DT1H"), Some(90000.0));
        assert_eq!(iso_duration("junk"), None);
    }

    #[test]
    fn template_expansion() {
        assert_eq!(expand("$RepresentationID$/s-$Number%05d$.m4s", "v1", 5, 42, 0), "v1/s-00042.m4s");
        assert_eq!(expand("t$Time$_$Bandwidth$_$$", "x", 800, 1, 9000), "t9000_800_$");
        assert_eq!(expand("no-vars.mp4", "x", 0, 0, 0), "no-vars.mp4");
    }

    #[test]
    fn parses_templates_with_duration_and_timeline() {
        let d = parse(MPD, "https://cdn.example.com/show/manifest.mpd").unwrap();
        assert!(!d.live && !d.drm);
        assert_eq!(d.duration, 20.0);
        let v = d.reps.iter().find(|r| r.id == "v720").unwrap();
        assert_eq!((v.width, v.height, v.content.as_str()), (1280, 720, "video"));
        assert_eq!(v.segments.len(), 5, "20 s in 4 s pieces");
        assert_eq!(v.segments[0].url, "https://cdn.example.com/show/media/v720/seg-001.m4s");
        assert_eq!(v.segments[4].url, "https://cdn.example.com/show/media/v720/seg-005.m4s");
        assert_eq!(v.init.as_ref().unwrap().url, "https://cdn.example.com/show/media/v720/init.mp4");

        let a = d.reps.iter().find(|r| r.id == "a1").unwrap();
        assert_eq!(a.content, "audio");
        assert_eq!(a.segments.len(), 4, "r=2 means three, plus one more");
        assert_eq!(a.segments[1].url, "https://cdn.example.com/show/media/a/96000.m4a");
        assert_eq!(a.segments[3].url, "https://cdn.example.com/show/media/a/288000.m4a");
        assert!((a.segments[0].duration - 2.0).abs() < 1e-9);
    }

    #[test]
    fn segment_list_and_single_file() {
        let mpd = r#"<MPD type="static" mediaPresentationDuration="PT10S"><Period>
          <AdaptationSet mimeType="video/mp4">
            <Representation id="a" bandwidth="1">
              <BaseURL>video.mp4</BaseURL>
              <SegmentList duration="5"><Initialization sourceURL="i.mp4" range="0-99"/><SegmentURL media="s1.m4s"/><SegmentURL media="s2.m4s" mediaRange="100-199"/></SegmentList>
            </Representation>
            <Representation id="b" bandwidth="2"><BaseURL>whole.mp4</BaseURL><SegmentBase indexRange="0-10"/></Representation>
          </AdaptationSet></Period></MPD>"#;
        let d = parse(mpd, "https://h.example/m.mpd").unwrap();
        let a = d.reps.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(a.segments.len(), 2);
        assert_eq!(a.init.as_ref().unwrap().range, Some((0, 100)));
        assert_eq!(a.segments[1].range, Some((100, 100)));
        let b = d.reps.iter().find(|r| r.id == "b").unwrap();
        assert_eq!(b.segments.len(), 1);
        assert_eq!(b.segments[0].url, "https://h.example/whole.mp4");
    }

    #[test]
    fn live_and_drm_are_flagged() {
        let live = r#"<MPD type="dynamic"><Period><AdaptationSet mimeType="video/mp4"><SegmentTemplate media="s$Number$.m4s" duration="2" startNumber="1"/><Representation id="x" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        // A live manifest has no total duration, so segment counts can't be known: it must fail or be flagged, never loop forever.
        match parse(live, "https://h/m.mpd") {
            Ok(d) => assert!(d.live),
            Err(_) => {}
        }
        let drm = r#"<MPD type="static" mediaPresentationDuration="PT8S"><Period><AdaptationSet mimeType="video/mp4"><ContentProtection schemeIdUri="urn:uuid:edef8ba9"/><SegmentTemplate media="s$Number$.m4s" duration="4" startNumber="1"/><Representation id="x" bandwidth="1"/></AdaptationSet></Period></MPD>"#;
        assert!(parse(drm, "https://h/m.mpd").unwrap().drm);
    }

    #[test]
    fn non_dash_is_rejected() {
        assert!(parse("<html></html>", "https://h/m.mpd").is_err());
        assert!(parse("not xml at all <<<", "https://h/m.mpd").is_err());
        assert!(looks_like_dash("<?xml version=\"1.0\"?>\n<MPD xmlns=\"x\">"));
        assert!(!looks_like_dash("#EXTM3U"));
    }
}
