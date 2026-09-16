use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use async_trait::async_trait;
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::Notify;

const BASE_ENV_KEYS: &[&str] = &[
    "PATH", "SystemRoot", "ComSpec", "PATHEXT", "USERPROFILE", "HOME", "TEMP", "TMP",
    "APPDATA", "LOCALAPPDATA", "XDG_CONFIG_HOME", "LANG", "LC_ALL", "USER", "USERNAME",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env_allowlist: BTreeSet<String>,
    pub timeout_ms: Option<u64>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env_allowlist: BTreeSet::new(),
            timeout_ms: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, path: impl Into<PathBuf>) -> Self {
        self.cwd = Some(path.into());
        self
    }

    pub fn allow_env(mut self, key: impl Into<String>) -> Self {
        self.env_allowlist.insert(key.into());
        self
    }

    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("failed to start process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("process timed out after {timeout_ms} ms")]
    TimedOut { timeout_ms: u64 },
    #[error("process execution cancelled")]
    Cancelled,
}

#[async_trait]
pub trait RuntimeAdapter: Send + Sync {
    async fn probe(&self) -> bool;
    async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError>;
    async fn cancel(&self) -> Result<(), RuntimeError>;
}

#[derive(Clone)]
pub struct NativeRuntime {
    cancel_signal: Arc<Notify>,
}

impl Default for NativeRuntime {
    fn default() -> Self {
        Self { cancel_signal: Arc::new(Notify::new()) }
    }
}

#[async_trait]
impl RuntimeAdapter for NativeRuntime {
    async fn probe(&self) -> bool {
        true
    }

    async fn execute(&self, spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        let started = Instant::now();
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        command.kill_on_drop(true);

        if let Some(cwd) = spec.cwd {
            command.current_dir(cwd);
        }

        command.env_clear();
        for key in BASE_ENV_KEYS
            .iter()
            .copied()
            .chain(spec.env_allowlist.iter().map(String::as_str))
        {
            if let Ok(value) = std::env::var(key) {
                command.env(key, value);
            }
        }

        let mut output_future = Box::pin(command.output());
        let output = match spec.timeout_ms {
            Some(timeout_ms) => {
                tokio::select! {
                    result = &mut output_future => result?,
                    _ = self.cancel_signal.notified() => return Err(RuntimeError::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
                        return Err(RuntimeError::TimedOut { timeout_ms });
                    }
                }
            }
            None => {
                tokio::select! {
                    result = &mut output_future => result?,
                    _ = self.cancel_signal.notified() => return Err(RuntimeError::Cancelled),
                }
            }
        };

        Ok(CommandResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            duration_ms: started.elapsed().as_millis(),
        })
    }

    async fn cancel(&self) -> Result<(), RuntimeError> {
        self.cancel_signal.notify_waiters();
        Ok(())
    }
}

#[derive(Clone)]
pub struct FakeRuntime {
    result: CommandResult,
}

impl FakeRuntime {
    pub fn new(result: CommandResult) -> Self {
        Self { result }
    }
}

#[async_trait]
impl RuntimeAdapter for FakeRuntime {
    async fn probe(&self) -> bool {
        true
    }

    async fn execute(&self, _spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        Ok(self.result.clone())
    }

    async fn cancel(&self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
