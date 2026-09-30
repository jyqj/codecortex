//! Deterministic sampling helpers. Repetitions are not independent quality cases.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Distribution {
    pub samples: usize,
    pub p50_us: Option<u64>,
    pub p95_us: Option<u64>,
    pub max_us: Option<u64>,
    pub tail_claim: &'static str,
}
pub fn distribution(values: &[u64]) -> Distribution {
    let mut v = values.to_vec();
    v.sort_unstable();
    let at = |p: f64| {
        if v.is_empty() {
            None
        } else {
            Some(
                v[((p * v.len() as f64).ceil() as usize)
                    .saturating_sub(1)
                    .min(v.len() - 1)],
            )
        }
    };
    Distribution {
        samples: v.len(),
        p50_us: at(0.5),
        p95_us: at(0.95),
        max_us: v.last().copied(),
        tail_claim: if v.len() < 200 {
            "insufficient_for_tail_claim"
        } else {
            "empirical_distribution_only"
        },
    }
}
pub fn next(seed: &mut u64) -> u64 {
    if *seed == 0 {
        *seed = 0x9e3779b97f4a7c15;
    }
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}
pub fn order(n: usize, seed: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let mut s = seed;
    for i in (1..n).rev() {
        let j = (next(&mut s) as usize) % (i + 1);
        v.swap(i, j);
    }
    v
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interval {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
    pub independent_units: usize,
}
pub fn bootstrap(values: &[f64], seed: u64) -> Option<Interval> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let mut s = seed;
    let mut draws = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let mut total = 0.0;
        for _ in values {
            total += values[(next(&mut s) as usize) % values.len()];
        }
        draws.push(total / values.len() as f64);
    }
    draws.sort_by(f64::total_cmp);
    Some(Interval {
        mean,
        low: draws[49],
        high: draws[1949],
        independent_units: values.len(),
    })
}
