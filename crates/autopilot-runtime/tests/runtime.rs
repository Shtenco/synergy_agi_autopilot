use std::time::Duration;
use autopilot_runtime::{CommandResult, CommandSpec, FakeRuntime, NativeRuntime, RuntimeAdapter, RuntimeError};

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

#[tokio::test]
async fn native_runtime_can_cancel_an_active_process() {
    let runtime = NativeRuntime::default();
    let spec = if cfg!(windows) {
        CommandSpec::new("powershell").args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"])
    } else {
        CommandSpec::new("sleep").arg("10")
    };
    let running_runtime = runtime.clone();
    let task = tokio::spawn(async move { running_runtime.execute(spec).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    runtime.cancel().await.unwrap();
    let result = task.await.unwrap();
    assert!(matches!(result, Err(RuntimeError::Cancelled)));
}
