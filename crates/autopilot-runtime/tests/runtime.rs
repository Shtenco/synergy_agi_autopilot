use autopilot_runtime::{CommandResult, CommandSpec, FakeRuntime, NativeRuntime, RuntimeAdapter};

#[test]
fn command_spec_requires_explicit_extra_environment_and_timeout() {
    let spec = CommandSpec::new("agent")
        .allow_env("OPENAI_API_KEY")
        .timeout_ms(30_000);
    assert!(spec.env_allowlist.contains("OPENAI_API_KEY"));
    assert_eq!(spec.timeout_ms, Some(30_000));
}

#[tokio::test]
async fn fake_runtime_returns_configured_result() {
    let expected = CommandResult { exit_code: 7, stdout: "out".into(), stderr: "err".into(), duration_ms: 3 };
    let runtime = FakeRuntime::new(expected.clone());
    let actual = runtime.execute(CommandSpec::new("ignored")).await.unwrap();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn native_runtime_executes_without_shell_concatenation() {
    let runtime = NativeRuntime;
    let spec = if cfg!(windows) {
        CommandSpec::new("cmd").args(["/C", "echo", "OPEN_AUTOPILOT"])
    } else {
        CommandSpec::new("printf").arg("OPEN_AUTOPILOT")
    };
    let result = runtime.execute(spec).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("OPEN_AUTOPILOT"));
}
