use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use autopilot_audit::AuditStore;
use autopilot_core::{
    AgentExecutor, AgentOutcome, Director, DirectorConfig, DirectorError, GoalRequest, GoalRunSummary,
    PlannedTask, Planner, VerificationOutcome, Verifier,
};
use autopilot_domain::{AgentRole, Capability, RiskClass};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::PolicyEngine;
use autopilot_runtime::{CommandSpec, RuntimeAdapter};
use autopilot_verification::{VerificationCheck, VerificationPipeline};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

impl VerificationCommand {
    pub fn new<I, S>(name: impl Into<String>, program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            name: name.into(),
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub backend: BackendKind,
    pub model: Option<String>,
    pub workdir: PathBuf,
    pub allowed_env: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub verification: Vec<VerificationCommand>,
    pub max_repairs: usize,
}

struct SingleTaskPlanner;

#[async_trait]
impl Planner for SingleTaskPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        Ok(vec![PlannedTask {
            role: AgentRole::Coder,
            objective: request.objective.clone(),
            capability: Capability::ExecuteProcess,
            risk: RiskClass::Medium,
        }])
    }
}

struct HarnessExecutor {
    harness: AgentHarness,
    config: EngineConfig,
}

#[async_trait]
impl AgentExecutor for HarnessExecutor {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        let mut spec = RunSpec::new(
            self.config.backend.clone(),
            task.objective.clone(),
            self.config.workdir.clone(),
        );
        if let Some(model) = &self.config.model {
            spec = spec.model(model.clone());
        }
        for key in &self.config.allowed_env {
            spec = spec.allow_env(key.clone());
        }
        if let Some(timeout_ms) = self.config.timeout_ms {
            spec = spec.timeout_ms(timeout_ms);
        }

        let result = self.harness.run(spec).await.map_err(|error| error.to_string())?;
        let success = matches!(result.termination, Termination::Exited);
        let summary = if success {
            result.stdout
        } else if result.stderr.is_empty() {
            format!("agent terminated with {:?}", result.termination)
        } else {
            result.stderr
        };

        Ok(AgentOutcome {
            objective: task.objective.clone(),
            success,
            summary,
        })
    }
}

struct PipelineVerifier {
    runtime: Arc<dyn RuntimeAdapter>,
    workdir: PathBuf,
    checks: Vec<VerificationCommand>,
}

#[async_trait]
impl Verifier for PipelineVerifier {
    async fn verify(
        &self,
        _request: &GoalRequest,
        outcomes: &[AgentOutcome],
    ) -> Result<VerificationOutcome, String> {
        if outcomes.is_empty() || outcomes.iter().any(|outcome| !outcome.success) {
            return Ok(VerificationOutcome {
                passed: false,
                evidence: vec![autopilot_domain::Evidence::failed(
                    "agent-execution",
                    "one or more agent executions failed",
                )],
            });
        }

        if self.checks.is_empty() {
            return Ok(VerificationOutcome {
                passed: true,
                evidence: vec![autopilot_domain::Evidence::passed(
                    "agent-execution",
                    "agent completed and no verification commands were configured",
                )],
            });
        }

        let checks = self
            .checks
            .iter()
            .map(|check| {
                VerificationCheck::new(
                    check.name.clone(),
                    CommandSpec::new(check.program.clone())
                        .args(check.args.clone())
                        .cwd(self.workdir.clone()),
                )
            })
            .collect();
        let report = VerificationPipeline::new(self.runtime.clone(), false, checks)
            .run()
            .await
            .map_err(|error| error.to_string())?;
        Ok(VerificationOutcome {
            passed: report.passed,
            evidence: report.evidence,
        })
    }
}

pub struct AutonomousEngine {
    director: Director,
}

impl AutonomousEngine {
    pub fn new(
        runtime: Arc<dyn RuntimeAdapter>,
        audit: Arc<dyn AuditStore>,
        config: EngineConfig,
    ) -> Self {
        let executor = Arc::new(HarnessExecutor {
            harness: AgentHarness::new(runtime.clone(), BackendRegistry::default()),
            config: config.clone(),
        });
        let verifier = Arc::new(PipelineVerifier {
            runtime,
            workdir: config.workdir.clone(),
            checks: config.verification.clone(),
        });
        let director = Director::new(
            DirectorConfig {
                max_repairs: config.max_repairs,
            },
            PolicyEngine::default(),
            audit,
            Arc::new(SingleTaskPlanner),
            executor,
            verifier,
        );
        Self { director }
    }

    pub async fn run(&self, objective: impl Into<String>) -> Result<GoalRunSummary, DirectorError> {
        self.director
            .run_goal(
                GoalRequest::new(objective)
                    .require(Capability::ExecuteProcess, RiskClass::Medium),
            )
            .await
    }
}
