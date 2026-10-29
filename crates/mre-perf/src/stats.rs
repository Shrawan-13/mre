//! Frame timing statistics.
//!
//! The mean is reported but never trusted: a renderer can hit a 4 ms mean while
//! stuttering at 30 ms every 40th frame. Percentiles are the contract.

/// Percentile summary of a sample window, in nanoseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub count: usize,
    pub min_ns: u64,
    pub mean_ns: u64,
    pub p50_ns: u64,
    pub p90_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub p999_ns: u64,
    pub max_ns: u64,
}

impl Summary {
    /// Frames per second implied by the mean frame time.
    #[must_use]
    pub fn mean_hz(&self) -> f64 {
        if self.mean_ns == 0 {
            0.0
        } else {
            1e9 / self.mean_ns as f64
        }
    }

    /// Frames per second sustainable at the p99 — the number that matters.
    #[must_use]
    pub fn p99_hz(&self) -> f64 {
        if self.p99_ns == 0 {
            0.0
        } else {
            1e9 / self.p99_ns as f64
        }
    }

    /// `p99 / budget`. Values `<= 1.0` mean no jank at that budget.
    #[must_use]
    pub fn p99_ratio(&self, budget_ns: u64) -> f64 {
        if budget_ns == 0 {
            0.0
        } else {
            self.p99_ns as f64 / budget_ns as f64
        }
    }

    /// `p999 / budget`. The contract allows up to `p999_ratio` here.
    #[must_use]
    pub fn p999_ratio(&self, budget_ns: u64) -> f64 {
        if budget_ns == 0 {
            0.0
        } else {
            self.p999_ns as f64 / budget_ns as f64
        }
    }

    /// `true` when the window violated the contract at the given budget.
    #[must_use]
    pub fn exceeds(&self, budget_ns: u64, p99_ratio: f64, p999_ratio: f64) -> bool {
        self.count == 0
            || self.p99_ns as f64 > budget_ns as f64 * p99_ratio
            || self.p999_ns as f64 > budget_ns as f64 * p999_ratio
    }
}

/// A growing-then-reused sample buffer.
///
/// Reserve once during warm-up, then [`Stats::record`] never allocates — which
/// matters, because the timing rig is itself subject to the allocation gate.
pub struct Stats {
    samples: Vec<u64>,
}

impl Stats {
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn record(&mut self, ns: u64) {
        self.samples.push(ns);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Sorts the window in place and summarises it.
    pub fn summary(&mut self) -> Summary {
        if self.samples.is_empty() {
            return Summary::default();
        }
        self.samples.sort_unstable();
        let count = self.samples.len();
        let sum: u64 = self.samples.iter().copied().sum();
        Summary {
            count,
            min_ns: self.samples[0],
            mean_ns: sum / count as u64,
            p50_ns: percentile(&self.samples, 0.50),
            p90_ns: percentile(&self.samples, 0.90),
            p95_ns: percentile(&self.samples, 0.95),
            p99_ns: percentile(&self.samples, 0.99),
            p999_ns: percentile(&self.samples, 0.999),
            max_ns: self.samples[count - 1],
        }
    }
}

/// Nearest-rank percentile over a sorted, non-empty slice.
fn percentile(sorted: &[u64], p: f64) -> u64 {
    debug_assert!(!sorted.is_empty());
    let last = sorted.len() - 1;
    // Nearest-rank: ceil(p * n) - 1, clamped. Chosen over interpolation because
    // interpolated p99s are not real frames and cannot be reproduced from a log.
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(last)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_summary_is_zero() {
        let mut s = Stats::with_capacity(4);
        assert_eq!(s.summary(), Summary::default());
    }

    #[test]
    fn percentiles_are_ordered() {
        let mut s = Stats::with_capacity(100);
        for i in 1..=100u64 {
            s.record(i * 1_000);
        }
        let sum = s.summary();
        assert_eq!(sum.count, 100);
        assert_eq!(sum.min_ns, 1_000);
        assert_eq!(sum.max_ns, 100_000);
        assert!(sum.p50_ns <= sum.p90_ns);
        assert!(sum.p90_ns <= sum.p95_ns);
        assert!(sum.p95_ns <= sum.p99_ns);
        assert!(sum.p99_ns <= sum.p999_ns);
        assert!(sum.p999_ns <= sum.max_ns);
    }

    #[test]
    fn p99_and_p999_tell_different_stories() {
        let mut s = Stats::with_capacity(100);
        for _ in 0..99 {
            s.record(1_000_000); // 1 ms: perfectly smooth
        }
        s.record(50_000_000); // one 50 ms hitch
        let sum = s.summary();

        assert_eq!(sum.mean_ns, 1_490_000, "mean hides the hitch");
        assert_eq!(sum.max_ns, 50_000_000);
        // One hitch in 100 frames is invisible to p99 but fatal to p999. This is
        // why the contract gates on both, and why a mean is never enough.
        assert!(
            sum.p99_ratio(8_333_333) <= 1.0,
            "p99 should miss a single spike"
        );
        assert!(
            sum.p999_ratio(8_333_333) > 1.0,
            "p999 must catch a single spike"
        );
        assert!(
            sum.exceeds(8_333_333, 1.0, 2.0),
            "contract must reject this window"
        );
    }

    #[test]
    fn ratio_maths() {
        let w = Summary {
            p99_ns: 4_000_000,
            ..Summary::default()
        };
        assert!((w.p99_ratio(8_000_000) - 0.5).abs() < 1e-9);
        assert_eq!(w.p99_ratio(0), 0.0);
    }
}
