use std::collections::VecDeque;
use std::sync::Arc;
use async_trait::async_trait;
use tokio::sync::Mutex;
use autopilot_audit::{AuditStore, InMemoryAuditStore};
use autopilot_core::{AgentExecutor, AgentOutcome, Director, DirectorConfig, GoalRequest, PlannedTask, Planner, VerificationOutcome, Verifier};
use autopilot_domain::{AgentRole, Capability, Evidence, GoalState, RiskClass};
use autopilot_policy::{PolicyDecision, PolicyEngine, PolicyRule};

struct OneTaskPlanner;
#[async_trait]
impl Planner for OneTaskPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        Ok(vec![PlannedTask { role: AgentRole::Coder, objective: request.objective.clone(), capability: Capability::WriteWorkspace, risk: RiskClass::Medium }])
    }
}

#[derive(Default)]
struct CountingExecutor { calls: Mutex<usize> }
#[async_trait]
impl AgentExecutor for CountingExecutor {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        *self.calls.lock().await += 1;
        Ok(AgentOutcome { objective: task.objective.clone(), success: true, summary: "done".into() })
    }
}

struct SequenceVerifier { sequence: Mutex<VecDeque<bool>> }
impl SequenceVerifier {
    fn new(values: impl IntoIterator<Item = bool>) -> Self { Self { sequence: Mutex::new(values.into_iter().collect()) } }
}
#[async_trait]
impl Verifier for SequenceVerifier {
    async fn verify(&self, _request: &GoalRequest, _outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String> {
        let passed = self.sequence.lock().await.pop_front().unwrap_or(false);
        Ok(VerificationOutcome { passed, evidence: vec![if passed { Evidence::passed("test", "green") } else { Evidence::failed("test", "red") }] })
    }
}

fn request() -> GoalRequest {
    GoalRequest::new("implement feature").require(Capability::WriteWorkspace, RiskClass::Medium)
}

#[tokio::test]
async fn completes_only_after_verification_passes() {
    let audit = Arc::new(InMemoryAuditStore::default());
    let director = Director::new(
        DirectorConfig { max_repairs: 2 }, PolicyEngine::default(), audit.clone(),
        Arc::new(OneTaskPlanner), Arc::new(CountingExecutor::default()), Arc::new(SequenceVerifier::new([true])),
    );
    let summary = director.run_goal(request()).await.unwrap();
    assert_eq!(summary.final_state, GoalState::Completed);
    assert_eq!(summary.repairs, 0);
    assert!(summary.evidence.iter().all(|e| e.passed));
    let events = audit.list_by_correlation(summary.correlation_id).await.unwrap();
    assert!(events.iter().any(|e| e.event_type == "Completed"));
}

#[tokio::test]
async fn repairs_after_failed_verification_then_completes() {
    let executor = Arc::new(CountingExecutor::default());
    let director = Director::new(
        DirectorConfig { max_repairs: 2 }, PolicyEngine::default(), Arc::new(InMemoryAuditStore::default()),
        Arc::new(OneTaskPlanner), executor.clone(), Arc::new(SequenceVerifier::new([false, true])),
    );
    let summary = director.run_goal(request()).await.unwrap();
    assert_eq!(summary.final_state, GoalState::Completed);
    assert_eq!(summary.repairs, 1);
    assert_eq!(*executor.calls.lock().await, 2);
}

#[tokio::test]
async fn fails_when_repair_budget_is_exhausted() {
    let director = Director::new(
        DirectorConfig { max_repairs: 1 }, PolicyEngine::default(), Arc::new(InMemoryAuditStore::default()),
        Arc::new(OneTaskPlanner), Arc::new(CountingExecutor::default()), Arc::new(SequenceVerifier::new([false, false])),
    );
    let summary = director.run_goal(request()).await.unwrap();
    assert_eq!(summary.final_state, GoalState::Failed);
    assert_eq!(summary.repairs, 1);
}

#[tokio::test]
async fn policy_denial_stops_before_execution() {
    let executor = Arc::new(CountingExecutor::default());
    let policy = PolicyEngine::new(vec![PolicyRule::for_capability(Capability::WriteWorkspace, PolicyDecision::Deny)]);
    let director = Director::new(
        DirectorConfig { max_repairs: 2 }, policy, Arc::new(InMemoryAuditStore::default()),
        Arc::new(OneTaskPlanner), executor.clone(), Arc::new(SequenceVerifier::new([true])),
    );
    let summary = director.run_goal(request()).await.unwrap();
    assert_eq!(summary.final_state, GoalState::Failed);
    assert!(summary.blocked_by_policy);
    assert_eq!(*executor.calls.lock().await, 0);
}
