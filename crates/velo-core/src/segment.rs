use crate::types::{SegmentState, MAX_SEGMENTS, MIN_SPLIT_BYTES};

/// Split [0, total) into `n` roughly equal ranges.
/// Small files get one segment: splitting them costs more handshakes than it saves.
pub fn plan_segments(total: Option<u64>, requested: u8, supports_range: bool) -> Vec<SegmentState> {
    let total = match total {
        Some(t) if t > 0 => t,
        // Unknown size: single streaming segment with an open end.
        _ => return vec![SegmentState::new(0, 0, u64::MAX)],
    };

    let n = if !supports_range || total < MIN_SPLIT_BYTES {
        1
    } else {
        let requested = requested.clamp(1, MAX_SEGMENTS) as u64;
        // Never make a segment smaller than 512 KiB.
        let by_size = (total / (512 * 1024)).max(1);
        requested.min(by_size) as u64
    };

    let n = n.max(1);
    let chunk = total / n;
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let start = i * chunk;
        let end = if i == n - 1 { total } else { start + chunk };
        out.push(SegmentState::new(i as u16, start, end));
    }
    out
}

/// Pick the segment worth stealing from: the one with the most bytes left.
/// Returns its index and the split point, or None if nothing is worth splitting.
pub fn find_steal_target(segments: &[SegmentState], min_steal: u64) -> Option<(usize, u64)> {
    let (idx, seg) = segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.remaining() >= min_steal * 2 && s.end != u64::MAX)
        .max_by_key(|(_, s)| s.remaining())?;

    // Cut the remaining work in half, aligned so both halves stay useful.
    let split = seg.cursor + seg.remaining() / 2;
    if split <= seg.cursor || split >= seg.end {
        return None;
    }
    Some((idx, split))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_file_is_one_segment() {
        let s = plan_segments(Some(1000), 8, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].end, 1000);
    }

    #[test]
    fn no_range_support_is_one_segment() {
        assert_eq!(plan_segments(Some(100_000_000), 8, false).len(), 1);
    }

    #[test]
    fn splits_evenly_and_covers_everything() {
        let s = plan_segments(Some(100_000_000), 8, true);
        assert_eq!(s.len(), 8);
        assert_eq!(s[0].start, 0);
        assert_eq!(s.last().unwrap().end, 100_000_000);
        for w in s.windows(2) {
            assert_eq!(w[0].end, w[1].start);
        }
    }

    #[test]
    fn unknown_size_streams() {
        let s = plan_segments(None, 8, true);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].end, u64::MAX);
    }

    #[test]
    fn steals_from_the_slowest() {
        let segs = vec![
            SegmentState {
                index: 0,
                start: 0,
                end: 10,
                cursor: 10,
            },
            SegmentState {
                index: 1,
                start: 10,
                end: 10_000_000,
                cursor: 20,
            },
        ];
        let (idx, split) = find_steal_target(&segs, 1024).unwrap();
        assert_eq!(idx, 1);
        assert!(split > 20 && split < 10_000_000);
    }

    #[test]
    fn wont_steal_tiny_leftovers() {
        let segs = vec![SegmentState {
            index: 0,
            start: 0,
            end: 1000,
            cursor: 0,
        }];
        assert!(find_steal_target(&segs, 1024).is_none());
    }
}
