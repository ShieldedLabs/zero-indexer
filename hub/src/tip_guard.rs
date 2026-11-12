//! Which mainnet heights are plausible, and how fast the chain can climb.
//!
//! The hub learns the tip from its indexers and cannot check a height itself:
//! `CompactTxStreamer` serves no headers to verify work against. So a height is
//! judged against the only witnesses the enclave has that an indexer does not
//! control: a compiled anchor and the enclave's own clocks. Both bounds here
//! are mainnet-only. Testnet's minimum-difficulty rule lets blocks arrive in
//! bursts these bounds would refuse.
//!
//! The bounds limit how far one lying indexer can move the hub. The module doc
//! of `batcher.rs` lists what it can still do inside them.

// See queue.rs: a panic here is a fleet-wide privacy event, not a crash.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A known mainnet block and its time: the NU7 activation block, where the
/// target spacing dropped from 75 s to 25 s. The chain averaged 25.2 s a block
/// over its first week, which only adds room under the ceiling. A fresher
/// anchor tightens [`mainnet_band`].
pub const ANCHOR_HEIGHT: u32 = 3_543_405;
pub const ANCHOR_EPOCH: u64 = 1_793_932_442;

/// Target block spacing from NU7 on. It was 75 s from Blossom to NU7.
pub const TARGET_BLOCK_SECS: u64 = 25;

/// Standard deviations of a Poisson block count the bounds allow. Five puts a
/// false refusal near 3e-7 per check.
const SIGMA: f64 = 5.0;

/// Seconds since the Unix epoch, or `None` for a clock set before it.
pub fn unix_now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|since| since.as_secs())
}

/// The inclusive range of heights mainnet can plausibly be at, at `unix_secs`.
/// `None` before the anchor: a clock that early is wrong, and nothing can be
/// judged against it.
///
/// The ceiling is the anchor plus one block per 25 s, plus a SIGMA Poisson
/// walk since the anchor, 0.2% for drift, and 200
/// blocks for propagation. The floor is the anchor itself. A floor that rose
/// with the clock would refuse the real tip after a halt longer than its
/// slack; an indexer still syncing far behind sits in the band, and the
/// tracker's warm-up follows it as it catches up.
pub fn mainnet_band(unix_secs: u64) -> Option<(u32, u32)> {
    let blocks = unix_secs.checked_sub(ANCHOR_EPOCH)? / TARGET_BLOCK_SECS;
    let slack = (SIGMA * (blocks as f64).sqrt()) as u64 + blocks / 500 + 200;
    let ceiling = u64::from(ANCHOR_HEIGHT) + blocks + slack;
    Some((ANCHOR_HEIGHT, u32::try_from(ceiling).unwrap_or(u32::MAX)))
}

/// The most blocks mainnet plausibly produces in `elapsed`: the Poisson mean
/// plus SIGMA standard deviations, plus three so a short window still allows a
/// small burst. 30 s allows 9, 25 minutes 101, 24 hours 3752.
pub fn max_new_blocks(elapsed: Duration) -> u32 {
    let mean = elapsed.as_secs_f64() / TARGET_BLOCK_SECS as f64;
    // `as` saturates a float into the integer range.
    (mean + SIGMA * mean.sqrt() + 3.0) as u32
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
pub(crate) mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60;

    /// 460 mainnet blocks in a row, mined on 2026-11-09 after NU7 cut the
    /// spacing to 25 s. One `height unix_time` per line.
    pub(crate) fn mainnet_nu7_blocks() -> Vec<(u32, u64)> {
        include_str!("../tests/fixtures/mainnet_nu7_block_times.txt")
            .lines()
            .map(|line| {
                let (height, time) = line.split_once(' ').unwrap();
                (height.parse().unwrap(), time.parse().unwrap())
            })
            .collect()
    }

    #[test]
    fn the_band_at_the_anchor_runs_from_it_to_the_fixed_slack_above() {
        assert_eq!(
            mainnet_band(ANCHOR_EPOCH),
            Some((ANCHOR_HEIGHT, ANCHOR_HEIGHT + 200))
        );
    }

    #[test]
    fn a_clock_before_the_anchor_has_no_band() {
        assert_eq!(mainnet_band(ANCHOR_EPOCH - 1), None);
        assert_eq!(mainnet_band(0), None);
    }

    #[test]
    fn the_ceiling_rises_with_age_and_tracks_the_target_rate() {
        // Sixty days on: 207,360 blocks, slack 2276 + 414 + 200.
        let (_, high) = mainnet_band(ANCHOR_EPOCH + 60 * DAY).unwrap();
        assert_eq!(high, ANCHOR_HEIGHT + 207_360 + 2890);
    }

    #[test]
    fn the_floor_stays_at_the_anchor_so_a_long_halt_stays_in_the_band() {
        // A chain halted since the anchor is a year behind schedule and still
        // in the band.
        let (low, _) = mainnet_band(ANCHOR_EPOCH + 365 * DAY).unwrap();
        assert_eq!(low, ANCHOR_HEIGHT);
    }

    #[test]
    fn the_band_never_reaches_the_heights_that_brick_the_hub() {
        // A century on, the ceiling is still nowhere near u32::MAX, so a height
        // that saturates the flush arithmetic is always refused.
        let (_, high) = mainnet_band(ANCHOR_EPOCH + 100 * 365 * DAY).unwrap();
        assert!(high < 200_000_000, "ceiling {high}");
    }

    #[test]
    fn max_new_blocks_at_the_windows_the_tracker_uses() {
        assert_eq!(max_new_blocks(Duration::ZERO), 3);
        assert_eq!(max_new_blocks(Duration::from_secs(30)), 9);
        assert_eq!(max_new_blocks(Duration::from_secs(25 * 60)), 101);
        assert_eq!(max_new_blocks(Duration::from_secs(DAY)), 3752);
    }

    #[test]
    fn max_new_blocks_never_shrinks_as_the_window_grows() {
        let mut previous = 0;
        for secs in (0..=2 * DAY).step_by(15) {
            let bound = max_new_blocks(Duration::from_secs(secs));
            assert!(bound >= previous, "{bound} < {previous} at {secs} s");
            previous = bound;
        }
    }

    #[test]
    fn max_new_blocks_stays_above_the_mean_rate() {
        // Every window allows at least the blocks the target rate produces in
        // it, so an honest chain on schedule always passes.
        for secs in (0..=2 * DAY).step_by(25) {
            let mean = (secs / TARGET_BLOCK_SECS) as u32;
            assert!(max_new_blocks(Duration::from_secs(secs)) > mean);
        }
    }

    #[test]
    fn every_real_post_nu7_height_is_inside_the_band_at_its_block_time() {
        for (height, time) in mainnet_nu7_blocks() {
            let (low, high) = mainnet_band(time).unwrap();
            assert!(
                (low..=high).contains(&height),
                "real height {height} at {time} is outside the band {low}..={high}"
            );
        }
    }

    #[test]
    fn no_real_post_nu7_climb_is_faster_than_max_new_blocks_allows() {
        let blocks = mainnet_nu7_blocks();
        for (i, &(from, start)) in blocks.iter().enumerate() {
            for &(to, end) in &blocks[i + 1..] {
                let secs = end.saturating_sub(start);
                let allowed = max_new_blocks(Duration::from_secs(secs));
                assert!(
                    to - from <= allowed,
                    "mainnet made {} blocks in {secs} s from {from}, and {allowed} are allowed",
                    to - from
                );
            }
        }
    }
}
