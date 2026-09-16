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

    pub fn at_least_risk(min_risk: RiskClass, decision: PolicyDecision) -> Self {
        Self { capability: None, min_risk: Some(min_risk), decision }
    }

    fn matches(&self, capability: &Capability, risk: RiskClass) -> bool {
        self.capability.as_ref().map_or(true, |expected| expected == capability)
            && self.min_risk.map_or(true, |minimum| risk >= minimum)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PolicyEngine { rules: Vec<PolicyRule> }

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
