use crate::config::RoutingScoreConfig;

pub struct RoutingParamsBounds {
    pub baseline_min: f64,
    pub baseline_max: f64,
    pub low_threshold_min: f64,
    pub low_threshold_max: f64,
    pub promote_min_chain_min: f64,
    pub promote_min_chain_max: f64,
}

impl Default for RoutingParamsBounds {
    fn default() -> Self {
        Self {
            baseline_min: 0.2,
            baseline_max: 2.0,
            low_threshold_min: 0.1,
            low_threshold_max: 1.0,
            promote_min_chain_min: 0.0,
            promote_min_chain_max: 1.0,
        }
    }
}

pub fn clamp_routing_params(c: &RoutingScoreConfig, b: &RoutingParamsBounds) -> (f64, f64, f64) {
    let baseline = c.baseline_score.clamp(b.baseline_min, b.baseline_max);
    let low_threshold = c.low_score_threshold.clamp(b.low_threshold_min, b.low_threshold_max);
    let promote_min_chain = c.promote_min_chain_score.clamp(b.promote_min_chain_min, b.promote_min_chain_max);
    (baseline, low_threshold, promote_min_chain)
}

pub fn score_from_evolution_stats(
    matching_count: usize,
    total_output_len: usize,
    baseline: f64,
) -> f64 {
    if matching_count == 0 {
        return baseline;
    }
    let avg_len = total_output_len as f64 / matching_count as f64;
    let raw = matching_count as f64 + avg_len / 100.0;
    raw.max(baseline)
}

pub fn promote_eligible(score: f64, min_chain: f64) -> bool {
    min_chain <= 0.0 || score >= min_chain
}

pub fn adjusted_baseline(
    current: f64,
    success_rate: f64,
    bounds: (f64, f64),
    step: f64,
) -> f64 {
    let delta = step * (success_rate - 0.5);
    (current + delta).clamp(bounds.0, bounds.1)
}
