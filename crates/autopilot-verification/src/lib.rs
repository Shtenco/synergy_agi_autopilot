use std::sync::Arc;
use autopilot_domain::Evidence;
use autopilot_runtime::{CommandSpec, RuntimeAdapter, RuntimeError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationCheck {
    pub name: String,
    pub command: CommandSpec,
}

impl VerificationCheck {
    pub fn new(name: impl Into<String>, command: CommandSpec) -> Self { Self { name: name.into(), command } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationReport {
    pub passed: bool,
    pub evidence: Vec<Evidence>,
}

pub struct VerificationPipeline {
    runtime: Arc<dyn RuntimeAdapter>,
    fail_fast: bool,
    checks: Vec<VerificationCheck>,
}

impl VerificationPipeline {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, fail_fast: bool, checks: Vec<VerificationCheck>) -> Self {
        Self { runtime, fail_fast, checks }
    }

    pub async fn run(&self) -> Result<VerificationReport, RuntimeError> {
        let mut evidence = Vec::with_capacity(self.checks.len());
        let mut passed = true;
        for check in &self.checks {
            let result = self.runtime.execute(check.command.clone()).await?;
            if result.exit_code == 0 {
                evidence.push(Evidence::passed(check.name.clone(), format!("exit=0; {}ms", result.duration_ms)));
            } else {
                passed = false;
                evidence.push(Evidence::failed(
                    check.name.clone(),
                    format!("exit={}; stderr={}", result.exit_code, result.stderr.trim()),
                ));
                if self.fail_fast { break; }
            }
        }
        Ok(VerificationReport { passed, evidence })
    }
}
