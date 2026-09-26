use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use clap::{Parser, Subcommand};
use chrono::Utc;
use autopilot_audit::{AuditStore, InMemoryAuditStore};
use autopilot_automation::{AutomationEvent, AutomationSpec, Trigger};
use autopilot_automation_runtime::AutomationRuntime;
use autopilot_control_plane::{ApprovalRequest, ControlPlaneStore};
use autopilot_capability_graph::{CapabilityQuery, RepositoryCatalog};
use autopilot_core::{AgentExecutor, AgentOutcome, Director, DirectorConfig, GoalRequest, PlannedTask, Planner, VerificationOutcome, Verifier};
use autopilot_delivery::{CiReport, CiState, DeliveryConfig, DeliveryEngine};
use autopilot_domain::{AgentRole, Capability, Evidence, RiskClass};
use autopilot_engine::{backend_for_provider, AutonomousEngine, EngineConfig, FleetEngine, FleetEngineConfig, GraphGoalEngine, PluginGoalEngine, PluginGoalStep, VerificationCommand};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::PolicyEngine;
use autopilot_plugins::{parse_manifest_json, PluginRegistry};
use autopilot_plugin_builder::{BuildDiscoveryChain, BuildRequest, CandidateStore, FilesystemMcpDiscovery, FilesystemOpenApiDiscovery, PluginBuilder, SandboxQualifier, SelfBuildCoordinator, SelfBuildOutcome, SelfBuildPolicy, SemanticGapDetector};
use autopilot_plugin_runtime::{PluginCall, PluginExecutor, PluginRuntimeConfig};
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};
use autopilot_runtime::{CommandSpec, DockerRuntime, ExecutionProfile, NativeRuntime, RuntimeAdapter, WslRuntime};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "open-autopilot", version, about = "OPEN AUTOPILOT local autonomous engineering harness")]
struct Cli { #[command(subcommand)] command: Commands }

#[derive(Subcommand)]
enum Commands {
    Demo { #[arg(long)] goal: String },
    Backends,
    Plugins,
    CapabilitySearch {
        #[arg(long)] capability: Option<String>,
        #[arg(long = "keyword")] keywords: Vec<String>,
        #[arg(long)] transport: Option<String>,
        #[arg(long, default_value_t = 20)] limit: usize,
    },
    CapabilityPlan {
        #[arg(long)] goal: String,
        #[arg(long, default_value_t = 8)] max_routes: usize,
    },
    SemanticGap {
        #[arg(long)] goal: String,
    },
    PluginBuild {
        #[arg(long)] request: PathBuf,
        #[arg(long)] quarantine: PathBuf,
    },
    PluginQualify {
        #[arg(long)] id: String,
        #[arg(long)] quarantine: PathBuf,
        #[arg(long)] workspace: PathBuf,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long = "allow-program")] allow_program: Vec<String>,
    },
    PluginPromote {
        #[arg(long)] id: String,
        #[arg(long)] quarantine: PathBuf,
        #[arg(long)] generated_plugins: PathBuf,
    },
    GraphGoal {
        #[arg(long)] goal: String,
        #[arg(long, default_value_t = 3)] max_routes: usize,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    UniversalGoal {
        #[arg(long)] goal: String,
        #[arg(long)] discovery_root: PathBuf,
        #[arg(long)] base_url: Option<String>,
        #[arg(long, default_value = ".autopilot/quarantine")] quarantine: PathBuf,
        #[arg(long, default_value = ".autopilot/generated-plugins")] generated_plugins: PathBuf,
        #[arg(long, default_value_t = 3)] max_routes: usize,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    AutomationAdd {
        #[arg(long)] db: PathBuf,
        #[arg(long)] name: String,
        #[arg(long)] goal: String,
        #[arg(long)] trigger_json: String,
    },
    AutomationTick {
        #[arg(long)] db: PathBuf,
    },
    AutomationEvent {
        #[arg(long)] db: PathBuf,
        #[arg(long)] topic: String,
        #[arg(long, default_value = "{}")] payload_json: String,
    },
    ApprovalRequest {
        #[arg(long)] db: PathBuf,
        #[arg(long)] subject: String,
        #[arg(long, default_value = "{}")] action_json: String,
    },
    ApprovalDecide {
        #[arg(long)] db: PathBuf,
        #[arg(long)] id: String,
        #[arg(long)] approve: bool,
        #[arg(long)] actor: String,
        #[arg(long)] reason: Option<String>,
    },
    BudgetSet {
        #[arg(long)] db: PathBuf,
        #[arg(long)] id: String,
        #[arg(long)] limit_microusd: i64,
    },
    BudgetReserve {
        #[arg(long)] db: PathBuf,
        #[arg(long)] id: String,
        #[arg(long)] amount_microusd: i64,
    },
    PluginCall {
        #[arg(long)] plugin: String,
        #[arg(long)] capability: String,
        #[arg(long, default_value = "{}")] input_json: String,
        #[arg(long, default_value_t = false)] approve: bool,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    PluginGoal {
        #[arg(long)] goal: String,
        #[arg(long)] plan: PathBuf,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    E2eSales {
        #[arg(long)] lead_url: String,
        #[arg(long)] lead_title: String,
        #[arg(long)] product: String,
        #[arg(long, default_value = "business owners")] audience: String,
        #[arg(long)] workspace: PathBuf,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long, default_value_t = false)] use_remote: bool,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
    },
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

fn initial_plugin_registry() -> Result<PluginRegistry, String> {
    let mut registry = PluginRegistry::default();
    for raw in [
        include_str!("../../../plugins/local-computer/plugin.json"),
        include_str!("../../../plugins/remote-computer/plugin.json"),
        include_str!("../../../plugins/business-runtime/plugin.json"),
        include_str!("../../../plugins/nexus/plugin.json"),
        include_str!("../../../plugins/agent-fabric/plugin.json"),
        include_str!("../../../plugins/auto-company/plugin.json"),
        include_str!("../../../plugins/browser/plugin.json"),
        include_str!("../../../plugins/connector-gateway/plugin.json"),
        include_str!("../../../plugins/email/plugin.json"),
        include_str!("../../../plugins/calendar/plugin.json"),
        include_str!("../../../plugins/crm/plugin.json"),
    ] {
        let manifest = parse_manifest_json(raw).map_err(|error| error.to_string())?;
        registry.register(manifest).map_err(|error| error.to_string())?;
    }
    if let Ok(root) = std::env::var("AUTOPILOT_GENERATED_PLUGINS") {
        registry.load_dir(PathBuf::from(root)).map_err(|error| error.to_string())?;
    }
    registry.validate_dependencies().map_err(|error| error.to_string())?;
    Ok(registry)
}

fn e2e_sales_steps(
    lead_url: &str,
    lead_title: &str,
    product: &str,
    audience: &str,
    use_remote: bool,
) -> Vec<PluginGoalStep> {
    let mut steps = vec![
        PluginGoalStep {
            id: Some("memory".into()),
            role: AgentRole::Custom("Memory".into()),
            plugin_id: "intelligence.nexus".into(),
            capability: "memory.search".into(),
            input: serde_json::json!({"query":format!("{lead_title} {product}"),"limit":10}),
            approved: false,
            risk: RiskClass::Low,
        },
        PluginGoalStep {
            id: Some("browser".into()),
            role: AgentRole::Custom("Researcher".into()),
            plugin_id: "browser.local.playwright".into(),
            capability: "browser.run_plan".into(),
            input: serde_json::json!({
                "actions":[
                    {"action":"navigate","url":lead_url},
                    {"action":"snapshot"}
                ],
                "headless":true,
                "timeout_ms":30000
            }),
            approved: false,
            risk: RiskClass::Medium,
        },
        PluginGoalStep {
            id: Some("opportunity".into()),
            role: AgentRole::Custom("Revenue".into()),
            plugin_id: "business.auto-company".into(),
            capability: "company.opportunity.ingest".into(),
            input: serde_json::json!({
                "title":lead_title,
                "source":"autopilot-browser",
                "description":{"$ref":"steps.1.output.results.1.text"},
                "url":lead_url,
                "expected_revenue_usd":0.0,
                "estimated_cost_usd":0.0,
                "probability":0.0,
                "hours":0.0,
                "evidence":["public browser snapshot"],
                "tags":["autopilot","e2e-sales"]
            }),
            approved: false,
            risk: RiskClass::Low,
        },
        PluginGoalStep {
            id: Some("sales".into()),
            role: AgentRole::Custom("Sales".into()),
            plugin_id: "business.capability.runtime".into(),
            capability: "business.sales.plan".into(),
            input: serde_json::json!({
                "message":{"$ref":"steps.1.output.results.1.text"},
                "current_score":0
            }),
            approved: false,
            risk: RiskClass::Low,
        },
        PluginGoalStep {
            id: Some("smm".into()),
            role: AgentRole::Custom("SMM".into()),
            plugin_id: "business.capability.runtime".into(),
            capability: "business.smm.plan".into(),
            input: serde_json::json!({
                "product":product,
                "audience":audience,
                "objective":"qualified leads",
                "days":7,
                "platforms":["telegram","vk","instagram"]
            }),
            approved: false,
            risk: RiskClass::Low,
        },
        PluginGoalStep {
            id: Some("artifact".into()),
            role: AgentRole::Custom("Operator".into()),
            plugin_id: "computer.local.commander".into(),
            capability: "computer.write_file".into(),
            input: serde_json::json!({
                "path":"autopilot/e2e-sales/lead_snapshot.txt",
                "content":{"$ref":"steps.1.output.results.1.text"},
                "mode":"rewrite"
            }),
            approved: false,
            risk: RiskClass::Medium,
        },
        PluginGoalStep {
            id: Some("worker".into()),
            role: AgentRole::Custom("Scheduler".into()),
            plugin_id: "workers.agent.fabric".into(),
            capability: "worker.schedule".into(),
            input: serde_json::json!({
                "agents":[
                    {"id":"local-sales-worker","role":"reviewer","cpu_millis":1000,"memory_mib":1024,"tools":["read_file"],"remote":false},
                    {"id":"remote-sales-worker","role":"reviewer","cpu_millis":2000,"memory_mib":2048,"tools":["read_file"],"remote":true}
                ],
                "task":{
                    "id":"e2e-sales-review",
                    "goal":"Review the generated lead research artifact and prepare a verified handoff",
                    "required_role":"reviewer",
                    "required_tools":["read_file"],
                    "cpu_millis":500,
                    "memory_mib":512,
                    "allow_remote":use_remote,
                    "epoch":1
                }
            }),
            approved: false,
            risk: RiskClass::Medium,
        },
    ];

    if use_remote {
        steps.push(PluginGoalStep {
            id: Some("remote".into()),
            role: AgentRole::Custom("RemoteOperator".into()),
            plugin_id: "computer.remote.shttps".into(),
            capability: "computer.remote.list_dir".into(),
            input: serde_json::json!({"path":"."}),
            approved: false,
            risk: RiskClass::Low,
        });
    }

    let mut evidence = serde_json::Map::new();
    evidence.insert("memory".into(), serde_json::json!({"$ref":"steps.0.output"}));
    evidence.insert("browser".into(), serde_json::json!({"$ref":"steps.1.output"}));
    evidence.insert("opportunity".into(), serde_json::json!({"$ref":"steps.2.output"}));
    evidence.insert("sales".into(), serde_json::json!({"$ref":"steps.3.output"}));
    evidence.insert("smm".into(), serde_json::json!({"$ref":"steps.4.output"}));
    evidence.insert("artifact".into(), serde_json::json!({"$ref":"steps.5.output"}));
    evidence.insert("worker".into(), serde_json::json!({"$ref":"steps.6.output"}));
    let mut required = vec!["memory","browser","opportunity","sales","smm","artifact","worker"];
    if use_remote {
        evidence.insert("remote".into(), serde_json::json!({"$ref":"steps.7.output"}));
        required.push("remote");
    }
    steps.push(PluginGoalStep {
        id: Some("verify".into()),
        role: AgentRole::Custom("Verifier".into()),
        plugin_id: "business.capability.runtime".into(),
        capability: "business.verify.evidence".into(),
        input: serde_json::json!({"required":required,"evidence":evidence}),
        approved: false,
        risk: RiskClass::Low,
    });
    steps
}

fn initial_capability_graph() -> Result<autopilot_capability_graph::CapabilityGraph, String> {
    let catalog: RepositoryCatalog = serde_json::from_str(include_str!("../../../catalog/repositories.json"))
        .map_err(|error| format!("invalid repository capability catalog: {error}"))?;
    let mut graph = catalog.into_graph().map_err(|error| error.to_string())?;
    let registry = initial_plugin_registry()?;
    graph.attach_plugins(&registry);
    Ok(graph)
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

fn build_plugin_runtime(
    kind: &str,
    workspace: Option<&Path>,
    docker_image: Option<&str>,
    wsl_distro: Option<&str>,
) -> Result<Arc<dyn RuntimeAdapter>, String> {
    let fallback = std::env::current_dir().map_err(|error| error.to_string())?;
    let root = workspace.unwrap_or(&fallback);
    build_runtime(kind, root, docker_image, wsl_distro)
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
        Commands::Plugins => {
            let registry = initial_plugin_registry()?;
            for plugin in registry.list() {
                println!("{}\t{}\t{}\t{:?}", plugin.id, plugin.version, plugin.name, plugin.kind);
                for capability in &plugin.capabilities {
                    println!("  {}\t{:?}\tapproval={}", capability.name, capability.risk, capability.requires_approval);
                }
            }
        },
        Commands::CapabilitySearch { capability, keywords, transport, limit } => {
            let graph = initial_capability_graph()?;
            let hits = graph.search(&CapabilityQuery {
                capability,
                keywords,
                require_transport: transport,
                include_archived: false,
            });
            if hits.is_empty() {
                println!("no capability providers matched");
            } else {
                for hit in hits.into_iter().take(limit.max(1)) {
                    println!(
                        "{}\tscore={}\tcapabilities={}\ttags={}\ttransports={}",
                        hit.repository,
                        hit.score,
                        hit.matched_capabilities.into_iter().collect::<Vec<_>>().join(","),
                        hit.matched_tags.into_iter().collect::<Vec<_>>().join(","),
                        hit.transports.into_iter().collect::<Vec<_>>().join(","),
                    );
                }
            }
        },
        Commands::CapabilityPlan { goal, max_routes } => {
            let graph = initial_capability_graph()?;
            let routes = graph.route_goal(&goal, max_routes);
            if routes.is_empty() {
                println!("no executable capability routes matched");
            } else {
                for route in routes {
                    println!(
                        "capability={}\tprovider={}\trepository={}\tscore={}\tmatched={}",
                        route.capability,
                        route.provider,
                        route.repository.unwrap_or_else(|| "-".into()),
                        route.score,
                        route.matched_terms.into_iter().collect::<Vec<_>>().join(","),
                    );
                }
            }
        },
        Commands::SemanticGap { goal } => {
            let graph = initial_capability_graph()?;
            match SemanticGapDetector::self_build_plan(&graph, &goal) {
                Some(plan) => println!("{}", serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?),
                None => println!("no semantic gap: executable provider already matches this goal"),
            }
        },
        Commands::PluginBuild { request, quarantine } => {
            let raw = fs::read_to_string(&request)
                .map_err(|error| format!("cannot read build request {}: {error}", request.display()))?;
            let build: BuildRequest = serde_json::from_str(&raw)
                .map_err(|error| format!("invalid build request {}: {error}", request.display()))?;
            let candidate = PluginBuilder::build(build).map_err(|error| error.to_string())?;
            let store = CandidateStore::new(quarantine);
            store.save(&candidate).map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&candidate).map_err(|error| error.to_string())?);
        },
        Commands::PluginQualify { id, quarantine, workspace, runtime, docker_image, allow_program } => {
            let id = Uuid::parse_str(&id).map_err(|error| format!("invalid candidate id: {error}"))?;
            let runtime = build_runtime(&runtime, &workspace, docker_image.as_deref(), None)?;
            let allowed = allow_program.into_iter().collect::<BTreeSet<_>>();
            let qualifier = SandboxQualifier::new(runtime, allowed);
            let store = CandidateStore::new(quarantine);
            let candidate = store.qualify(id, &qualifier).await.map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&candidate).map_err(|error| error.to_string())?);
        },
        Commands::PluginPromote { id, quarantine, generated_plugins } => {
            let id = Uuid::parse_str(&id).map_err(|error| format!("invalid candidate id: {error}"))?;
            let store = CandidateStore::new(quarantine);
            let path = store.promote(id, generated_plugins).map_err(|error| error.to_string())?;
            println!("promoted_manifest={}", path.display());
            println!("Set AUTOPILOT_GENERATED_PLUGINS to the generated plugin root to load promoted manifests.");
        },
        Commands::GraphGoal { goal, max_routes, workspace, control_db, allow_env, allow_generated_program, runtime, docker_image, wsl_distro } => {
            let graph = Arc::new(initial_capability_graph()?);
            let registry = initial_plugin_registry()?;
            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;
            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = workspace;
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.iter().cloned().collect();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("business.capability.runtime".into(), "AUTOPILOT_API_TOKEN".into());
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            config.bearer_token_env.insert("business.auto-company".into(), "SYNERGY_API_TOKEN".into());
            config.bearer_token_env.insert("connector.gateway".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.email".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.calendar".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.crm".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            for plugin_id in registry.list().map(|manifest| manifest.id.clone()).collect::<Vec<_>>() {
                if !allow_env.is_empty() { config.env_allowlist.insert(plugin_id, allow_env.clone()); }
            }
            let executor = Arc::new(PluginExecutor::new(
                registry,
                PolicyEngine::default(),
                execution_runtime,
                config,
            ));
            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = GraphGoalEngine::new(graph, audit.clone(), executor, max_routes);
            let summary = engine.run(goal).await.map_err(|error| error.to_string())?;
            println!(
                "final_state={:?}\tevidence={}\taudit_events={}\tblocked_by_policy={}",
                summary.final_state,
                summary.evidence.len(),
                audit.list_by_correlation(summary.correlation_id).await?.len(),
                summary.blocked_by_policy,
            );
        },
        Commands::UniversalGoal {
            goal, discovery_root, base_url, quarantine, generated_plugins,
            max_routes, workspace, control_db, allow_env, allow_generated_program,
            runtime, docker_image, wsl_distro
        } => {
            let mut graph = initial_capability_graph()?;
            let mut registry = initial_plugin_registry()?;
            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;

            if SemanticGapDetector::detect(&graph, &goal).is_some() {
                let allowed_programs = allow_generated_program.iter().cloned().collect::<BTreeSet<_>>();
                let discovery = BuildDiscoveryChain::new(vec![
                    Arc::new(FilesystemOpenApiDiscovery::new(discovery_root.clone(), base_url)),
                    Arc::new(FilesystemMcpDiscovery::new(
                        discovery_root,
                        execution_runtime.clone(),
                        allowed_programs.clone(),
                    )),
                ]);
                let qualifier = SandboxQualifier::new(
                    execution_runtime.clone(),
                    allowed_programs,
                );
                let coordinator = SelfBuildCoordinator::new(
                    CandidateStore::new(quarantine),
                    qualifier,
                    discovery,
                    SelfBuildPolicy::default(),
                    generated_plugins.clone(),
                );
                let outcome = coordinator.ensure_for_goal(&graph, &goal).await
                    .map_err(|error| error.to_string())?;
                println!(
                    "self_build_outcome={}\n",
                    serde_json::to_string_pretty(&outcome).map_err(|error| error.to_string())?
                );

                match outcome {
                    SelfBuildOutcome::Promoted { manifest_path, .. } => {
                        let raw = fs::read_to_string(&manifest_path)
                            .map_err(|error| format!("cannot read promoted manifest {}: {error}", manifest_path.display()))?;
                        let manifest = parse_manifest_json(&raw).map_err(|error| error.to_string())?;
                        registry.upsert(manifest).map_err(|error| error.to_string())?;
                        registry.validate_dependencies().map_err(|error| error.to_string())?;
                        graph.attach_plugins(&registry);
                    }
                    SelfBuildOutcome::ExistingProvider => {}
                    SelfBuildOutcome::DiscoveryEmpty { .. }
                    | SelfBuildOutcome::CandidateRejected { .. }
                    | SelfBuildOutcome::QualifiedAwaitingPromotion { .. } => {
                        return Err("self-build did not yield an auto-promoted executable provider".into());
                    }
                }
            }

            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = workspace;
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.iter().cloned().collect();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("business.capability.runtime".into(), "AUTOPILOT_API_TOKEN".into());
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            config.bearer_token_env.insert("business.auto-company".into(), "SYNERGY_API_TOKEN".into());
            config.bearer_token_env.insert("connector.gateway".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.email".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.calendar".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.crm".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            for plugin_id in registry.list().map(|manifest| manifest.id.clone()).collect::<Vec<_>>() {
                if !allow_env.is_empty() {
                    config.env_allowlist.insert(plugin_id, allow_env.clone());
                }
            }

            let executor = Arc::new(PluginExecutor::new(
                registry,
                PolicyEngine::default(),
                execution_runtime,
                config,
            ));
            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = GraphGoalEngine::new(Arc::new(graph), audit.clone(), executor, max_routes);
            let summary = engine.run(goal).await.map_err(|error| error.to_string())?;
            println!(
                "UNIVERSAL_GOAL\nfinal_state={:?}\nevidence={}\naudit_events={}\nblocked_by_policy={}",
                summary.final_state,
                summary.evidence.len(),
                audit.list_by_correlation(summary.correlation_id).await?.len(),
                summary.blocked_by_policy,
            );
        },
        Commands::AutomationAdd { db, name, goal, trigger_json } => {
            let trigger: Trigger = serde_json::from_str(&trigger_json)
                .map_err(|error| format!("invalid --trigger-json: {error}"))?;
            let spec = AutomationSpec::new(name, goal, trigger);
            let runtime = AutomationRuntime::open(&db).map_err(|error| error.to_string())?;
            runtime.control().put_automation(&spec).map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&spec).map_err(|error| error.to_string())?);
        },
        Commands::AutomationTick { db } => {
            let runtime = AutomationRuntime::open(&db).map_err(|error| error.to_string())?;
            let dispatched = runtime.schedule_due(Utc::now()).map_err(|error| error.to_string())?;
            println!("dispatched={}", dispatched.len());
            for item in dispatched {
                println!("automation={}\tmission={}\tgoal={}", item.automation_id, item.mission_id.0, item.goal);
            }
        },
        Commands::AutomationEvent { db, topic, payload_json } => {
            let value: serde_json::Value = serde_json::from_str(&payload_json)
                .map_err(|error| format!("invalid --payload-json: {error}"))?;
            let object = value.as_object().ok_or_else(|| "--payload-json must be a JSON object".to_string())?;
            let payload = object.iter().map(|(key,value)|(key.clone(),value.clone())).collect();
            let runtime = AutomationRuntime::open(&db).map_err(|error| error.to_string())?;
            let (event_id, dispatched) = runtime.emit_event(AutomationEvent::new(topic, payload))
                .map_err(|error| error.to_string())?;
            println!("event={}\tdispatched={}", event_id, dispatched.len());
            for item in dispatched {
                println!("automation={}\tmission={}\tgoal={}", item.automation_id, item.mission_id.0, item.goal);
            }
        },
        Commands::ApprovalRequest { db, subject, action_json } => {
            let action = serde_json::from_str(&action_json)
                .map_err(|error| format!("invalid --action-json: {error}"))?;
            let store = ControlPlaneStore::open(&db).map_err(|error| error.to_string())?;
            let request = ApprovalRequest::pending(subject, action);
            store.request_approval(&request).map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&request).map_err(|error| error.to_string())?);
        },
        Commands::ApprovalDecide { db, id, approve, actor, reason } => {
            let id = Uuid::parse_str(&id).map_err(|error| format!("invalid approval id: {error}"))?;
            let store = ControlPlaneStore::open(&db).map_err(|error| error.to_string())?;
            let request = store.decide_approval(id, approve, &actor, reason.as_deref())
                .map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&request).map_err(|error| error.to_string())?);
        },
        Commands::BudgetSet { db, id, limit_microusd } => {
            let store = ControlPlaneStore::open(&db).map_err(|error| error.to_string())?;
            store.set_daily_budget(&id, Utc::now().date_naive(), limit_microusd)
                .map_err(|error| error.to_string())?;
            println!("budget_set={}\tlimit_microusd={}", id, limit_microusd.max(0));
        },
        Commands::BudgetReserve { db, id, amount_microusd } => {
            let store = ControlPlaneStore::open(&db).map_err(|error| error.to_string())?;
            let remaining = store.reserve_budget(&id, Utc::now().date_naive(), amount_microusd)
                .map_err(|error| error.to_string())?;
            println!("budget_reserved={}\tamount_microusd={}\tremaining_microusd={}", id, amount_microusd.max(0), remaining);
        },
        Commands::PluginCall { plugin, capability, input_json, approve, workspace, control_db, allow_env, allow_generated_program, runtime, docker_image, wsl_distro } => {
            let registry = initial_plugin_registry()?;
            let input = serde_json::from_str(&input_json).map_err(|error| format!("invalid --input-json: {error}"))?;
            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;
            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = workspace;
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.iter().cloned().collect();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("business.capability.runtime".into(), "AUTOPILOT_API_TOKEN".into());
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            config.bearer_token_env.insert("business.auto-company".into(), "SYNERGY_API_TOKEN".into());
            config.bearer_token_env.insert("connector.gateway".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.email".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.calendar".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.crm".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            if !allow_env.is_empty() { config.env_allowlist.insert(plugin.clone(), allow_env); }
            let executor = PluginExecutor::new(
                registry,
                PolicyEngine::default(),
                execution_runtime,
                config,
            );
            let result = executor.execute(PluginCall {
                plugin_id: plugin,
                capability,
                input,
                approved: approve,
            }).await.map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?);
        },
        Commands::PluginGoal { goal, plan, workspace, control_db, allow_env, allow_generated_program, runtime, docker_image, wsl_distro } => {
            let registry = initial_plugin_registry()?;
            let raw = fs::read_to_string(&plan).map_err(|error| format!("cannot read plan {}: {error}", plan.display()))?;
            let steps: Vec<PluginGoalStep> = serde_json::from_str(&raw)
                .map_err(|error| format!("invalid plugin goal plan {}: {error}", plan.display()))?;
            if steps.is_empty() { return Err("plugin goal plan must contain at least one step".into()); }

            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;
            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = workspace;
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.iter().cloned().collect();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("business.capability.runtime".into(), "AUTOPILOT_API_TOKEN".into());
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            config.bearer_token_env.insert("business.auto-company".into(), "SYNERGY_API_TOKEN".into());
            config.bearer_token_env.insert("connector.gateway".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.email".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.calendar".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.crm".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            for plugin_id in registry.list().map(|manifest| manifest.id.clone()).collect::<Vec<_>>() {
                if !allow_env.is_empty() { config.env_allowlist.insert(plugin_id, allow_env.clone()); }
            }

            let executor = Arc::new(PluginExecutor::new(
                registry,
                PolicyEngine::default(),
                execution_runtime,
                config,
            ));
            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = PluginGoalEngine::new(audit.clone(), executor, steps, 0);
            let summary = engine.run(goal).await.map_err(|error| error.to_string())?;
            println!(
                "final_state={:?}\nevidence={}\naudit_events={}\nblocked_by_policy={}",
                summary.final_state,
                summary.evidence.len(),
                audit.list_by_correlation(summary.correlation_id).await?.len(),
                summary.blocked_by_policy
            );
        },
        Commands::E2eSales { lead_url, lead_title, product, audience, workspace, control_db, use_remote, allow_env, allow_generated_program } => {
            let registry = initial_plugin_registry()?;
            let steps = e2e_sales_steps(&lead_url, &lead_title, &product, &audience, use_remote);
            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = Some(workspace);
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.iter().cloned().collect();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("business.capability.runtime".into(), "AUTOPILOT_API_TOKEN".into());
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            config.bearer_token_env.insert("business.auto-company".into(), "SYNERGY_API_TOKEN".into());
            config.bearer_token_env.insert("connector.gateway".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.email".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.calendar".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            config.bearer_token_env.insert("capability.crm".into(), "CONNECTOR_GATEWAY_TOKEN".into());
            for plugin_id in registry.list().map(|manifest| manifest.id.clone()).collect::<Vec<_>>() {
                if !allow_env.is_empty() { config.env_allowlist.insert(plugin_id, allow_env.clone()); }
            }
            let executor = Arc::new(PluginExecutor::new(
                registry,
                PolicyEngine::default(),
                Arc::new(NativeRuntime::default()),
                config,
            ));
            let audit = Arc::new(InMemoryAuditStore::default());
            let engine = PluginGoalEngine::new(audit.clone(), executor, steps, 0);
            let summary = engine.run(format!("Find and prepare sales opportunity: {lead_title} for {product}")).await
                .map_err(|error| error.to_string())?;
            println!(
                "SYNERGY AI AUTOPILOT E2E SALES\nfinal_state={:?}\nevidence={}\naudit_events={}\nblocked_by_policy={}\nremote={}",
                summary.final_state,
                summary.evidence.len(),
                audit.list_by_correlation(summary.correlation_id).await?.len(),
                summary.blocked_by_policy,
                use_remote
            );
            if summary.final_state != autopilot_domain::GoalState::Completed {
                return Err("E2E sales vertical slice did not complete".into());
            }
        },
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
