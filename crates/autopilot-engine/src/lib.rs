use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use autopilot_audit::AuditStore;
use autopilot_core::{
    AgentExecutor, AgentOutcome, Director, DirectorConfig, DirectorError, GoalRequest, GoalRunSummary,
    PlannedTask, Planner, VerificationOutcome, Verifier,
};
use autopilot_domain::{AgentRole, AuditEvent, Capability, CorrelationId, Evidence, GoalState, RiskClass};
use autopilot_fleet::{FleetScheduler, FleetTask};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::{PolicyDecision, PolicyEngine};
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};
use autopilot_runtime::{CommandSpec, RuntimeAdapter};
use autopilot_verification::{VerificationCheck, VerificationPipeline};
use autopilot_workspace::WorkspaceManager;

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
                evidence: vec![Evidence::failed(
                    "agent-execution",
                    "one or more agent executions failed",
                )],
            });
        }

        if self.checks.is_empty() {
            return Ok(VerificationOutcome {
                passed: true,
                evidence: vec![Evidence::passed(
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

#[derive(Clone, Debug)]
pub struct FleetEngineConfig {
    pub repo_root: PathBuf,
    pub worktree_root: PathBuf,
    pub base_ref: String,
    pub routing_profile: RoutingProfile,
    pub candidates: Vec<ModelCandidate>,
    pub allowed_env: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub verification: Vec<VerificationCommand>,
    pub max_repairs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleRun {
    pub role: AgentRole,
    pub provider: String,
    pub model: String,
    pub backend: BackendKind,
    pub workdir: PathBuf,
    pub success: bool,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct FleetRunSummary {
    pub final_state: GoalState,
    pub repairs: usize,
    pub evidence: Vec<Evidence>,
    pub role_runs: Vec<RoleRun>,
    pub worktree: Option<PathBuf>,
    pub correlation_id: CorrelationId,
}

pub fn backend_for_provider(provider: &str) -> Result<BackendKind, String> {
    match provider.to_ascii_lowercase().as_str() {
        "opencode" => Ok(BackendKind::OpenCode),
        "openai" | "codex" => Ok(BackendKind::Codex),
        "anthropic" | "claude" | "claude-code" => Ok(BackendKind::Claude),
        "google" | "gemini" => Ok(BackendKind::Gemini),
        other => Err(format!("unsupported routed provider: {other}")),
    }
}

pub struct FleetEngine {
    runtime: Arc<dyn RuntimeAdapter>,
    audit: Arc<dyn AuditStore>,
    policy: PolicyEngine,
    config: FleetEngineConfig,
}

impl FleetEngine {
    pub fn new(
        runtime: Arc<dyn RuntimeAdapter>,
        audit: Arc<dyn AuditStore>,
        config: FleetEngineConfig,
    ) -> Self {
        Self {
            runtime,
            audit,
            policy: PolicyEngine::default(),
            config,
        }
    }

    async fn audit(&self, correlation: CorrelationId, actor: &str, event: &str, status: &str) -> Result<(), String> {
        self.audit
            .append(AuditEvent::new(correlation, actor, event, RiskClass::Low, status))
            .await
    }

    fn route(&self, role: &AgentRole) -> Result<(ModelCandidate, BackendKind), String> {
        let candidate = ModelRouter::select(role, self.config.routing_profile, &self.config.candidates)
            .ok_or_else(|| format!("no healthy model candidate for role {role:?}"))?;
        let backend = backend_for_provider(&candidate.provider)?;
        Ok((candidate, backend))
    }

    async fn run_role(
        &self,
        role: AgentRole,
        prompt: String,
        workdir: PathBuf,
        correlation: CorrelationId,
    ) -> Result<RoleRun, String> {
        let (candidate, backend) = self.route(&role)?;
        self.audit(correlation, &format!("{role:?}"), "agent-start", "running").await?;

        let mut spec = RunSpec::new(backend.clone(), prompt, workdir.clone()).model(candidate.model.clone());
        for key in &self.config.allowed_env {
            spec = spec.allow_env(key.clone());
        }
        if let Some(timeout_ms) = self.config.timeout_ms {
            spec = spec.timeout_ms(timeout_ms);
        }

        let result = AgentHarness::new(self.runtime.clone(), BackendRegistry::default())
            .run(spec)
            .await
            .map_err(|error| error.to_string())?;
        let success = matches!(result.termination, Termination::Exited);
        let summary = if success {
            result.stdout
        } else if result.stderr.is_empty() {
            format!("agent terminated with {:?}", result.termination)
        } else {
            result.stderr
        };
        self.audit(
            correlation,
            &format!("{role:?}"),
            "agent-finish",
            if success { "passed" } else { "failed" },
        )
        .await?;

        Ok(RoleRun {
            role,
            provider: candidate.provider,
            model: candidate.model,
            backend,
            workdir,
            success,
            summary,
        })
    }

    async fn verify(&self, workdir: &Path) -> Result<VerificationOutcome, String> {
        if self.config.verification.is_empty() {
            return Ok(VerificationOutcome {
                passed: true,
                evidence: vec![Evidence::passed("verification", "no verification commands configured")],
            });
        }
        let checks = self
            .config
            .verification
            .iter()
            .map(|check| {
                VerificationCheck::new(
                    check.name.clone(),
                    CommandSpec::new(check.program.clone())
                        .args(check.args.clone())
                        .cwd(workdir.to_path_buf()),
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

    fn fleet_scheduler() -> Result<FleetScheduler, String> {
        FleetScheduler::new(vec![
            FleetTask {
                id: "architect".into(),
                role: AgentRole::Architect,
                dependencies: vec![],
                writes: false,
                workspace_id: "source".into(),
            },
            FleetTask {
                id: "coder".into(),
                role: AgentRole::Coder,
                dependencies: vec!["architect".into()],
                writes: true,
                workspace_id: "change".into(),
            },
            FleetTask {
                id: "tester".into(),
                role: AgentRole::Tester,
                dependencies: vec!["coder".into()],
                writes: false,
                workspace_id: "change".into(),
            },
            FleetTask {
                id: "reviewer".into(),
                role: AgentRole::Reviewer,
                dependencies: vec!["coder".into()],
                writes: false,
                workspace_id: "change".into(),
            },
            FleetTask {
                id: "security".into(),
                role: AgentRole::Security,
                dependencies: vec!["coder".into()],
                writes: false,
                workspace_id: "change".into(),
            },
        ])
        .map_err(|error| error.to_string())
    }

    fn worktree_label(objective: &str) -> String {
        let mut hasher = DefaultHasher::new();
        objective.hash(&mut hasher);
        format!("goal-{:x}", hasher.finish())
    }

    pub async fn run(&self, objective: impl Into<String>) -> Result<FleetRunSummary, String> {
        let objective = objective.into();
        let correlation = CorrelationId::new();
        self.audit(correlation, "Director", "Planning", "entered").await?;

        for (capability, risk) in [
            (Capability::ReadWorkspace, RiskClass::Low),
            (Capability::WriteWorkspace, RiskClass::Medium),
            (Capability::ExecuteProcess, RiskClass::Medium),
        ] {
            if self.policy.evaluate(capability, risk) != PolicyDecision::Allow {
                self.audit(correlation, "Director", "PolicyCheck", "blocked").await?;
                return Ok(FleetRunSummary {
                    final_state: GoalState::Failed,
                    repairs: 0,
                    evidence: vec![Evidence::failed("policy", "required fleet capability was not allowed")],
                    role_runs: vec![],
                    worktree: None,
                    correlation_id: correlation,
                });
            }
        }

        let scheduler = Self::fleet_scheduler()?;
        let workspace = WorkspaceManager::new(
            self.config.repo_root.clone(),
            self.config.worktree_root.clone(),
            self.runtime.clone(),
        );
        let mut completed = BTreeSet::new();
        let mut role_runs = Vec::new();

        let initial = scheduler.ready(&completed, &[]);
        let architect_task = initial
            .into_iter()
            .find(|task| task.role == AgentRole::Architect)
            .ok_or_else(|| "architect task was not ready".to_string())?;
        let architect = self
            .run_role(
                architect_task.role,
                format!(
                    "Act as the Architect. Analyze the repository for this goal without editing files. Goal: {objective}. Return a concrete implementation plan, affected files, interfaces, risks, and acceptance checks."
                ),
                self.config.repo_root.clone(),
                correlation,
            )
            .await?;
        if !architect.success {
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("architect", "architecture agent failed")],
                role_runs: vec![architect],
                worktree: None,
                correlation_id: correlation,
            });
        }
        let architecture = architect.summary.clone();
        role_runs.push(architect);
        completed.insert("architect".to_string());

        let worktree = workspace
            .create_worktree(&Self::worktree_label(&objective), &self.config.base_ref)
            .await
            .map_err(|error| error.to_string())?;

        let coder_task = scheduler
            .ready(&completed, &[])
            .into_iter()
            .find(|task| task.role == AgentRole::Coder)
            .ok_or_else(|| "coder task was not ready".to_string())?;
        let coder = self
            .run_role(
                coder_task.role,
                format!(
                    "Act as the Coder. Implement the goal in this isolated worktree. Goal: {objective}\nArchitect plan:\n{architecture}\nDo not publish or force-push. Keep changes scoped and leave the worktree ready for independent review."
                ),
                worktree.path.clone(),
                correlation,
            )
            .await?;
        if !coder.success {
            role_runs.push(coder);
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("coder", "coding agent failed")],
                role_runs,
                worktree: Some(worktree.path),
                correlation_id: correlation,
            });
        }
        role_runs.push(coder);
        completed.insert("coder".to_string());

        let ready_reviews = scheduler.ready(&completed, &[]);
        let by_role: HashMap<AgentRole, FleetTask> = ready_reviews
            .into_iter()
            .map(|task| (task.role.clone(), task))
            .collect();
        for role in [AgentRole::Tester, AgentRole::Reviewer, AgentRole::Security] {
            if !by_role.contains_key(&role) {
                return Err(format!("{role:?} task was not ready after coder"));
            }
        }

        let tester_prompt = format!(
            "Act as an independent Tester. Inspect the completed implementation for goal: {objective}. Run or inspect relevant tests, identify missing regression coverage, and do not modify files."
        );
        let reviewer_prompt = format!(
            "Act as an independent Reviewer. Review the implementation for goal: {objective}. Focus on correctness, regressions, maintainability, concurrency, portability and requirement coverage. Do not modify files."
        );
        let security_prompt = format!(
            "Act as an independent Security reviewer. Review the implementation for goal: {objective}. Check secret exposure, command injection, path traversal, unsafe process execution, dependency and supply-chain risk. Do not modify files."
        );
        let review_workdir = worktree.path.clone();
        let (tester, reviewer, security) = tokio::join!(
            self.run_role(AgentRole::Tester, tester_prompt, review_workdir.clone(), correlation),
            self.run_role(AgentRole::Reviewer, reviewer_prompt, review_workdir.clone(), correlation),
            self.run_role(AgentRole::Security, security_prompt, review_workdir.clone(), correlation),
        );
        let tester = tester?;
        let reviewer = reviewer?;
        let security = security?;
        let reviews_passed = tester.success && reviewer.success && security.success;
        role_runs.extend([tester, reviewer, security]);
        if !reviews_passed {
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("cross-review", "one or more independent review agents failed")],
                role_runs,
                worktree: Some(worktree.path),
                correlation_id: correlation,
            });
        }

        let mut verification = self.verify(&worktree.path).await?;
        let mut repairs = 0usize;
        let mut evidence = verification.evidence.clone();
        while !verification.passed && repairs < self.config.max_repairs {
            repairs += 1;
            let debugger = self
                .run_role(
                    AgentRole::Debugger,
                    format!(
                        "Act as the Debugger. Verification failed for goal: {objective}. Diagnose the failures in this worktree, make the minimum corrective edits, and leave the repository ready for another verification cycle. Previous evidence: {evidence:?}"
                    ),
                    worktree.path.clone(),
                    correlation,
                )
                .await?;
            let debugger_success = debugger.success;
            role_runs.push(debugger);
            if !debugger_success {
                break;
            }
            verification = self.verify(&worktree.path).await?;
            evidence.extend(verification.evidence.clone());
        }

        let final_state = if verification.passed {
            GoalState::Completed
        } else {
            GoalState::Failed
        };
        self.audit(
            correlation,
            "Director",
            &format!("{final_state:?}"),
            if final_state == GoalState::Completed { "passed" } else { "failed" },
        )
        .await?;

        Ok(FleetRunSummary {
            final_state,
            repairs,
            evidence,
            role_runs,
            worktree: Some(worktree.path),
            correlation_id: correlation,
        })
    }
}
