use std::cmp::Ordering;
use std::collections::BTreeMap;
use autopilot_domain::AgentRole;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutingProfile { BestQuality, Balanced, Budget }

#[derive(Clone, Debug, PartialEq)]
pub struct ModelCandidate {
    pub provider: String,
    pub model: String,
    pub quality: u8,
    pub cost_per_million: f64,
    pub latency_ms: u64,
    pub healthy: bool,
    pub role_affinities: BTreeMap<AgentRole, u8>,
}

pub struct ModelRouter;

impl ModelRouter {
    fn score(role: &AgentRole, profile: RoutingProfile, item: &ModelCandidate) -> f64 {
        let quality = f64::from(item.quality.min(100)) / 100.0;
        let affinity = f64::from(*item.role_affinities.get(role).unwrap_or(&50)) / 100.0;
        let cost = 1.0 / (1.0 + item.cost_per_million.max(0.0));
        let latency = 1.0 / (1.0 + item.latency_ms as f64 / 1000.0);
        match profile {
            RoutingProfile::BestQuality => quality * 0.72 + affinity * 0.23 + latency * 0.04 + cost * 0.01,
            RoutingProfile::Balanced => quality * 0.45 + affinity * 0.20 + latency * 0.15 + cost * 0.20,
            RoutingProfile::Budget => quality * 0.20 + affinity * 0.15 + latency * 0.10 + cost * 0.55,
        }
    }

    pub fn rank(role: &AgentRole, profile: RoutingProfile, candidates: &[ModelCandidate]) -> Vec<ModelCandidate> {
        let mut ranked: Vec<_> = candidates.iter().filter(|item| item.healthy).cloned().collect();
        ranked.sort_by(|left, right| {
            Self::score(role, profile, right).partial_cmp(&Self::score(role, profile, left)).unwrap_or(Ordering::Equal)
                .then_with(|| left.provider.cmp(&right.provider))
                .then_with(|| left.model.cmp(&right.model))
        });
        ranked
    }

    pub fn select(role: &AgentRole, profile: RoutingProfile, candidates: &[ModelCandidate]) -> Option<ModelCandidate> {
        Self::rank(role, profile, candidates).into_iter().next()
    }
}
