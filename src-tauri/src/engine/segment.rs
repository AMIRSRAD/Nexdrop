use crate::db::models::DownloadSegment;

pub fn calculate_segments(total_bytes: i64, requested_connections: usize) -> Vec<DownloadSegment> {
    calculate_segments_with_chunk_size(total_bytes, requested_connections, 512 * 1024)
}

pub fn calculate_segments_with_chunk_size(
    total_bytes: i64,
    requested_connections: usize,
    min_chunk_size: u64,
) -> Vec<DownloadSegment> {
    if total_bytes <= 0 || requested_connections <= 1 {
        return vec![DownloadSegment {
            segment_index: 0,
            start_byte: 0,
            end_byte: if total_bytes > 0 { (total_bytes - 1) as u64 } else { 0 },
            downloaded_bytes: 0,
            total_bytes: if total_bytes > 0 { total_bytes as u64 } else { 0 },
            status: "pending".to_string(),
        }];
    }

    let total = total_bytes as u64;
    let min_chunk = min_chunk_size.max(64 * 1024); // at least 64KB per chunk

    // Calculate maximum connections that respect min_chunk_size
    let max_chunks_by_size = (total / min_chunk).max(1) as usize;
    let connections = requested_connections.min(max_chunks_by_size).clamp(1, 32);

    let part_size = total / (connections as u64);
    let mut segments = Vec::with_capacity(connections);

    for i in 0..connections {
        let start = (i as u64) * part_size;
        let end = if i == connections - 1 {
            total - 1
        } else {
            ((i as u64) + 1) * part_size - 1
        };

        segments.push(DownloadSegment {
            segment_index: i,
            start_byte: start,
            end_byte: end,
            downloaded_bytes: 0,
            total_bytes: end - start + 1,
            status: "pending".to_string(),
        });
    }

    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_calculation() {
        let segs = calculate_segments(1000, 4);
        // Small file < 512KB with 512KB min chunk defaults to 1 connection
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start_byte, 0);
        assert_eq!(segs[0].end_byte, 999);

        let large_segs = calculate_segments(100_000_000, 4);
        assert_eq!(large_segs.len(), 4);
        assert_eq!(large_segs[0].start_byte, 0);
        assert_eq!(large_segs[3].end_byte, 99_999_999);

        // Verify continuity with no gaps and no overlap
        for i in 0..large_segs.len() - 1 {
            assert_eq!(large_segs[i].end_byte + 1, large_segs[i + 1].start_byte);
        }
    }

    #[test]
    fn test_chunk_size_threshold() {
        // 5MB file with 2MB min chunk can have at most 2 chunks even if 8 requested
        let segs = calculate_segments_with_chunk_size(5 * 1024 * 1024, 8, 2 * 1024 * 1024);
        assert_eq!(segs.len(), 2);
    }
}
