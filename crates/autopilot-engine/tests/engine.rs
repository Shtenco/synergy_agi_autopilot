use std::path::PathBuf;
use std::sync::Arc;
use async_trait::async_trait;
use autopilot_audit::InMemoryAuditStore;
use autopilot_domain::GoalState;
use autopilot_engine::{AutonomousEngine, EngineConfig, VerificationCommand};
use autopilot_harness::BackendKind;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};

#[derive(Default)]
struct EngineRuntime;

#[async_trait]
impl RuntimeAdapter for EngineRuntime {
    async fn probe(&self) -> bool { true }

    async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        let is_agent = matches!(spec.program.as_str(), "opencode" | "codex" | "claude" | "gemini");
        Ok(CommandResult {
            exit_code: 0,
            stdout: if is_agent { "agent finished".into() } else { "check passed".into() },
            stderr: String::new(),
            duration_ms: 1,
        })
    }

    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}

#[tokio::test]
async fn autonomous_engine_executes_agent_then_verification_and_completes() {
    let runtime: Arc<dyn RuntimeAdapter> = Arc::new(EngineRuntime);
    let audit = Arc::new(InMemoryAuditStore::default());
    let config = EngineConfig {
        backend: BackendKind::OpenCode,
        model: None,
        workdir: PathBuf::from("."),
        allowed_env: vec![],
        timeout_ms: Some(30_000),
        verification: vec![VerificationCommand::new("tests", "cargo", ["test", "--workspace"])],
        max_repairs: 1,
    };
    let engine = AutonomousEngine::new(runtime, audit, config);
    let result = engine.run("implement requested change").await.unwrap();
    assert_eq!(result.final_state, GoalState::Completed);
    assert_eq!(result.repairs, 0);
    assert!(result.evidence.iter().all(|e| e.passed));
}

#[tokio::test]
async fn autonomous_engine_enters_repair_loop_when_verification_fails() {
    struct RepairRuntime(std::sync::Mutex<usize>);
    #[async_trait]
    impl RuntimeAdapter for RepairRuntime {
        async fn probe(&self) -> bool { true }
        async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
            if matches!(spec.program.as_str(), "opencode" | "codex" | "claude" | "gemini") {
                return Ok(CommandResult { exit_code: 0, stdout: "agent finished".into(), stderr: String::new(), duration_ms: 1 });
            }
            let mut count = self.0.lock().unwrap();
            *count += 1;
            let exit = if *count == 1 { 1 } else { 0 };
            Ok(CommandResult { exit_code: exit, stdout: String::new(), stderr: if exit == 0 { String::new() } else { "failed once".into() }, duration_ms: 1 })
        }
        async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
    }

    let runtime: Arc<dyn RuntimeAdapter> = Arc::new(RepairRuntime(std::sync::Mutex::new(0)));
    let audit = Arc::new(InMemoryAuditStore::default());
    let config = EngineConfig {
        backend: BackendKind::Codex,
        model: None,
        workdir: PathBuf::from("."),
        allowed_env: vec![],
        timeout_ms: Some(30_000),
        verification: vec![VerificationCommand::new("tests", "cargo", ["test"])],
        max_repairs: 1,
    };
    let engine = AutonomousEngine::new(runtime, audit, config);
    let result = engine.run("repair until green").await.unwrap();
    assert_eq!(result.final_state, GoalState::Completed);
    assert_eq!(result.repairs, 1);
}
