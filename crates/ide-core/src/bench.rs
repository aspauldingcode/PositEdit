//! First-party micro-benchmark helpers (no Criterion dependency).
//!
//! Methodology matches `benchmarks/BUDGETS.md`: ≥5 samples, median + p95.

use std::time::{Duration, Instant};

/// Default sample count for built-in benches.
pub const DEFAULT_SAMPLES: usize = 9;
/// Warmup iterations discarded before timing.
pub const DEFAULT_WARMUP: usize = 2;

#[derive(Clone, Debug)]
pub struct SampleStats {
    pub name: String,
    pub samples_ns: Vec<u64>,
    pub median_ns: u64,
    pub p95_ns: u64,
    pub min_ns: u64,
    pub max_ns: u64,
    pub mean_ns: u64,
}

impl SampleStats {
    pub fn from_samples(name: impl Into<String>, mut samples_ns: Vec<u64>) -> Self {
        samples_ns.sort_unstable();
        let n = samples_ns.len().max(1);
        let median_ns = samples_ns[n / 2];
        let p95_index = ((n as f64 - 1.0) * 0.95).round() as usize;
        let p95_ns = samples_ns[p95_index.min(n - 1)];
        let min_ns = *samples_ns.first().unwrap_or(&0);
        let max_ns = *samples_ns.last().unwrap_or(&0);
        let sum: u128 = samples_ns.iter().map(|v| *v as u128).sum();
        let mean_ns = (sum / n as u128) as u64;
        Self {
            name: name.into(),
            samples_ns,
            median_ns,
            p95_ns,
            min_ns,
            max_ns,
            mean_ns,
        }
    }

    pub fn median_ms(&self) -> f64 {
        self.median_ns as f64 / 1_000_000.0
    }

    pub fn p95_ms(&self) -> f64 {
        self.p95_ns as f64 / 1_000_000.0
    }

    pub fn report_line(&self) -> String {
        format!(
            "  {:<28} median={:>8.3}ms  p95={:>8.3}ms  min={:>8.3}ms  max={:>8.3}ms  n={}",
            self.name,
            self.median_ms(),
            self.p95_ms(),
            self.min_ns as f64 / 1_000_000.0,
            self.max_ns as f64 / 1_000_000.0,
            self.samples_ns.len()
        )
    }
}

/// Run `body` for warmup + samples iterations; returns timing stats for the timed runs.
pub fn time_iters<F>(name: &str, warmup: usize, samples: usize, mut body: F) -> SampleStats
where
    F: FnMut(),
{
    for _ in 0..warmup {
        body();
    }
    let mut samples_ns = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        body();
        samples_ns.push(duration_ns(start.elapsed()));
    }
    SampleStats::from_samples(name, samples_ns)
}

/// Time a single closure once (for heavy one-shot benches).
pub fn time_once<F, T>(mut body: F) -> (T, Duration)
where
    F: FnMut() -> T,
{
    let start = Instant::now();
    let value = body();
    (value, start.elapsed())
}

pub fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_median_and_p95() {
        let stats = SampleStats::from_samples("t", vec![10, 20, 30, 40, 50]);
        assert_eq!(stats.median_ns, 30);
        assert_eq!(stats.min_ns, 10);
        assert_eq!(stats.max_ns, 50);
        assert!(stats.p95_ns >= stats.median_ns);
    }
}
