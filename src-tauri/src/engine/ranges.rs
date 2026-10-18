//! Byte-range bookkeeping for the segmented downloader.
//!
//! Everything here is plain data manipulation (no I/O except the small
//! sidecar file), so it can be tested exhaustively. Ranges are half-open:
//! `(start, end)` covers bytes `start..end`.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub type Range = (u64, u64);

/// The set of byte ranges that are known to be written to disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DoneMap {
    ranges: Vec<Range>,
}

impl DoneMap {
    pub fn from_ranges(mut ranges: Vec<Range>) -> Self {
        ranges.retain(|(s, e)| s < e);
        ranges.sort_unstable();
        let mut map = DoneMap::default();
        for (s, e) in ranges {
            map.insert(s, e);
        }
        map
    }

    /// Adds `start..end`, merging with anything it touches or overlaps.
    pub fn insert(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let (mut new_start, mut new_end) = (start, end);
        let mut out = Vec::with_capacity(self.ranges.len() + 1);
        let mut placed = false;
        for &(s, e) in &self.ranges {
            if e < new_start {
                out.push((s, e));
            } else if s > new_end {
                if !placed {
                    out.push((new_start, new_end));
                    placed = true;
                }
                out.push((s, e));
            } else {
                new_start = new_start.min(s);
                new_end = new_end.max(e);
            }
        }
        if !placed {
            out.push((new_start, new_end));
        }
        self.ranges = out;
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    /// Total number of bytes covered.
    pub fn covered(&self) -> u64 {
        self.ranges.iter().map(|(s, e)| e - s).sum()
    }

    pub fn is_complete(&self, total: u64) -> bool {
        total == 0 || self.ranges.len() == 1 && self.ranges[0].0 == 0 && self.ranges[0].1 >= total
    }

    /// The parts of `0..total` that are *not* covered yet.
    pub fn gaps(&self, total: u64) -> Vec<Range> {
        let mut gaps = Vec::new();
        let mut cursor = 0u64;
        for &(s, e) in &self.ranges {
            if s > cursor {
                gaps.push((cursor, s.min(total)));
            }
            cursor = cursor.max(e);
            if cursor >= total {
                break;
            }
        }
        if cursor < total {
            gaps.push((cursor, total));
        }
        gaps.retain(|(s, e)| s < e);
        gaps
    }

    /// Drops anything beyond `total` (a defence against a corrupt sidecar).
    pub fn clamp(&mut self, total: u64) {
        self.ranges.retain(|(s, _)| *s < total);
        for r in &mut self.ranges {
            r.1 = r.1.min(total);
        }
    }
}

/// Cuts the missing `gaps` into about `parts` pieces (never smaller than
/// `min_piece`), so each connection starts with a sizeable range of its own.
pub fn split_gaps(gaps: &[Range], parts: usize, min_piece: u64) -> Vec<Range> {
    let missing: u64 = gaps.iter().map(|(s, e)| e - s).sum();
    if missing == 0 {
        return Vec::new();
    }
    let piece = (missing / parts.max(1) as u64).max(min_piece).max(1);
    let mut out = Vec::new();
    for &(s, e) in gaps {
        let mut cur = s;
        while cur < e {
            let mut next = (cur + piece).min(e);
            // Don't leave a sliver at the end of a gap.
            if e - next < piece / 2 {
                next = e;
            }
            out.push((cur, next));
            cur = next;
        }
    }
    out
}

/// Where to cut an in-flight range `pos..end` so another connection can take
/// the upper half. `None` if too little is left to be worth a new request.
pub fn steal_point(pos: u64, end: u64, min_steal: u64) -> Option<u64> {
    let remaining = end.saturating_sub(pos);
    if remaining >= min_steal.saturating_mul(2) {
        Some(pos + remaining / 2)
    } else {
        None
    }
}

// ───────────────────────────── sidecar file ─────────────────────────────

#[derive(Serialize, Deserialize)]
struct MapFile {
    version: u32,
    total: u64,
    done: Vec<Range>,
}

/// Loads the saved progress, but only if it was written for a file of the
/// same size. Anything unreadable or mismatched means "start over".
pub fn load_map(path: &Path, total: u64) -> Option<DoneMap> {
    let text = std::fs::read_to_string(path).ok()?;
    let file: MapFile = serde_json::from_str(&text).ok()?;
    if file.version != 1 || file.total != total {
        return None;
    }
    let mut map = DoneMap::from_ranges(file.done);
    map.clamp(total);
    Some(map)
}

/// Writes the map atomically (temp file + rename) so a crash can never leave
/// a half-written sidecar behind.
pub fn save_map(path: &Path, total: u64, map: &DoneMap) -> io::Result<()> {
    let body = serde_json::to_string(&MapFile { version: 1, total, done: map.ranges().to_vec() })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let tmp = path.with_extension("map.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_merges_touching_and_overlapping() {
        let mut m = DoneMap::default();
        m.insert(10, 20);
        m.insert(30, 40);
        assert_eq!(m.ranges(), &[(10, 20), (30, 40)]);
        m.insert(20, 30); // bridges the gap exactly
        assert_eq!(m.ranges(), &[(10, 40)]);
        m.insert(0, 5);
        m.insert(5, 12);
        assert_eq!(m.ranges(), &[(0, 40)]);
        m.insert(35, 100);
        assert_eq!(m.ranges(), &[(0, 100)]);
        assert_eq!(m.covered(), 100);
    }

    #[test]
    fn gaps_are_the_complement() {
        let m = DoneMap::from_ranges(vec![(10, 20), (30, 40)]);
        assert_eq!(m.gaps(50), vec![(0, 10), (20, 30), (40, 50)]);
        assert_eq!(m.gaps(40), vec![(0, 10), (20, 30)]);
        assert!(DoneMap::default().gaps(0).is_empty());
        assert_eq!(DoneMap::default().gaps(7), vec![(0, 7)]);
    }

    #[test]
    fn completeness() {
        let mut m = DoneMap::default();
        assert!(!m.is_complete(100));
        m.insert(0, 60);
        m.insert(60, 100);
        assert!(m.is_complete(100));
        assert!(!m.is_complete(101));
    }

    #[test]
    fn split_covers_everything_exactly_once() {
        let gaps = vec![(0, 1_000_003), (2_000_000, 2_500_000)];
        for parts in [1usize, 3, 8, 32] {
            let pieces = split_gaps(&gaps, parts, 1_000);
            let mut m = DoneMap::default();
            let mut sum = 0;
            for (s, e) in &pieces {
                assert!(s < e);
                m.insert(*s, *e);
                sum += e - s;
            }
            assert_eq!(sum, 1_500_003, "pieces must not overlap (parts={parts})");
            assert_eq!(m.covered(), 1_500_003);
        }
    }

    #[test]
    fn split_respects_minimum_piece() {
        let pieces = split_gaps(&[(0, 10_000)], 32, 4_000);
        // 32 parts were requested, but no piece (except the tail) may be below the minimum.
        assert_eq!(pieces.len(), 3, "{pieces:?}");
        assert!(pieces[..pieces.len() - 1].iter().all(|(s, e)| e - s >= 4_000), "{pieces:?}");
    }

    #[test]
    fn steal_point_halves_the_remainder() {
        assert_eq!(steal_point(0, 100, 10), Some(50));
        assert_eq!(steal_point(40, 100, 10), Some(70));
        assert_eq!(steal_point(0, 19, 10), None);
        assert_eq!(steal_point(100, 100, 1), None);
    }

    #[test]
    fn sidecar_round_trip_and_validation() {
        let dir = std::env::temp_dir().join(format!("nexdrop_map_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.nexpart.map");
        let map = DoneMap::from_ranges(vec![(0, 10), (50, 60)]);
        save_map(&path, 100, &map).unwrap();

        assert_eq!(load_map(&path, 100), Some(map));
        assert_eq!(load_map(&path, 101), None, "different size means a different file");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load_map(&path, 100), None);
        assert_eq!(load_map(&dir.join("missing"), 100), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
