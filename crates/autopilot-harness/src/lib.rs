use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use autopilot_runtime::{CommandSpec, RuntimeAdapter, RuntimeError};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BackendKind {
    OpenCode,
    Codex,
    Claude,
    Gemini,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    pub backend: BackendKind,
    pub prompt: String,
    pub workdir: PathBuf,
    pub model: Option<String>,
    pub env_allowlist: BTreeSet<String>,
    pub timeout_ms: Option<u64>,
}

impl RunSpec {
    pub fn new(backend: BackendKind, prompt: impl Into<String>, workdir: PathBuf) -> Self {
        Self {
            backend,
            prompt: prompt.into(),
            workdir,
            model: None,
            env_allowlist: BTreeSet::new(),
            timeout_ms: None,
        }
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
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
pub enum Termination {
    Exited,
    NonZero(i32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub backend: BackendKind,
    pub termination: Termination,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliBackendSpec {
    pub program: String,
    pub prefix_args: Vec<String>,
    pub model_flag: Option<String>,
    pub prompt_flag: Option<String>,
}

impl CliBackendSpec {
    fn command(&self, run: &RunSpec) -> CommandSpec {
        let mut args = self.prefix_args.clone();
        if let (Some(flag), Some(model)) = (&self.model_flag, &run.model) {
            args.push(flag.clone());
            args.push(model.clone());
        }
        if let Some(flag) = &self.prompt_flag {
            args.push(flag.clone());
        }
        args.push(run.prompt.clone());

        let mut command = CommandSpec::new(self.program.clone()).args(args).cwd(run.workdir.clone());
        for key in &run.env_allowlist {
            command = command.allow_env(key.clone());
        }
        if let Some(timeout_ms) = run.timeout_ms {
            command = command.timeout_ms(timeout_ms);
        }
        command
    }
}

#[derive(Clone, Debug, Default)]
pub struct BackendRegistry {
    custom: BTreeMap<String, CliBackendSpec>,
}

impl BackendRegistry {
    pub fn register_custom(&mut self, name: impl Into<String>, spec: CliBackendSpec) {
        self.custom.insert(name.into(), spec);
    }

    pub fn builtin_kinds() -> [BackendKind; 4] {
        [BackendKind::OpenCode, BackendKind::Codex, BackendKind::Claude, BackendKind::Gemini]
    }

    pub fn resolve(&self, kind: &BackendKind) -> Result<CliBackendSpec, HarnessError> {
        let spec = match kind {
            BackendKind::OpenCode => CliBackendSpec {
                program: "opencode".into(), prefix_args: vec!["run".into()], model_flag: Some("--model".into()), prompt_flag: None,
            },
            BackendKind::Codex => CliBackendSpec {
                program: "codex".into(), prefix_args: vec!["exec".into()], model_flag: Some("--model".into()), prompt_flag: None,
            },
            BackendKind::Claude => CliBackendSpec {
                program: "claude".into(), prefix_args: vec![], model_flag: Some("--model".into()), prompt_flag: Some("-p".into()),
            },
            BackendKind::Gemini => CliBackendSpec {
                program: "gemini".into(), prefix_args: vec![], model_flag: Some("-m".into()), prompt_flag: Some("-p".into()),
            },
            BackendKind::Custom(name) => self.custom.get(name).cloned().ok_or_else(|| HarnessError::UnknownBackend(name.clone()))?,
        };
        Ok(spec)
    }

    pub fn build_command(&self, run: &RunSpec) -> Result<CommandSpec, HarnessError> {
        Ok(self.resolve(&run.backend)?.command(run))
    }
}

#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("unknown custom backend: {0}")]
    UnknownBackend(String),
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

pub struct AgentHarness {
    runtime: Arc<dyn RuntimeAdapter>,
    registry: BackendRegistry,
}

impl AgentHarness {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, registry: BackendRegistry) -> Self {
        Self { runtime, registry }
    }

    pub async fn run(&self, spec: RunSpec) -> Result<RunResult, HarnessError> {
        let command = self.registry.build_command(&spec)?;
        let output = self.runtime.execute(command).await?;
        let termination = if output.exit_code == 0 { Termination::Exited } else { Termination::NonZero(output.exit_code) };
        Ok(RunResult {
            backend: spec.backend,
            termination,
            stdout: output.stdout,
            stderr: output.stderr,
            duration_ms: output.duration_ms,
        })
    }
}
