use std::path::PathBuf;
use std::sync::Arc;
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_runtime::{CommandResult, ExecutionProfile, FakeRuntime};

#[test]
fn builtin_registry_exposes_expected_agents() {
    let registry = BackendRegistry::default();
    assert!(registry.resolve(&BackendKind::OpenCode).is_ok());
    assert!(registry.resolve(&BackendKind::Codex).is_ok());
    assert!(registry.resolve(&BackendKind::Claude).is_ok());
    assert!(registry.resolve(&BackendKind::Gemini).is_ok());
}

#[test]
fn codex_command_is_explicit_and_has_no_bypass_flag() {
    let registry = BackendRegistry::default();
    let spec = RunSpec::new(BackendKind::Codex, "review this", PathBuf::from("C:/repo"))
        .model("gpt-x")
        .profile(ExecutionProfile::ReadOnlyWorkspace);
    let command = registry.build_command(&spec).unwrap();
    assert_eq!(command.program, "codex");
    assert_eq!(command.args, vec!["exec", "--model", "gpt-x", "review this"]);
    assert_eq!(command.profile, ExecutionProfile::ReadOnlyWorkspace);
    assert!(!command.args.iter().any(|arg| arg.contains("bypass") || arg.contains("danger")));
}

#[test]
fn empty_model_uses_backend_default_without_model_flag() {
    let registry = BackendRegistry::default();
    let spec = RunSpec::new(BackendKind::Codex, "review this", PathBuf::from("C:/repo")).model("");
    let command = registry.build_command(&spec).unwrap();
    assert_eq!(command.args, vec!["exec", "review this"]);
}

#[test]
fn projects_only_explicit_extra_environment() {
    let registry = BackendRegistry::default();
    let spec = RunSpec::new(BackendKind::Codex, "task", PathBuf::from("C:/repo"))
        .allow_env("OPENAI_API_KEY")
        .timeout_ms(120_000);
    let command = registry.build_command(&spec).unwrap();
    assert!(command.env_allowlist.contains("OPENAI_API_KEY"));
    assert_eq!(command.timeout_ms, Some(120_000));
}

#[test]
fn unknown_custom_backend_is_rejected() {
    let registry = BackendRegistry::default();
    let spec = RunSpec::new(BackendKind::Custom("missing".into()), "x", PathBuf::from("."));
    assert!(registry.build_command(&spec).is_err());
}

#[tokio::test]
async fn non_zero_exit_is_returned_as_structured_termination() {
    let runtime = Arc::new(FakeRuntime::new(CommandResult { exit_code: 7, stdout: String::new(), stderr: "failed".into(), duration_ms: 1 }));
    let harness = AgentHarness::new(runtime, BackendRegistry::default());
    let result = harness.run(RunSpec::new(BackendKind::OpenCode, "task", PathBuf::from("."))).await.unwrap();
    assert_eq!(result.termination, Termination::NonZero(7));
    assert_eq!(result.stderr, "failed");
}
