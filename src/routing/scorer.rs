use crate::config::WeightConfig;
use crate::model::score::RouteScore;

pub fn weighted_score(score: RouteScore, weights: &WeightConfig) -> f32 {
    (score.activity * weights.activity)
        + (score.distance * weights.distance)
        + (score.security * weights.security)
}
