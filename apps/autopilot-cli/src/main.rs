use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::{Parser, Subcommand};
use autopilot_audit::{AuditStore, InMemoryAuditStore};
use autopilot_core::{AgentExecutor, AgentOutcome, Director, DirectorConfig, GoalRequest, PlannedTask, Planner, VerificationOutcome, Verifier};
use autopilot_delivery::{CiReport, CiState, DeliveryConfig, DeliveryEngine};
use autopilot_domain::{AgentRole, Capability, Evidence, RiskClass};
use autopilot_engine::{backend_for_provider, AutonomousEngine, EngineConfig, FleetEngine, FleetEngineConfig, VerificationCommand};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::PolicyEngine;
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};
use autopilot_runtime::{CommandSpec, DockerRuntime, ExecutionProfile, NativeRuntime, RuntimeAdapter, WslRuntime};

#[derive(Parser)]
#[command(name = "open-autopilot", version, about = "OPEN AUTOPILOT local autonomous engineering harness")]
struct Cli { #[command(subcommand)] command: Commands }

#[derive(Subcommand)]
enum Commands {
    Demo { #[arg(long)] goal: String },
    Backends,
    Agent {
        #[arg(long)] backend: String,
        #[arg(long)] prompt: String,
        #[arg(long)] workdir: PathBuf,
        #[arg(long)] model: Option<String>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long)] timeout_ms: Option<u64>,
    },
    Autopilot {
        #[arg(long)] backend: String,
        #[arg(long)] goal: String,
        #[arg(long)] workdir: PathBuf,
        #[arg(long)] model: Option<String>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long)] timeout_ms: Option<u64>,
        #[arg(long, default_value_t = 2)] max_repairs: usize,
        #[arg(long)] verify_program: Option<String>,
        #[arg(long = "verify-arg")] verify_args: Vec<String>,
    },
    Fleet {
        #[arg(long)] goal: String,
        #[arg(long)] repo: PathBuf,
        #[arg(long)] worktree_root: Option<PathBuf>,
        #[arg(long, default_value = "main")] base_ref: String,
        #[arg(long, default_value = "opencode")] provider: String,
        #[arg(long)] model: Option<String>,
        #[arg(long = "route")] routes: Vec<String>,
        #[arg(long, default_value = "best-quality")] profile: String,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long)] timeout_ms: Option<u64>,
        #[arg(long, default_value_t = 2)] max_repairs: usize,
        #[arg(long)] verify_program: Option<String>,
        #[arg(long = "verify-arg")] verify_args: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long, default_value = "docker")] review_runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
        #[arg(long, default_value_t = false)] publish: bool,
        #[arg(long, default_value = "origin")] remote: String,
        #[arg(long, default_value_t = 2)] max_ci_repairs: usize,
        #[arg(long, default_value_t = 12)] ci_poll_attempts: usize,
        #[arg(long, default_value_t = 5_000)] ci_poll_interval_ms: u64,
    },
}

fn parse_backend(value: &str) -> Result<BackendKind, String> {
    match value.to_ascii_lowercase().as_str() {
        "opencode" => Ok(BackendKind::OpenCode),
        "codex" => Ok(BackendKind::Codex),
        "claude" | "claude-code" => Ok(BackendKind::Claude),
        "gemini" => Ok(BackendKind::Gemini),
        other => Err(format!("unsupported builtin backend: {other}")),
    }
}

fn parse_role(value: &str) -> Result<AgentRole, String> {
    match value.to_ascii_lowercase().as_str() {
        "architect" => Ok(AgentRole::Architect),
        "coder" => Ok(AgentRole::Coder),
        "tester" => Ok(AgentRole::Tester),
        "reviewer" => Ok(AgentRole::Reviewer),
        "security" => Ok(AgentRole::Security),
        "debugger" => Ok(AgentRole::Debugger),
        other => Err(format!("unsupported fleet role in route: {other}")),
    }
}

fn parse_profile(value: &str) -> Result<RoutingProfile, String> {
    match value.to_ascii_lowercase().as_str() {
        "best" | "best-quality" | "quality" => Ok(RoutingProfile::BestQuality),
        "balanced" => Ok(RoutingProfile::Balanced),
        "budget" => Ok(RoutingProfile::Budget),
        other => Err(format!("unsupported routing profile: {other}")),
    }
}

fn parse_route(value: &str) -> Result<(AgentRole, String, String), String> {
    let (role, target) = value.split_once('=').ok_or_else(|| format!("route must use role=provider[:model], got: {value}"))?;
    let role = parse_role(role)?;
    let (provider, model) = match target.split_once(':') {
        Some((provider, model)) => (provider.to_string(), model.to_string()),
        None => (target.to_string(), String::new()),
    };
    if provider.trim().is_empty() { return Err(format!("route provider is empty: {value}")); }
    Ok((role, provider, model))
}

fn candidate(provider: String, model: String, role: AgentRole) -> ModelCandidate {
    let mut affinities = BTreeMap::new();
    affinities.insert(role, 100);
    ModelCandidate { provider, model, quality: 90, cost_per_million: 1.0, latency_ms: 500, healthy: true, role_affinities: affinities }
}

fn fleet_candidates(default_provider: String, default_model: Option<String>, routes: Vec<String>) -> Result<Vec<ModelCandidate>, String> {
    let mut overrides = BTreeMap::new();
    for route in routes {
        let (role, provider, model) = parse_route(&route)?;
        overrides.insert(role, (provider, model));
    }
    let default_model = default_model.unwrap_or_default();
    let roles = [AgentRole::Architect, AgentRole::Coder, AgentRole::Tester, AgentRole::Reviewer, AgentRole::Security, AgentRole::Debugger];
    Ok(roles.into_iter().map(|role| {
        let (provider, model) = overrides.get(&role).cloned().unwrap_or_else(|| (default_provider.clone(), default_model.clone()));
        candidate(provider, model, role)
    }).collect())
}

fn build_runtime(kind: &str, workspace: &Path, docker_image: Option<&str>, wsl_distro: Option<&str>) -> Result<Arc<dyn RuntimeAdapter>, String> {
    match kind.to_ascii_lowercase().as_str() {
        "native" | "windows" => Ok(Arc::new(NativeRuntime::default())),
        "wsl" | "wsl2" => Ok(Arc::new(WslRuntime::new(wsl_distro.map(str::to_string)))),
        "docker" => {
            let image = docker_image.filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "--docker-image is required when a Docker runtime is selected; the image must contain the selected coding-agent CLI".to_string())?;
            Ok(Arc::new(DockerRuntime::new(image, workspace.to_path_buf())))
        }
        other => Err(format!("unsupported runtime: {other}; expected native, wsl, or docker")),
    }
}

async fn current_branch(runtime: Arc<dyn RuntimeAdapter>, worktree: &Path) -> Result<String, String> {
    let result = runtime.execute(CommandSpec::new("git").args(["branch", "--show-current"]).cwd(worktree.to_path_buf()).profile(ExecutionProfile::WorkspaceWrite))
        .await.map_err(|error| error.to_string())?;
    if result.exit_code != 0 { return Err(format!("cannot resolve delivery branch: {}", result.stderr)); }
    let branch = result.stdout.trim().to_string();
    if branch.is_empty() { return Err("delivery worktree is detached; refusing autonomous push".into()); }
    Ok(branch)
}

async fn run_acceptance(runtime: Arc<dyn RuntimeAdapter>, worktree: &Path, checks: &[VerificationCommand]) -> Result<bool, String> {
    for check in checks {
        let result = runtime.execute(CommandSpec::new(check.program.clone()).args(check.args.clone()).cwd(worktree.to_path_buf()).profile(ExecutionProfile::WorkspaceWrite))
            .await.map_err(|error| error.to_string())?;
        if result.exit_code != 0 { return Ok(false); }
    }
    Ok(true)
}

async fn wait_for_ci(engine: &DeliveryEngine, attempts: usize, interval_ms: u64) -> Result<CiReport, String> {
    let mut last = engine.read_ci().await.map_err(|error| error.to_string())?;
    for _ in 1..attempts.max(1) {
        if last.state != CiState::Pending { return Ok(last); }
        tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        last = engine.read_ci().await.map_err(|error| error.to_string())?;
    }
    Ok(last)
}

async fn repair_failing_ci(
    runtime: Arc<dyn RuntimeAdapter>, worktree: &Path, goal: &str, candidates: &[ModelCandidate],
    routing_profile: RoutingProfile, allowed_env: &[String], timeout_ms: Option<u64>, report: &CiReport,
) -> Result<(), String> {
    let candidate = ModelRouter::select(&AgentRole::Debugger, routing_profile, candidates)
        .ok_or_else(|| "no healthy Debugger model available for CI repair".to_string())?;
    let backend = backend_for_provider(&candidate.provider)?;
    let failures = report.checks.iter().filter(|check| {
        let bucket = check.bucket.to_ascii_lowercase();
        let state = check.state.to_ascii_lowercase();
        bucket == "fail" || state.contains("fail") || state.contains("error") || state.contains("cancel")
    }).map(|check| format!("{} [{}] {}", check.name, check.state, check.link)).collect::<Vec<_>>().join("\n");
    let prompt = format!("Act as the Debugger. GitHub CI failed after a locally verified change. Goal: {goal}\nFailing checks:\n{failures}\nDiagnose and make the minimum corrective edits in this worktree. Do not force-push, publish releases, or expose secrets.");
    let mut spec = RunSpec::new(backend, prompt, worktree.to_path_buf()).model(candidate.model).profile(ExecutionProfile::WorkspaceWrite);
    for key in allowed_env { spec = spec.allow_env(key.clone()); }
    if let Some(timeout) = timeout_ms { spec = spec.timeout_ms(timeout); }
    let result = AgentHarness::new(runtime, BackendRegistry::default()).run(spec).await.map_err(|error| error.to_string())?;
    if !matches!(result.termination, Termination::Exited) { return Err(format!("CI Debugger failed: {:?}: {}", result.termination, result.stderr)); }
    Ok(())
}

struct DemoPlanner;
#[async_trait]
impl Planner for DemoPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        Ok(vec![PlannedTask { role: AgentRole::Coder, objective: request.objective.clone(), capability: Capability::WriteWorkspace, risk: RiskClass::Medium }])
    }
}
struct DemoExecutor;
#[async_trait]
impl AgentExecutor for DemoExecutor {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        Ok(AgentOutcome { objective: task.objective.clone(), success: true, summary: "Coder agent completed demo execution".into() })
    }
}
struct DemoVerifier;
#[async_trait]
impl Verifier for DemoVerifier {
    async fn verify(&self, _request: &GoalRequest, outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String> {
        let passed = !outcomes.is_empty() && outcomes.iter().all(|outcome| outcome.success);
        Ok(VerificationOutcome { passed, evidence: vec![if passed { Evidence::passed("demo-verification", "all demo agent outcomes succeeded") } else { Evidence::failed("demo-verification", "one or more demo outcomes failed") }] })
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Demo { goal } => {
            let audit = Arc::new(InMemoryAuditStore::default());
            let director = Director::new(DirectorConfig::default(), PolicyEngine::default(), audit.clone(), Arc::new(DemoPlanner), Arc::new(DemoExecutor), Arc::new(DemoVerifier));
            let summary = director.run_goal(GoalRequest::new(goal).require(Capability::WriteWorkspace, RiskClass::Medium)).await.map_err(|error| error.to_string())?;
            println!("final_state={:?}\naudit_events={}", summary.final_state, audit.list_by_correlation(summary.correlation_id).await?.len());
        }
        Commands::Backends => for backend in BackendRegistry::builtin_kinds() { println!("{backend:?}"); },
        Commands::Agent { backend, prompt, workdir, model, allow_env, timeout_ms } => {
            let mut spec = RunSpec::new(parse_backend(&backend)?, prompt, workdir);
            if let Some(model) = model { spec = spec.model(model); }
            for key in allow_env { spec = spec.allow_env(key); }
            if let Some(timeout) = timeout_ms { spec = spec.timeout_ms(timeout); }
            let result = AgentHarness::new(Arc::new(NativeRuntime::default()), BackendRegistry::default()).run(spec).await.map_err(|error| error.to_string())?;
            println!("termination={:?}\nduration_ms={}", result.termination, result.duration_ms);
            if !result.stdout.is_empty() { print!("{}", result.stdout); }
            if !result.stderr.is_empty() { eprint!("{}", result.stderr); }
        }
        Commands::Autopilot { backend, goal, workdir, model, allow_env, timeout_ms, max_repairs, verify_program, verify_args } => {
            let verification = verify_program.map(|program| vec![VerificationCommand::new("acceptance", program, verify_args)]).unwrap_or_default();
            let runtime = Arc::new(NativeRuntime::default());
            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = AutonomousEngine::new(runtime, audit.clone(), EngineConfig { backend: parse_backend(&backend)?, model, workdir, allowed_env: allow_env, timeout_ms, verification, max_repairs });
            let summary = engine.run(goal).await.map_err(|error| error.to_string())?;
            println!("final_state={:?}\nrepairs={}\nevidence={}\naudit_events={}", summary.final_state, summary.repairs, summary.evidence.len(), audit.list_by_correlation(summary.correlation_id).await?.len());
        }
        Commands::Fleet {
            goal, repo, worktree_root, base_ref, provider, model, routes, profile, allow_env,
            timeout_ms, max_repairs, verify_program, verify_args,
            runtime: writer_runtime_kind, review_runtime: review_runtime_kind,
            docker_image, wsl_distro, publish, remote, max_ci_repairs, ci_poll_attempts, ci_poll_interval_ms,
        } => {
            let routing_profile = parse_profile(&profile)?;
            let verification = verify_program.map(|program| vec![VerificationCommand::new("acceptance", program, verify_args)]).unwrap_or_default();
            if publish && verification.is_empty() { return Err("--publish requires --verify-program so autonomous delivery has local acceptance evidence".into()); }
            let candidates = fleet_candidates(provider, model, routes)?;
            let worktree_root = worktree_root.unwrap_or_else(|| repo.join(".open-autopilot").join("worktrees"));
            let writer_runtime = build_runtime(&writer_runtime_kind, &repo, docker_image.as_deref(), wsl_distro.as_deref())?;
            let review_runtime = build_runtime(&review_runtime_kind, &repo, docker_image.as_deref(), wsl_distro.as_deref())?;
            if !writer_runtime.probe().await { return Err(format!("writer runtime `{writer_runtime_kind}` is unavailable")); }
            if !review_runtime.probe().await { return Err(format!("review runtime `{review_runtime_kind}` is unavailable")); }

            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = FleetEngine::with_role_runtimes(
                writer_runtime.clone(), review_runtime, audit.clone(),
                FleetEngineConfig {
                    repo_root: repo.clone(), worktree_root, base_ref: base_ref.clone(), routing_profile,
                    candidates: candidates.clone(), allowed_env: allow_env.clone(), timeout_ms,
                    verification: verification.clone(), max_repairs,
                },
            );
            let summary = engine.run(goal.clone()).await?;
            println!("OPEN AUTOPILOT fleet run\nfinal_state={:?}\nrepairs={}\nrole_runs={}\nevidence={}\naudit_events={}", summary.final_state, summary.repairs, summary.role_runs.len(), summary.evidence.len(), audit.list_by_correlation(summary.correlation_id).await?.len());
            let worktree = summary.worktree.clone();
            if let Some(path) = &worktree { println!("worktree={}", path.display()); }
            for run in &summary.role_runs { println!("role:{:?}:provider={}:model={}:backend={:?}:success={}", run.role, run.provider, if run.model.is_empty() { "<default>" } else { &run.model }, run.backend, run.success); }
            for item in &summary.evidence { println!("evidence:{}:{}:{}", item.kind, item.passed, item.summary); }

            if !publish { return Ok(()); }
            if summary.final_state != autopilot_domain::GoalState::Completed { return Err("refusing publish because local fleet verification did not complete successfully".into()); }
            let worktree = worktree.ok_or_else(|| "verified fleet result has no worktree".to_string())?;
            let branch = current_branch(writer_runtime.clone(), &worktree).await?;
            let delivery = DeliveryEngine::new(writer_runtime.clone(), DeliveryConfig {
                workdir: worktree.clone(), remote, branch, base: base_ref,
                title: format!("OPEN AUTOPILOT: {}", goal.chars().take(72).collect::<String>()),
                body: "Automated change produced by OPEN AUTOPILOT. Local fleet verification and independent review completed before publication.".into(),
            }).map_err(|error| error.to_string())?;
            let pr = delivery.publish_verified_change("feat: OPEN AUTOPILOT verified change").await.map_err(|error| error.to_string())?;
            println!("pull_request={}", pr.url);

            let mut ci_repairs = 0usize;
            loop {
                let report = wait_for_ci(&delivery, ci_poll_attempts, ci_poll_interval_ms).await?;
                println!("ci_state={:?}", report.state);
                match report.state {
                    CiState::Passing => { println!("delivery=verified"); break; }
                    CiState::Pending => return Err("CI remained pending after the configured polling budget".into()),
                    CiState::Unknown => return Err("CI state is unknown; refusing to claim delivery success".into()),
                    CiState::Failing if ci_repairs < max_ci_repairs => {
                        ci_repairs += 1;
                        println!("ci_repair_attempt={ci_repairs}");
                        repair_failing_ci(writer_runtime.clone(), &worktree, &goal, &candidates, routing_profile, &allow_env, timeout_ms, &report).await?;
                        if !run_acceptance(writer_runtime.clone(), &worktree, &verification).await? { return Err("CI Debugger changed the worktree but local acceptance verification still fails".into()); }
                        delivery.prepare_commit(&format!("fix: repair CI attempt {ci_repairs}")).await.map_err(|error| error.to_string())?;
                        delivery.push_branch().await.map_err(|error| error.to_string())?;
                    }
                    CiState::Failing => return Err(format!("CI remains failing after {max_ci_repairs} repair attempts")),
                }
            }
        }
    }
    Ok(())
}
