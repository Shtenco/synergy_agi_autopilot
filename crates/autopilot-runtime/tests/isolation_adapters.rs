use std::path::PathBuf;
use autopilot_runtime::{CommandSpec, DockerRuntime, ExecutionProfile, RuntimeAdapter, RuntimeError, WslRuntime};

#[test]
fn docker_read_only_profile_builds_read_only_networkless_container() {
    let runtime = DockerRuntime::new("rust:1.90", PathBuf::from("C:/repo"));
    let command = runtime.build_command(
        &CommandSpec::new("cargo")
            .args(["test", "--workspace"])
            .profile(ExecutionProfile::ReadOnlyWorkspace)
            .allow_env("CARGO_HOME")
    ).unwrap();
    assert_eq!(command.program, "docker");
    assert!(command.args.iter().any(|item| item.contains("readonly=true")));
    assert!(command.args.windows(2).any(|pair| pair == ["--network", "none"]));
    assert!(command.args.windows(2).any(|pair| pair == ["--env", "CARGO_HOME"]));
    assert!(command.args.contains(&"/tmp:rw,noexec,nosuid,size=268435456".to_string()));
}

#[tokio::test]
async fn wsl_fails_closed_for_hard_read_only_profile() {
    let runtime = WslRuntime::new(None);
    let result = runtime.execute(
        CommandSpec::new("cargo").arg("test").profile(ExecutionProfile::ReadOnlyWorkspace)
    ).await;
    assert!(matches!(result, Err(RuntimeError::UnsupportedIsolation { .. })));
}
