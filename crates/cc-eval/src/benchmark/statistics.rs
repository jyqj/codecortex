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

/// Canonical nearest-rank estimate for an already sorted sample, in its input unit.
/// Distribution, interval, benchmark and evaluation reports share this owner.
/// Empty samples stay absent here; legacy wire adapters retain their own zero defaults.
pub(crate) fn nearest_rank(sorted: &[u64], quantile: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    sorted
        .get(
            ((quantile * sorted.len() as f64).ceil() as usize)
                .saturating_sub(1)
                .min(sorted.len() - 1),
        )
        .copied()
}

pub fn distribution(values: &[u64]) -> Distribution {
    let mut v = values.to_vec();
    v.sort_unstable();
    Distribution {
        samples: v.len(),
        p50_us: nearest_rank(&v, 0.5),
        p95_us: nearest_rank(&v, 0.95),
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

/// A lifecycle label requires observations made by the runner. The suite name,
/// repetition number, query text and originating-work cost receipt are not cache
/// evidence. In particular, a reused query need not have hit the result cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementOperation {
    Build,
    Query,
    #[default]
    Unknown,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultCacheObservation {
    Hit,
    Miss,
    #[default]
    Unknown,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatencyEvidence {
    pub operation: MeasurementOperation,
    pub index_was_empty: Option<bool>,
    pub parse_cache_was_empty: Option<bool>,
    /// The measured request is the first query handled by this process.
    pub first_query_in_process: Option<bool>,
    /// This process opened a pre-existing index for the measured query without
    /// rebuilding it. An index created earlier in this process is not a reopen.
    pub reopened_existing_index: Option<bool>,
    pub warmup_completed: Option<bool>,
    pub result_cache: ResultCacheObservation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatencyStratum {
    ColdBuild,
    ProcessReopen,
    WarmUncached,
    CacheHit,
    Unknown,
}
impl LatencyEvidence {
    pub fn classify(&self) -> LatencyStratum {
        match self.operation {
            MeasurementOperation::Build
                if self.index_was_empty == Some(true)
                    && self.parse_cache_was_empty == Some(true)
                    && self.first_query_in_process.is_none()
                    && self.reopened_existing_index != Some(true)
                    && self.warmup_completed != Some(true)
                    && self.result_cache == ResultCacheObservation::Unknown =>
            {
                LatencyStratum::ColdBuild
            }
            MeasurementOperation::Query if self.index_was_empty == Some(false) => {
                match (
                    self.first_query_in_process,
                    self.warmup_completed,
                    self.result_cache,
                ) {
                    (
                        Some(true),
                        Some(false),
                        ResultCacheObservation::Unknown | ResultCacheObservation::Miss,
                    ) if self.reopened_existing_index == Some(true) => {
                        LatencyStratum::ProcessReopen
                    }
                    (Some(false), Some(true), ResultCacheObservation::Miss) => {
                        LatencyStratum::WarmUncached
                    }
                    (Some(false), Some(true), ResultCacheObservation::Hit) => {
                        LatencyStratum::CacheHit
                    }
                    _ => LatencyStratum::Unknown,
                }
            }
            _ => LatencyStratum::Unknown,
        }
    }
}

/// A nonparametric order-statistic interval for a population quantile. The
/// binomial coverage calculation assumes IID timings; serial correlation and
/// a mixed workload still require a controlled profile, even with a large N.
/// A null endpoint is unbounded, not zero and not the observed min/max.
#[derive(Debug, Clone, Serialize)]
pub struct QuantileInterval {
    pub quantile: f64,
    pub confidence: f64,
    pub samples: usize,
    pub estimate_us: u64,
    pub lower_us: Option<u64>,
    pub upper_us: Option<u64>,
    pub method: &'static str,
}
pub fn quantile_interval(values: &[u64], quantile: f64) -> Option<QuantileInterval> {
    if values.is_empty() || !(0.0..1.0).contains(&quantile) || quantile == 0.0 {
        return None;
    }
    let n = values.len();
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    // Work in log-space so p^N and (1-p)^N need not be representable. Summing
    // the normalized PMF also keeps a rounded final CDF from missing 0.975.
    let log_failure = (-quantile).ln_1p();
    let log_odds = quantile.ln() - log_failure;
    let mut log_probability = n as f64 * log_failure;
    let mut probabilities = Vec::with_capacity(n + 1);
    for k in 0..=n {
        probabilities.push(log_probability.exp());
        if k < n {
            log_probability += ((n - k) as f64).ln() - ((k + 1) as f64).ln() + log_odds;
        }
    }
    let total: f64 = probabilities.iter().sum();
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    let count_quantile = |p: f64| {
        let mut cumulative = 0.0;
        probabilities
            .iter()
            .position(|probability| {
                cumulative += probability / total;
                cumulative >= p
            })
            .unwrap_or(n)
    };
    let lower_rank = count_quantile(0.025);
    let upper_rank = count_quantile(0.975) + 1;
    Some(QuantileInterval {
        quantile,
        confidence: 0.95,
        samples: n,
        estimate_us: nearest_rank(&sorted, quantile)?,
        lower_us: lower_rank
            .checked_sub(1)
            .and_then(|i| sorted.get(i).copied()),
        upper_us: sorted.get(upper_rank - 1).copied(),
        method: "binomial_order_statistics_95pct_iid_assumption",
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LatencySample {
    pub evidence: LatencyEvidence,
    pub status: super::schema::ResultStatus,
    pub elapsed_us: Option<u64>,
}
#[derive(Debug, Clone, Serialize)]
pub struct LatencyLayer {
    pub stratum: LatencyStratum,
    pub samples: usize,
    pub completed: usize,
    pub partial: usize,
    pub errors: usize,
    pub timeouts: usize,
    pub cancelled: usize,
    pub missing_timings: usize,
    pub error_rate: Option<f64>,
    pub timeout_rate: Option<f64>,
    /// Includes errors, partials, cancellation and deadline-censored timeouts.
    /// This is the observed duration of attempts, not time to a valid answer.
    pub all_attempt_elapsed: Distribution,
    pub completed_elapsed: Distribution,
    pub p95_completed_ci: Option<QuantileInterval>,
    pub p99_completed_ci: Option<QuantileInterval>,
    pub statistical_status: &'static str,
}
#[derive(Debug, Clone, Serialize)]
pub struct LatencyLayers {
    pub schema_version: u32,
    pub expected_samples: usize,
    pub recorded_samples: usize,
    pub missing_samples: usize,
    pub unexpected_samples: usize,
    pub status: &'static str,
    pub timeout_semantics: &'static str,
    pub os_page_cache: &'static str,
    pub layers: Vec<LatencyLayer>,
}
/// Preserve every attempt and every status. The confidence intervals describe
/// completed attempts only and can never hide failure rates in a pass decision.
/// This is an observation artifact, not an independent replacement for V20.
pub fn latency_layers(samples: &[LatencySample], expected_samples: usize) -> LatencyLayers {
    use super::schema::ResultStatus;
    let layers = [
        LatencyStratum::ColdBuild,
        LatencyStratum::ProcessReopen,
        LatencyStratum::WarmUncached,
        LatencyStratum::CacheHit,
        LatencyStratum::Unknown,
    ]
    .into_iter()
    .map(|stratum| {
        let rows: Vec<_> = samples
            .iter()
            .filter(|s| s.evidence.classify() == stratum)
            .collect();
        let count = |status| rows.iter().filter(|s| s.status == status).count();
        let completed = count(ResultStatus::Success) + count(ResultStatus::NoMatch);
        let errors = count(ResultStatus::ToolError) + count(ResultStatus::ProtocolError);
        let timeouts = count(ResultStatus::Timeout);
        let all: Vec<_> = rows.iter().filter_map(|s| s.elapsed_us).collect();
        let complete_times: Vec<_> = rows
            .iter()
            .filter(|s| matches!(s.status, ResultStatus::Success | ResultStatus::NoMatch))
            .filter_map(|s| s.elapsed_us)
            .collect();
        let p95_completed_ci = quantile_interval(&complete_times, 0.95);
        let p99_completed_ci = quantile_interval(&complete_times, 0.99);
        let bounded = |ci: &Option<QuantileInterval>| {
            ci.as_ref()
                .is_some_and(|ci| ci.lower_us.is_some() && ci.upper_us.is_some())
        };
        let statistical_status = if rows.is_empty() {
            "unavailable"
        } else if stratum == LatencyStratum::Unknown {
            "inconclusive_unknown_lifecycle_or_cache"
        } else if all.len() != rows.len() || completed != rows.len() {
            "inconclusive_incomplete_or_failed_attempts"
        } else if rows.len() < 200 {
            "inconclusive_insufficient_tail_samples"
        } else if !bounded(&p95_completed_ci) || !bounded(&p99_completed_ci) {
            "inconclusive_unbounded_tail_interval"
        } else {
            "observations_only_requires_controlled_release_profile"
        };
        LatencyLayer {
            stratum,
            samples: rows.len(),
            completed,
            partial: count(ResultStatus::Partial),
            errors,
            timeouts,
            cancelled: count(ResultStatus::Cancelled),
            missing_timings: rows.len() - all.len(),
            error_rate: (!rows.is_empty()).then(|| errors as f64 / rows.len() as f64),
            timeout_rate: (!rows.is_empty()).then(|| timeouts as f64 / rows.len() as f64),
            all_attempt_elapsed: distribution(&all),
            completed_elapsed: distribution(&complete_times),
            p95_completed_ci,
            p99_completed_ci,
            statistical_status,
        }
    })
    .collect();
    LatencyLayers { schema_version: 1, expected_samples, recorded_samples: samples.len(),
        missing_samples: expected_samples.saturating_sub(samples.len()),
        unexpected_samples: samples.len().saturating_sub(expected_samples),
        status: "observations_only_not_a_performance_gate",
        timeout_semantics: "elapsed attempt durations retained; timeout is deadline-censored time to completion; failure denominators include every recorded attempt",
        os_page_cache: "unknown_not_cold_disk",
        layers }
}
