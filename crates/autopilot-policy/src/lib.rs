use autopilot_domain::{Capability, RiskClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyDecision { Allow, RequireApproval, Deny }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyRule {
    pub capability: Option<Capability>,
    pub min_risk: Option<RiskClass>,
    pub decision: PolicyDecision,
}

impl PolicyRule {
    pub fn for_capability(capability: Capability, decision: PolicyDecision) -> Self {
        Self { capability: Some(capability), min_risk: None, decision }
    }

    pub fn for_capability_at_least_risk(capability: Capability, min_risk: RiskClass, decision: PolicyDecision) -> Self {
        Self { capability: Some(capability), min_risk: Some(min_risk), decision }
    }

    pub fn at_least_risk(min_risk: RiskClass, decision: PolicyDecision) -> Self {
        Self { capability: None, min_risk: Some(min_risk), decision }
    }

    fn matches(&self, capability: &Capability, risk: RiskClass) -> bool {
        let capability_matches = match &self.capability {
            Some(expected) => expected == capability,
            None => true,
        };
        let risk_matches = match self.min_risk {
            Some(minimum) => risk >= minimum,
            None => true,
        };
        capability_matches && risk_matches
    }
}

#[derive(Clone, Debug)]
pub struct PolicyEngine { rules: Vec<PolicyRule> }

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new(vec![
            PolicyRule::for_capability(Capability::ForcePush, PolicyDecision::Deny),
            PolicyRule::for_capability(Capability::ReleasePublish, PolicyDecision::Deny),
            PolicyRule::for_capability_at_least_risk(Capability::AccessSecrets, RiskClass::High, PolicyDecision::RequireApproval),
            PolicyRule::for_capability_at_least_risk(Capability::DeleteFiles, RiskClass::High, PolicyDecision::RequireApproval),
        ])
    }
}

impl PolicyEngine {
    pub fn new(rules: Vec<PolicyRule>) -> Self { Self { rules } }

    pub fn evaluate(&self, capability: Capability, risk: RiskClass) -> PolicyDecision {
        let mut result = PolicyDecision::Allow;
        for rule in self.rules.iter().filter(|rule| rule.matches(&capability, risk)) {
            match rule.decision {
                PolicyDecision::Deny => return PolicyDecision::Deny,
                PolicyDecision::RequireApproval => result = PolicyDecision::RequireApproval,
                PolicyDecision::Allow => {}
            }
        }
        result
    }
}
