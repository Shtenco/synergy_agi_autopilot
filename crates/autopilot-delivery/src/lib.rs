use std::path::PathBuf;
use std::sync::Arc;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};
use serde::Deserialize;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct DeliveryConfig {
    pub workdir: PathBuf,
    pub remote: String,
    pub branch: String,
    pub base: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequestRef { pub url: String }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CiState { Passing, Failing, Pending, Unknown }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiCheck {
    pub name: String,
    pub state: String,
    pub bucket: String,
    pub link: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiReport { pub state: CiState, pub checks: Vec<CiCheck> }

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("unsafe delivery branch: {0}")]
    UnsafeBranch(String),
    #[error("git/gh command failed: {program} exit={code}: {stderr}")]
    CommandFailed { program: String, code: i32, stderr: String },
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

#[derive(Deserialize)]
struct GhCheck {
    #[serde(default)] name: String,
    #[serde(default)] state: String,
    #[serde(default)] bucket: String,
    #[serde(default)] link: String,
}

pub struct DeliveryEngine {
    runtime: Arc<dyn RuntimeAdapter>,
    config: DeliveryConfig,
}

impl DeliveryEngine {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, config: DeliveryConfig) -> Result<Self, DeliveryError> {
        validate_branch(&config.branch)?;
        Ok(Self { runtime, config })
    }

    async fn execute(&self, program: &str, args: Vec<String>) -> Result<CommandResult, DeliveryError> {
        Ok(self.runtime.execute(CommandSpec::new(program).args(args).cwd(self.config.workdir.clone())).await?)
    }

    async fn run(&self, program: &str, args: Vec<String>) -> Result<String, DeliveryError> {
        let result = self.execute(program, args).await?;
        if result.exit_code != 0 {
            return Err(DeliveryError::CommandFailed { program: program.into(), code: result.exit_code, stderr: result.stderr });
        }
        Ok(result.stdout)
    }

    pub async fn prepare_commit(&self, message: &str) -> Result<(), DeliveryError> {
        self.run("git", vec!["add".into(), "--all".into()]).await?;
        self.run("git", vec!["commit".into(), "-m".into(), message.into()]).await?;
        Ok(())
    }

    pub async fn push_branch(&self) -> Result<(), DeliveryError> {
        self.run("git", vec!["push".into(), self.config.remote.clone(), self.config.branch.clone()]).await?;
        Ok(())
    }

    pub async fn ensure_pr(&self) -> Result<PullRequestRef, DeliveryError> {
        let existing = self.execute("gh", vec![
            "pr".into(), "view".into(), self.config.branch.clone(),
            "--json".into(), "url".into(), "--jq".into(), ".url".into(),
        ]).await?;
        if existing.exit_code == 0 && !existing.stdout.trim().is_empty() {
            return Ok(PullRequestRef { url: existing.stdout.trim().to_string() });
        }
        let stdout = self.run("gh", vec![
            "pr".into(), "create".into(),
            "--base".into(), self.config.base.clone(),
            "--head".into(), self.config.branch.clone(),
            "--title".into(), self.config.title.clone(),
            "--body".into(), self.config.body.clone(),
        ]).await?;
        Ok(PullRequestRef { url: stdout.trim().to_string() })
    }

    pub async fn publish_verified_change(&self, message: &str) -> Result<PullRequestRef, DeliveryError> {
        self.prepare_commit(message).await?;
        self.push_branch().await?;
        self.ensure_pr().await
    }

    pub async fn read_ci(&self) -> Result<CiReport, DeliveryError> {
        let result = self.execute("gh", vec![
            "pr".into(), "checks".into(),
            "--json".into(), "name,state,bucket,link".into(),
        ]).await?;
        let parsed: Result<Vec<GhCheck>, _> = serde_json::from_str(&result.stdout);
        let Ok(parsed) = parsed else {
            return Ok(CiReport { state: CiState::Unknown, checks: Vec::new() });
        };
        let checks: Vec<CiCheck> = parsed.into_iter().map(|item| CiCheck {
            name: item.name,
            state: item.state,
            bucket: item.bucket,
            link: item.link,
        }).collect();
        let state = classify_ci(&checks);
        Ok(CiReport { state, checks })
    }
}

fn validate_branch(branch: &str) -> Result<(), DeliveryError> {
    let valid_prefix = branch.starts_with("autopilot/");
    let suffix = branch.strip_prefix("autopilot/").unwrap_or_default();
    let bad = branch == "main"
        || branch == "master"
        || !valid_prefix
        || suffix.is_empty()
        || branch.contains("..")
        || branch.contains('\\')
        || branch.ends_with('/')
        || branch.starts_with('/');
    if bad { return Err(DeliveryError::UnsafeBranch(branch.into())); }
    Ok(())
}

fn classify_ci(checks: &[CiCheck]) -> CiState {
    if checks.is_empty() { return CiState::Unknown; }
    let mut pending = false;
    for check in checks {
        let bucket = check.bucket.to_ascii_lowercase();
        let state = check.state.to_ascii_lowercase();
        if bucket == "fail" || state.contains("fail") || state.contains("error") || state.contains("cancel") {
            return CiState::Failing;
        }
        if bucket == "pending" || state.contains("pending") || state.contains("queued") || state.contains("progress") {
            pending = true;
        }
    }
    if pending {
        CiState::Pending
    } else if checks.iter().all(|check| {
        let bucket = check.bucket.to_ascii_lowercase();
        let state = check.state.to_ascii_lowercase();
        bucket == "pass" || state == "success" || state == "completed"
    }) {
        CiState::Passing
    } else {
        CiState::Unknown
    }
}
