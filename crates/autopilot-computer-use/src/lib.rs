use std::sync::Arc;

use autopilot_runtime::{CommandSpec, RuntimeAdapter, RuntimeError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowsTarget {
    App(String),
    Hwnd(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputerUseAction {
    Inspect,
    Search { query: String },
    GetValue { selector: String },
    SetValue { selector: String, value: String },
    Invoke { selector: String },
    WaitFor { selector: String, timeout_ms: Option<u64> },
    Screenshot { output: Option<String> },
}

impl ComputerUseAction {
    pub fn is_write(&self) -> bool {
        matches!(self, Self::SetValue { .. } | Self::Invoke { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputerUseResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Error)]
pub enum ComputerUseError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error("computer use command failed with exit code {0}")]
    Failed(i32),
    #[error("invalid browser request: {0}")]
    InvalidBrowserRequest(String),
}

pub struct WinAppAdapter {
    runtime: Arc<dyn RuntimeAdapter>,
}

impl WinAppAdapter {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>) -> Self { Self { runtime } }

    pub fn build_command(&self, target: &WindowsTarget, action: &ComputerUseAction) -> CommandSpec {
        let mut args = vec!["ui".to_string()];
        match action {
            ComputerUseAction::Inspect => args.push("inspect".into()),
            ComputerUseAction::Search { query } => { args.push("search".into()); args.push(query.clone()); }
            ComputerUseAction::GetValue { selector } => { args.push("get-value".into()); args.push(selector.clone()); }
            ComputerUseAction::SetValue { selector, value } => { args.push("set-value".into()); args.push(selector.clone()); args.push(value.clone()); }
            ComputerUseAction::Invoke { selector } => { args.push("invoke".into()); args.push(selector.clone()); }
            ComputerUseAction::WaitFor { selector, timeout_ms } => {
                args.push("wait-for".into()); args.push(selector.clone());
                if let Some(timeout_ms) = timeout_ms { args.push("--timeout".into()); args.push(timeout_ms.to_string()); }
            }
            ComputerUseAction::Screenshot { output } => {
                args.push("screenshot".into());
                if let Some(output) = output { args.push("--output".into()); args.push(output.clone()); }
            }
        }
        match target {
            WindowsTarget::App(app) => { args.push("-a".into()); args.push(app.clone()); }
            WindowsTarget::Hwnd(hwnd) => { args.push("-w".into()); args.push(hwnd.to_string()); }
        }
        args.push("--json".into());
        CommandSpec::new("winapp").args(args)
    }

    pub async fn execute(&self, target: &WindowsTarget, action: &ComputerUseAction) -> Result<ComputerUseResult, ComputerUseError> {
        let output = self.runtime.execute(self.build_command(target, action)).await?;
        if output.exit_code != 0 { return Err(ComputerUseError::Failed(output.exit_code)); }
        let json = serde_json::from_str(&output.stdout).ok();
        Ok(ComputerUseResult { exit_code: output.exit_code, stdout: output.stdout, stderr: output.stderr, json })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BrowserAction {
    Navigate { url: String },
    Snapshot,
    Click { selector: String },
    Fill { selector: String, value: String },
    WaitFor { selector: String, timeout_ms: Option<u64> },
    Screenshot { output: String },
}

impl BrowserAction {
    pub fn requires_network(&self) -> bool { matches!(self, Self::Navigate { .. }) }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRequest {
    pub actions: Vec<BrowserAction>,
    pub profile_dir: Option<String>,
    pub headless: bool,
    pub timeout_ms: Option<u64>,
}

impl BrowserRequest {
    pub fn validate(&self) -> Result<(), ComputerUseError> {
        if self.actions.is_empty() { return Err(ComputerUseError::InvalidBrowserRequest("actions cannot be empty".into())); }
        if self.actions.len() > 64 { return Err(ComputerUseError::InvalidBrowserRequest("action plan exceeds 64 steps".into())); }
        Ok(())
    }
}

pub struct PlaywrightAdapter {
    runtime: Arc<dyn RuntimeAdapter>,
    bridge_path: String,
}

impl PlaywrightAdapter {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, bridge_path: impl Into<String>) -> Self {
        Self { runtime, bridge_path: bridge_path.into() }
    }

    pub fn build_command(&self, request: &BrowserRequest) -> Result<CommandSpec, ComputerUseError> {
        request.validate()?;
        let payload = serde_json::to_string(request).map_err(|error| ComputerUseError::InvalidBrowserRequest(error.to_string()))?;
        let mut command = CommandSpec::new("node").args([self.bridge_path.clone(), payload]);
        if let Some(timeout) = request.timeout_ms { command = command.timeout_ms(timeout); }
        Ok(command)
    }

    pub async fn execute(&self, request: &BrowserRequest) -> Result<ComputerUseResult, ComputerUseError> {
        let output = self.runtime.execute(self.build_command(request)?).await?;
        if output.exit_code != 0 { return Err(ComputerUseError::Failed(output.exit_code)); }
        Ok(ComputerUseResult { exit_code: output.exit_code, json: serde_json::from_str(&output.stdout).ok(), stdout: output.stdout, stderr: output.stderr })
    }
}
