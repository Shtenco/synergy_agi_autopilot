use std::collections::BTreeMap;
use autopilot_domain::AgentRole;
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};

fn candidate(provider: &str, model: &str, quality: u8, cost: f64, latency: u64, healthy: bool, affinity: u8) -> ModelCandidate {
    let mut affinities = BTreeMap::new();
    affinities.insert(AgentRole::Coder, affinity);
    ModelCandidate { provider: provider.into(), model: model.into(), quality, cost_per_million: cost, latency_ms: latency, healthy, role_affinities: affinities }
}

#[test]
fn excludes_unhealthy_models() {
    let items = vec![candidate("a", "great-but-down", 100, 1.0, 10, false, 100), candidate("b", "healthy", 70, 2.0, 100, true, 70)];
    assert_eq!(ModelRouter::select(&AgentRole::Coder, RoutingProfile::BestQuality, &items).unwrap().model, "healthy");
}

#[test]
fn best_quality_prefers_quality_and_affinity() {
    let items = vec![candidate("a", "cheap", 60, 0.1, 40, true, 60), candidate("b", "strong", 95, 8.0, 300, true, 95)];
    assert_eq!(ModelRouter::select(&AgentRole::Coder, RoutingProfile::BestQuality, &items).unwrap().model, "strong");
}

#[test]
fn budget_profile_prefers_materially_lower_cost() {
    let items = vec![candidate("a", "cheap", 75, 0.2, 150, true, 70), candidate("b", "expensive", 90, 20.0, 100, true, 90)];
    assert_eq!(ModelRouter::select(&AgentRole::Coder, RoutingProfile::Budget, &items).unwrap().model, "cheap");
}

#[test]
fn ranking_is_stable_for_equal_scores() {
    let items = vec![candidate("z", "same", 80, 1.0, 100, true, 80), candidate("a", "same", 80, 1.0, 100, true, 80)];
    let ranked = ModelRouter::rank(&AgentRole::Coder, RoutingProfile::Balanced, &items);
    assert_eq!(ranked[0].provider, "a");
}
