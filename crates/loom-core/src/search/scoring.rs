use crate::{models::CouplingScore, LoomConfig};

const DEFAULT_RELATIONSHIP_WEIGHT: f64 = 0.5;

#[must_use]
pub fn compute_structural(relationship: &str, confidence: f64, depth: usize) -> f64 {
    let weight = match relationship {
        "calls" | "extends" => 1.0,
        "called_by" | "extended_by" => 0.9,
        "instantiates" => 0.85,
        "imports" => 0.5,
        "imported_by" => 0.4,
        "co_located" => 0.2,
        _ => DEFAULT_RELATIONSHIP_WEIGHT,
    };
    let depth_decay = 1.0 / 2.0_f64.powi(depth.saturating_sub(1) as i32);
    (weight * confidence * depth_decay).clamp(0.0, 1.0)
}

#[must_use]
pub fn compute_semantic(distance: f64) -> f64 {
    (1.0 - distance).clamp(0.0, 1.0)
}

#[must_use]
pub fn compute_evolutionary(frequency: i64, max_frequency: i64) -> f64 {
    if max_frequency <= 0 {
        return 0.0;
    }
    ((frequency as f64) / (max_frequency as f64)).clamp(0.0, 1.0)
}

#[must_use]
pub fn fuse_signals(
    structural: f64,
    semantic: f64,
    evolutionary: f64,
    config: &LoomConfig,
) -> CouplingScore {
    let combined = if evolutionary.abs() < 1e-9 {
        let base = config.structural_weight + config.semantic_weight;
        if base <= 0.0 {
            0.0
        } else {
            let structural_weight = config.structural_weight / base;
            let semantic_weight = config.semantic_weight / base;
            structural * structural_weight + semantic * semantic_weight
        }
    } else {
        structural * config.structural_weight
            + semantic * config.semantic_weight
            + evolutionary * config.evolutionary_weight
    };
    CouplingScore {
        structural,
        semantic,
        evolutionary,
        combined: combined.clamp(0.0, 1.0),
    }
}
