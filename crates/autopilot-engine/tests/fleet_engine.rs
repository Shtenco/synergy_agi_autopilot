use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use autopilot_audit::InMemoryAuditStore;
use autopilot_domain::{AgentRole, GoalState};
use autopilot_engine::{FleetEngine, FleetEngineConfig, VerificationCommand};
use autopilot_harness::BackendKind;
use autopilot_router::{ModelCandidate, RoutingProfile};
use autopilot_runtime::{CommandResult, CommandSpec, ExecutionProfile, RuntimeAdapter, RuntimeError};

#[derive(Default)]
struct RecordingRuntime {
    commands: Mutex<Vec<CommandSpec>>,
}

#[async_trait]
impl RuntimeAdapter for RecordingRuntime {
    async fn probe(&self) -> bool { true }

    async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        self.commands.lock().unwrap().push(spec.clone());
        let stdout = if spec.program == "git" { String::new() } else { "ok".into() };
        Ok(CommandResult { exit_code: 0, stdout, stderr: String::new(), duration_ms: 1 })
    }

    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}

fn candidate(provider: &str, model: &str, quality: u8, role: AgentRole) -> ModelCandidate {
    let mut affinities = BTreeMap::new();
    affinities.insert(role, 100);
    ModelCandidate {
        provider: provider.into(),
        model: model.into(),
        quality,
        cost_per_million: 1.0,
        latency_ms: 100,
        healthy: true,
        role_affinities: affinities,
    }
}

#[tokio::test]
async fn fleet_engine_routes_roles_and_completes_cross_review_dag() {
    let writer_runtime = Arc::new(RecordingRuntime::default());
    let review_runtime = Arc::new(RecordingRuntime::default());
    let audit = Arc::new(InMemoryAuditStore::default());
    let config = FleetEngineConfig {
        repo_root: PathBuf::from("C:/repo"),
        worktree_root: PathBuf::from("C:/repo/.open-autopilot/worktrees"),
        base_ref: "main".into(),
        routing_profile: RoutingProfile::BestQuality,
        candidates: vec![
            candidate("opencode", "architect-model", 95, AgentRole::Architect),
            candidate("codex", "coder-model", 95, AgentRole::Coder),
            candidate("gemini", "tester-model", 95, AgentRole::Tester),
            candidate("claude", "review-model", 95, AgentRole::Reviewer),
            candidate("claude", "security-model", 95, AgentRole::Security),
            candidate("codex", "debug-model", 95, AgentRole::Debugger),
        ],
        allowed_env: vec![],
        timeout_ms: Some(30_000),
        verification: vec![VerificationCommand::new("tests", "cargo", ["test", "--workspace"])],
        max_repairs: 1,
    };

    let engine = FleetEngine::with_role_runtimes(writer_runtime.clone(), review_runtime.clone(), audit, config);
    let result = engine.run("implement feature safely").await.unwrap();

    assert_eq!(result.final_state, GoalState::Completed);
    assert!(result.role_runs.iter().any(|run| run.role == AgentRole::Architect && run.model == "architect-model"));
    assert!(result.role_runs.iter().any(|run| run.role == AgentRole::Coder && run.model == "coder-model"));
    assert!(result.role_runs.iter().any(|run| run.role == AgentRole::Tester));
    assert!(result.role_runs.iter().any(|run| run.role == AgentRole::Reviewer));
    assert!(result.role_runs.iter().any(|run| run.role == AgentRole::Security));

    let writer_commands = writer_runtime.commands.lock().unwrap();
    assert!(writer_commands.iter().any(|command| command.program == "git" && command.args.iter().any(|arg| arg == "worktree")));
    assert!(writer_commands.iter().any(|command| command.program == "codex" && command.args.iter().any(|arg| arg == "coder-model") && command.profile == ExecutionProfile::WorkspaceWrite));

    let review_commands = review_runtime.commands.lock().unwrap();
    assert!(review_commands.iter().any(|command| command.program == "opencode" && command.profile == ExecutionProfile::ReadOnlyWorkspace));
    assert!(review_commands.iter().filter(|command| command.profile == ExecutionProfile::ReadOnlyWorkspace).count() >= 4);
}

#[test]
fn provider_mapping_is_explicit_and_never_silent() {
    assert_eq!(autopilot_engine::backend_for_provider("opencode").unwrap(), BackendKind::OpenCode);
    assert_eq!(autopilot_engine::backend_for_provider("openai").unwrap(), BackendKind::Codex);
    assert_eq!(autopilot_engine::backend_for_provider("anthropic").unwrap(), BackendKind::Claude);
    assert_eq!(autopilot_engine::backend_for_provider("google").unwrap(), BackendKind::Gemini);
    assert!(autopilot_engine::backend_for_provider("unknown-provider").is_err());
}
