use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};
use autopilot_workspace::{normalize_branch_fragment, WorkspaceError, WorkspaceManager};

#[derive(Default)]
struct RecordingRuntime {
    commands: Mutex<Vec<CommandSpec>>,
    results: Mutex<VecDeque<CommandResult>>,
}
impl RecordingRuntime {
    fn with_results(results: Vec<CommandResult>) -> Self { Self { commands: Mutex::new(vec![]), results: Mutex::new(results.into()) } }
}
#[async_trait]
impl RuntimeAdapter for RecordingRuntime {
    async fn probe(&self) -> bool { true }
    async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        self.commands.lock().unwrap().push(spec);
        Ok(self.results.lock().unwrap().pop_front().unwrap_or(CommandResult { exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0 }))
    }
    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}

#[test]
fn normalizes_branch_names_without_path_escape() {
    assert_eq!(normalize_branch_fragment("Fix Checkout / Crash"), "fix-checkout-crash");
    assert_eq!(normalize_branch_fragment("../../main"), "main");
}

#[tokio::test]
async fn clean_probe_uses_git_porcelain() {
    let runtime = Arc::new(RecordingRuntime::with_results(vec![CommandResult { exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 1 }]));
    let manager = WorkspaceManager::new(PathBuf::from("C:/repo"), PathBuf::from("C:/worktrees"), runtime.clone());
    assert!(manager.is_clean().await.unwrap());
    let commands = runtime.commands.lock().unwrap();
    assert_eq!(commands[0].program, "git");
    assert_eq!(commands[0].args, vec!["-C", "C:/repo", "status", "--porcelain"]);
}

#[tokio::test]
async fn creates_worktree_with_sanitized_branch_and_explicit_base() {
    let runtime = Arc::new(RecordingRuntime::default());
    let manager = WorkspaceManager::new(PathBuf::from("C:/repo"), PathBuf::from("C:/worktrees"), runtime.clone());
    let created = manager.create_worktree("Fix Crash", "main").await.unwrap();
    assert_eq!(created.branch, "autopilot/fix-crash");
    assert!(created.path.ends_with("fix-crash"));
    let commands = runtime.commands.lock().unwrap();
    assert_eq!(commands[0].args[0..5], ["-C", "C:/repo", "worktree", "add", "-b"]);
    assert_eq!(commands[0].args[5], "autopilot/fix-crash");
}

#[tokio::test]
async fn rejects_removal_outside_managed_worktree_root() {
    let runtime = Arc::new(RecordingRuntime::default());
    let manager = WorkspaceManager::new(PathBuf::from("C:/repo"), PathBuf::from("C:/worktrees"), runtime.clone());
    let error = manager.remove_worktree(PathBuf::from("C:/important-project")).await.unwrap_err();
    assert!(matches!(error, WorkspaceError::PathOutsideManagedRoot { .. }));
    assert!(runtime.commands.lock().unwrap().is_empty());
}

#[tokio::test]
async fn allows_removal_inside_managed_worktree_root() {
    let runtime = Arc::new(RecordingRuntime::default());
    let manager = WorkspaceManager::new(PathBuf::from("C:/repo"), PathBuf::from("C:/worktrees"), runtime.clone());
    manager.remove_worktree(PathBuf::from("C:/worktrees/fix-crash")).await.unwrap();
    let commands = runtime.commands.lock().unwrap();
    assert_eq!(commands[0].args, vec!["-C", "C:/repo", "worktree", "remove", "C:/worktrees/fix-crash"]);
}
