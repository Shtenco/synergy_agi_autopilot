use std::sync::Arc;
use async_trait::async_trait;
use autopilot_audit::AuditStore;
use autopilot_domain::{AgentRole, AuditEvent, Capability, CorrelationId, Evidence, GoalState, RiskClass};
use autopilot_policy::{PolicyDecision, PolicyEngine};
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct GoalRequest {
    pub objective: String,
    pub required_capabilities: Vec<(Capability, RiskClass)>,
}

impl GoalRequest {
    pub fn new(objective: impl Into<String>) -> Self {
        Self { objective: objective.into(), required_capabilities: Vec::new() }
    }
    pub fn require(mut self, capability: Capability, risk: RiskClass) -> Self {
        self.required_capabilities.push((capability, risk));
        self
    }
}

#[derive(Clone, Debug)]
pub struct PlannedTask {
    pub role: AgentRole,
    pub objective: String,
    pub capability: Capability,
    pub risk: RiskClass,
}

#[derive(Clone, Debug)]
pub struct AgentOutcome {
    pub objective: String,
    pub success: bool,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct VerificationOutcome {
    pub passed: bool,
    pub evidence: Vec<Evidence>,
}

#[derive(Clone, Copy, Debug)]
pub struct DirectorConfig { pub max_repairs: usize }

impl Default for DirectorConfig {
    fn default() -> Self { Self { max_repairs: 2 } }
}

#[derive(Clone, Debug)]
pub struct GoalRunSummary {
    pub final_state: GoalState,
    pub repairs: usize,
    pub evidence: Vec<Evidence>,
    pub correlation_id: CorrelationId,
    pub blocked_by_policy: bool,
}

#[derive(Debug, Error)]
pub enum DirectorError {
    #[error("planner failed: {0}")] Planner(String),
    #[error("agent failed: {0}")] Agent(String),
    #[error("verifier failed: {0}")] Verifier(String),
    #[error("audit failed: {0}")] Audit(String),
    #[error("illegal state transition: {0:?} -> {1:?}")] IllegalTransition(GoalState, GoalState),
}

#[async_trait]
pub trait Planner: Send + Sync {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String>;
}

#[async_trait]
pub trait AgentExecutor: Send + Sync {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String>;
}

#[async_trait]
pub trait Verifier: Send + Sync {
    async fn verify(&self, request: &GoalRequest, outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String>;
}

pub struct Director {
    config: DirectorConfig,
    policy: PolicyEngine,
    audit: Arc<dyn AuditStore>,
    planner: Arc<dyn Planner>,
    executor: Arc<dyn AgentExecutor>,
    verifier: Arc<dyn Verifier>,
}

impl Director {
    pub fn new(
        config: DirectorConfig,
        policy: PolicyEngine,
        audit: Arc<dyn AuditStore>,
        planner: Arc<dyn Planner>,
        executor: Arc<dyn AgentExecutor>,
        verifier: Arc<dyn Verifier>,
    ) -> Self {
        Self { config, policy, audit, planner, executor, verifier }
    }

    async fn transition(
        &self,
        state: &mut GoalState,
        next: GoalState,
        correlation: CorrelationId,
    ) -> Result<(), DirectorError> {
        if !state.can_transition_to(&next) {
            return Err(DirectorError::IllegalTransition(*state, next));
        }
        *state = next;
        self.audit
            .append(AuditEvent::new(correlation, "director", format!("{:?}", next), RiskClass::Low, "entered"))
            .await
            .map_err(DirectorError::Audit)
    }

    pub async fn run_goal(&self, request: GoalRequest) -> Result<GoalRunSummary, DirectorError> {
        let correlation = CorrelationId::new();
        let mut state = GoalState::Created;
        let mut repairs = 0usize;
        let mut evidence = Vec::new();

        self.transition(&mut state, GoalState::Planning, correlation).await?;
        let tasks = self.planner.plan(&request).await.map_err(DirectorError::Planner)?;

        self.transition(&mut state, GoalState::PolicyCheck, correlation).await?;
        let mut blocked = false;
        for (capability, risk) in &request.required_capabilities {
            match self.policy.evaluate(capability.clone(), *risk) {
                PolicyDecision::Allow => {}
                PolicyDecision::RequireApproval | PolicyDecision::Deny => {
                    blocked = true;
                    break;
                }
            }
        }
        for task in &tasks {
            match self.policy.evaluate(task.capability.clone(), task.risk) {
                PolicyDecision::Allow => {}
                PolicyDecision::RequireApproval | PolicyDecision::Deny => {
                    blocked = true;
                    break;
                }
            }
        }
        if blocked {
            self.transition(&mut state, GoalState::Failed, correlation).await?;
            return Ok(GoalRunSummary { final_state: state, repairs, evidence, correlation_id: correlation, blocked_by_policy: true });
        }

        self.transition(&mut state, GoalState::Scheduling, correlation).await?;

        loop {
            self.transition(&mut state, GoalState::Executing, correlation).await?;
            let mut outcomes = Vec::with_capacity(tasks.len());
            for task in &tasks {
                let outcome = self.executor.execute(task).await.map_err(DirectorError::Agent)?;
                outcomes.push(outcome);
            }

            self.transition(&mut state, GoalState::Reviewing, correlation).await?;
            self.transition(&mut state, GoalState::Verifying, correlation).await?;
            let verification = self.verifier.verify(&request, &outcomes).await.map_err(DirectorError::Verifier)?;
            evidence.extend(verification.evidence);

            if verification.passed {
                self.transition(&mut state, GoalState::Publishing, correlation).await?;
                self.transition(&mut state, GoalState::Completed, correlation).await?;
                return Ok(GoalRunSummary { final_state: state, repairs, evidence, correlation_id: correlation, blocked_by_policy: false });
            }

            if repairs >= self.config.max_repairs {
                self.transition(&mut state, GoalState::Failed, correlation).await?;
                return Ok(GoalRunSummary { final_state: state, repairs, evidence, correlation_id: correlation, blocked_by_policy: false });
            }

            repairs += 1;
            self.transition(&mut state, GoalState::Repairing, correlation).await?;
        }
    }
}
