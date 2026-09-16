use autopilot_domain::{Capability, RiskClass};
use autopilot_policy::{PolicyDecision, PolicyEngine, PolicyRule};

#[test]
fn explicit_empty_policy_allows_when_no_rule_matches() {
    let engine = PolicyEngine::new(vec![]);
    assert_eq!(engine.evaluate(Capability::ReadWorkspace, RiskClass::Low), PolicyDecision::Allow);
}

#[test]
fn high_risk_can_require_approval() {
    let engine = PolicyEngine::new(vec![PolicyRule::at_least_risk(RiskClass::High, PolicyDecision::RequireApproval)]);
    assert_eq!(engine.evaluate(Capability::ExecuteProcess, RiskClass::High), PolicyDecision::RequireApproval);
}

#[test]
fn capability_rule_can_deny() {
    let engine = PolicyEngine::new(vec![PolicyRule::for_capability(Capability::ForcePush, PolicyDecision::Deny)]);
    assert_eq!(engine.evaluate(Capability::ForcePush, RiskClass::Medium), PolicyDecision::Deny);
}

#[test]
fn deny_has_precedence_over_approval() {
    let engine = PolicyEngine::new(vec![
        PolicyRule::at_least_risk(RiskClass::Medium, PolicyDecision::RequireApproval),
        PolicyRule::for_capability(Capability::ReleasePublish, PolicyDecision::Deny),
    ]);
    assert_eq!(engine.evaluate(Capability::ReleasePublish, RiskClass::Critical), PolicyDecision::Deny);
}

#[test]
fn secure_defaults_block_force_push_and_release_publish() {
    let engine = PolicyEngine::default();
    assert_eq!(engine.evaluate(Capability::ForcePush, RiskClass::Medium), PolicyDecision::Deny);
    assert_eq!(engine.evaluate(Capability::ReleasePublish, RiskClass::High), PolicyDecision::Deny);
}

#[test]
fn secure_defaults_require_boundary_for_secrets_and_high_risk_delete() {
    let engine = PolicyEngine::default();
    assert_eq!(engine.evaluate(Capability::AccessSecrets, RiskClass::High), PolicyDecision::RequireApproval);
    assert_eq!(engine.evaluate(Capability::DeleteFiles, RiskClass::High), PolicyDecision::RequireApproval);
    assert_eq!(engine.evaluate(Capability::WriteWorkspace, RiskClass::Medium), PolicyDecision::Allow);
}
