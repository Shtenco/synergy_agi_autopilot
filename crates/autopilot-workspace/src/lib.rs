use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub branch: String,
    pub path: PathBuf,
}

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("git {operation} failed with exit code {code}: {stderr}")]
    GitFailed { operation: String, code: i32, stderr: String },
    #[error("branch fragment is empty after normalization")]
    EmptyBranch,
    #[error("path is outside managed worktree root: {path}")]
    PathOutsideManagedRoot { path: String },
}

pub fn normalize_branch_fragment(input: &str) -> String {
    let mut output = String::new();
    let mut separator_pending = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if separator_pending && !output.is_empty() {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
            separator_pending = false;
        } else if !output.is_empty() {
            separator_pending = true;
        }
    }
    output
}

fn lexical_normalize(path: &Path) -> Option<PathBuf> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            other => result.push(other.as_os_str()),
        }
    }
    Some(result)
}

pub struct WorkspaceManager {
    repo_root: PathBuf,
    worktree_root: PathBuf,
    runtime: Arc<dyn RuntimeAdapter>,
}

impl WorkspaceManager {
    pub fn new(repo_root: PathBuf, worktree_root: PathBuf, runtime: Arc<dyn RuntimeAdapter>) -> Self {
        Self { repo_root, worktree_root, runtime }
    }

    fn repo_string(&self) -> String {
        self.repo_root.to_string_lossy().into_owned()
    }

    fn ensure_managed_path(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let normalized_root = lexical_normalize(&self.worktree_root).ok_or_else(|| WorkspaceError::PathOutsideManagedRoot {
            path: self.worktree_root.to_string_lossy().into_owned(),
        })?;
        let normalized_path = lexical_normalize(path).ok_or_else(|| WorkspaceError::PathOutsideManagedRoot {
            path: path.to_string_lossy().into_owned(),
        })?;

        if normalized_path == normalized_root || !normalized_path.starts_with(&normalized_root) {
            return Err(WorkspaceError::PathOutsideManagedRoot {
                path: path.to_string_lossy().into_owned(),
            });
        }
        Ok(normalized_path)
    }

    async fn run_git(&self, operation: &str, args: Vec<String>) -> Result<CommandResult, WorkspaceError> {
        let mut full = vec!["-C".to_string(), self.repo_string()];
        full.extend(args);
        let result = self.runtime.execute(CommandSpec::new("git").args(full)).await?;
        if result.exit_code != 0 {
            return Err(WorkspaceError::GitFailed {
                operation: operation.into(),
                code: result.exit_code,
                stderr: result.stderr,
            });
        }
        Ok(result)
    }

    pub async fn is_clean(&self) -> Result<bool, WorkspaceError> {
        let result = self.run_git("status", vec!["status".into(), "--porcelain".into()]).await?;
        Ok(result.stdout.trim().is_empty())
    }

    pub async fn current_branch(&self) -> Result<String, WorkspaceError> {
        let result = self.run_git("branch", vec!["branch".into(), "--show-current".into()]).await?;
        Ok(result.stdout.trim().to_string())
    }

    pub async fn create_worktree(&self, label: &str, base_ref: &str) -> Result<WorktreeInfo, WorkspaceError> {
        let slug = normalize_branch_fragment(label);
        if slug.is_empty() {
            return Err(WorkspaceError::EmptyBranch);
        }
        let branch = format!("autopilot/{slug}");
        let path = self.ensure_managed_path(&self.worktree_root.join(&slug))?;
        let args = vec![
            "worktree".into(),
            "add".into(),
            "-b".into(),
            branch.clone(),
            path.to_string_lossy().into_owned(),
            base_ref.into(),
        ];
        self.run_git("worktree add", args).await?;
        Ok(WorktreeInfo { branch, path })
    }

    pub async fn remove_worktree(&self, path: PathBuf) -> Result<(), WorkspaceError> {
        let path = self.ensure_managed_path(&path)?;
        self.run_git(
            "worktree remove",
            vec!["worktree".into(), "remove".into(), path.to_string_lossy().into_owned()],
        )
        .await?;
        Ok(())
    }

    pub async fn diff(&self) -> Result<String, WorkspaceError> {
        let result = self.run_git("diff", vec!["diff".into(), "--no-ext-diff".into()]).await?;
        Ok(result.stdout)
    }
}
