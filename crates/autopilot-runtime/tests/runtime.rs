use autopilot_runtime::{CommandResult, CommandSpec, FakeRuntime, NativeRuntime, RuntimeAdapter};

#[tokio::test]
async fn fake_runtime_returns_configured_result() {
    let expected = CommandResult { exit_code: 7, stdout: "out".into(), stderr: "err".into(), duration_ms: 3 };
    let runtime = FakeRuntime::new(expected.clone());
    let actual = runtime.execute(CommandSpec::new("ignored")).await.unwrap();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn native_runtime_executes_without_shell_concatenation() {
    let runtime = NativeRuntime::default();
    let spec = if cfg!(windows) {
        CommandSpec::new("cmd").args(["/C", "echo", "OPEN_AUTOPILOT"])
    } else {
        CommandSpec::new("printf").arg("OPEN_AUTOPILOT")
    };
    let result = runtime.execute(spec).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("OPEN_AUTOPILOT"));
}
