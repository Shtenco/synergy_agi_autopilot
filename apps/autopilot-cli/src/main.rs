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
use autopilot_engine::{backend_for_provider, AutonomousEngine, EngineConfig, FleetEngine, FleetEngineConfig, GraphGoalEngine, ParallelPluginWorkflowEngine, PluginGoalEngine, PluginGoalStep, VerificationCommand};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::PolicyEngine;
use autopilot_org::{Organization, OrganizationSpec};
use autopilot_outcomes::{fingerprint_goal, OutcomeStore};
use autopilot_workforce::{Initiative, WorkforceConfig, WorkforceExecutor, WorkforceNode, WorkforceStore, WorkItem};
use autopilot_plugins::{parse_manifest_json, PluginRegistry};
use autopilot_plugin_builder::{BuildDiscoveryChain, BuildRequest, CandidateStore, FilesystemCliDescriptorDiscovery, FilesystemMcpDiscovery, FilesystemOpenApiDiscovery, PluginBuilder, SandboxQualifier, SelfBuildCoordinator, SelfBuildOutcome, SelfBuildPolicy, SemanticGapDetector};
use autopilot_plugin_runtime::{PluginCall, PluginExecutor, PluginRuntimeConfig};
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};
use autopilot_runtime::{CommandSpec, DockerRuntime, ExecutionProfile, NativeRuntime, RuntimeAdapter, WslRuntime};
use autopilot_semantic_planner::{cognitive_planner_prompt, AdaptiveWorkflowSynthesizer, CognitivePlanProposal, MappingContext, SemanticArgumentMapper, VerifiedCognitivePlanner, WorkflowPlan};
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
    ArgumentMap {
        #[arg(long)] plugin: String,
        #[arg(long)] capability: String,
        #[arg(long)] goal: String,
        #[arg(long, default_value = "{}")] hints_json: String,
        #[arg(long, default_value = "null")] memory_json: String,
    },
    WorkflowPlan {
        #[arg(long)] goal: String,
        #[arg(long, default_value = "{}")] hints_json: String,
        #[arg(long, default_value = "null")] memory_json: String,
    },
    CognitivePlanVerify {
        #[arg(long)] proposal: PathBuf,
        #[arg(long, default_value = "{}")] hints_json: String,
        #[arg(long, default_value = "null")] memory_json: String,
    },
    CognitivePlan {
        #[arg(long)] goal: String,
        #[arg(long, default_value = "{}")] hints_json: String,
        #[arg(long, default_value = "null")] memory_json: String,
    },
    WorkflowRun {
        #[arg(long)] goal: String,
        #[arg(long, default_value = "{}")] hints_json: String,
        #[arg(long, default_value = "null")] memory_json: String,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "approve-step")] approve_step: Vec<String>,
        #[arg(long = "approval-id")] approval_id: Vec<String>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
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
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long, default_value = ".autopilot/outcomes.db")] outcome_db: PathBuf,
        #[arg(long, default_value_t = true)] parallel: bool,
    },
    OrgValidate {
        #[arg(long)] organization: Option<PathBuf>,
    },
    OrgSwarm {
        #[arg(long)] organization: Option<PathBuf>,
    },
    OrgRoute {
        #[arg(long)] capability: String,
        #[arg(long)] risk: String,
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long)] outcome_db: Option<PathBuf>,
        #[arg(long)] goal: Option<String>,
    },
    OutcomeBest {
        #[arg(long)] db: PathBuf,
        #[arg(long)] goal: String,
    },
    WorkforceInit {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] organization: Option<PathBuf>,
    },
    WorkforceStatus {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
    },
    WorkforceNodeRegister {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] node_id: String,
        #[arg(long)] kind: String,
        #[arg(long)] endpoint: String,
        #[arg(long = "capability")] capabilities: Vec<String>,
        #[arg(long, default_value_t = 1000)] cpu_millis: u64,
        #[arg(long, default_value_t = 1024)] memory_mib: u64,
    },
    WorkforceBindNode {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long)] node_id: String,
    },
    WorkforceEnqueue {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] department: String,
        #[arg(long)] created_by: String,
        #[arg(long)] goal: String,
        #[arg(long)] plugin: String,
        #[arg(long)] capability: String,
        #[arg(long, default_value = "{}")] input_json: String,
        #[arg(long, default_value = "low")] risk: String,
        #[arg(long)] employee: Option<String>,
        #[arg(long, default_value_t = 0)] priority: i64,
        #[arg(long)] budget_id: Option<String>,
        #[arg(long, default_value_t = 0)] budget_microusd: i64,
        #[arg(long)] approval_id: Option<String>,
        #[arg(long = "skill")] skills: Vec<String>,
        #[arg(long = "tool")] tools: Vec<String>,
    },
    WorkforceSkillSet {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long)] skill: String,
        #[arg(long)] level: f64,
        #[arg(long, default_value_t = 0)] evidence_count: u64,
    },
    WorkforceKpiSet {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long)] key: String,
        #[arg(long)] target: f64,
        #[arg(long)] current: f64,
        #[arg(long, default_value_t = 1.0)] weight: f64,
    },
    WorkforceInitiativeAdd {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long)] department: String,
        #[arg(long)] title: String,
        #[arg(long)] objective: String,
        #[arg(long, default_value_t = 0.5)] score: f64,
    },
    WorkforceInitiativeList {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] department: Option<String>,
    },
    WorkforceInitiativePromote {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] initiative_id: String,
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long, default_value = "config/workforce.default.json")] workforce_config: PathBuf,
    },
    WorkforceScheduleLink {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long)] automation_id: String,
        #[arg(long)] purpose: String,
    },
    WorkforceHeartbeat {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: Option<String>,
        #[arg(long)] node_id: Option<String>,
    },
    WorkforceRun {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] employee: String,
        #[arg(long, default_value = "local-workforce")] owner: String,
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long, default_value = "config/workforce.default.json")] workforce_config: PathBuf,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    WorkforceDepartmentCycle {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] department: String,
        #[arg(long, default_value = "department-cycle")] owner_prefix: String,
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long, default_value = "config/workforce.default.json")] workforce_config: PathBuf,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long)] control_db: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
        #[arg(long = "allow-generated-program")] allow_generated_program: Vec<String>,
        #[arg(long, default_value = "native")] runtime: String,
        #[arg(long)] docker_image: Option<String>,
        #[arg(long)] wsl_distro: Option<String>,
    },
    WorkforceMaintain {
        #[arg(long, default_value = ".autopilot/workforce.db")] db: PathBuf,
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long, default_value = "config/workforce.default.json")] workforce_config: PathBuf,
    },
    WorkforceSyncSwarm {
        #[arg(long)] organization: Option<PathBuf>,
        #[arg(long)] workspace: Option<PathBuf>,
        #[arg(long = "allow-env")] allow_env: Vec<String>,
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

fn load_organization(path: Option<&Path>) -> Result<Organization, String> {
    let raw = match path {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| format!("cannot read organization {}: {error}", path.display()))?,
        None => include_str!("../../../config/organization.default.json").to_string(),
    };
    let spec: OrganizationSpec = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid organization JSON: {error}"))?;
    Organization::validate(spec).map_err(|error| error.to_string())
}

fn parse_risk_class(value: &str) -> Result<RiskClass, String> {
    match value.to_ascii_lowercase().as_str() {
        "low" => Ok(RiskClass::Low),
        "medium" => Ok(RiskClass::Medium),
        "high" => Ok(RiskClass::High),
        "critical" => Ok(RiskClass::Critical),
        other => Err(format!("invalid risk class: {other}")),
    }
}

fn assign_ai_employees(
    steps: &mut [PluginGoalStep],
    organization: &Organization,
    outcomes: Option<&OutcomeStore>,
    goal_fingerprint: &str,
) -> Result<(), String> {
    for step in steps {
        let eligible = organization.eligible_employees(&step.capability, step.risk);
        if eligible.is_empty() {
            eprintln!(
                "org_unassigned step={:?} capability={} risk={:?}; no eligible default AI employee",
                step.id, step.capability, step.risk
            );
            continue;
        }

        let candidate_keys = eligible.iter().map(|employee| {
            format!(
                "employee:{}|{}/{}",
                employee.id, step.plugin_id, step.capability
            )
        }).collect::<Vec<_>>();

        let selected_id = outcomes
            .map(|store| store.best_route_among(goal_fingerprint, &candidate_keys, 2))
            .transpose()
            .map_err(|error| error.to_string())?
            .flatten()
            .and_then(|best| {
                best.route_key.strip_prefix("employee:")
                    .and_then(|value| value.split_once('|').map(|(employee, _)| employee.to_string()))
            })
            .unwrap_or_else(|| eligible[0].id.clone());

        let employee = organization.employee(&selected_id)
            .ok_or_else(|| format!("outcome routing selected unknown employee: {selected_id}"))?;
        step.employee_id = Some(employee.id.clone());
        step.department_id = Some(employee.department_id.clone());
        step.role = AgentRole::Custom(format!(
            "{}::{}::{}",
            employee.department_id, employee.title, employee.id
        ));
    }
    Ok(())
}

fn open_outcome_store(path: &Path) -> Result<OutcomeStore, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create outcome directory {}: {error}", parent.display()))?;
    }
    OutcomeStore::open(path).map_err(|error| error.to_string())
}

fn load_workforce_config(path: &Path) -> Result<WorkforceConfig, String> {
    let raw = fs::read_to_string(path)
        .or_else(|_| {
            if path == Path::new("config/workforce.default.json") {
                Ok(include_str!("../../../config/workforce.default.json").to_string())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "workforce config not found"))
            }
        })
        .map_err(|error| format!("cannot read workforce config {}: {error}", path.display()))?;
    let config: WorkforceConfig = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid workforce config: {error}"))?;
    if config.schema != "synergy.workforce.v1" {
        return Err(format!("unsupported workforce schema: {}", config.schema));
    }
    Ok(config)
}

fn parse_skill_requirements(values: &[String]) -> Result<BTreeMap<String,f64>, String> {
    let mut out=BTreeMap::new();
    for value in values {
        let (skill,level)=value.split_once('=')
            .ok_or_else(||format!("--skill must use skill=level syntax: {value}"))?;
        let level:f64=level.parse().map_err(|_|format!("invalid skill level: {value}"))?;
        if !(0.0..=1.0).contains(&level) { return Err(format!("skill level must be 0..1: {value}")); }
        out.insert(skill.to_string(),level);
    }
    Ok(out)
}

fn configured_plugin_executor(
    workspace: Option<PathBuf>,
    control_db: Option<PathBuf>,
    allow_env: &[String],
    allow_generated_program: &[String],
    runtime: &str,
    docker_image: Option<&str>,
    wsl_distro: Option<&str>,
) -> Result<Arc<PluginExecutor>, String> {
    let registry=initial_plugin_registry()?;
    let execution_runtime=build_plugin_runtime(runtime,workspace.as_deref(),docker_image,wsl_distro)?;
    let mut config=PluginRuntimeConfig::default();
    config.local_workspace=workspace;
    config.control_plane_db=control_db;
    config.generated_program_allowlist=allow_generated_program.iter().cloned().collect();
    config.timeout_ms=Some(120_000);
    config.bearer_token_env.insert("business.capability.runtime".into(),"AUTOPILOT_API_TOKEN".into());
    config.bearer_token_env.insert("intelligence.nexus".into(),"NEXUS_BRIDGE_TOKEN".into());
    config.bearer_token_env.insert("business.auto-company".into(),"SYNERGY_API_TOKEN".into());
    config.bearer_token_env.insert("connector.gateway".into(),"CONNECTOR_GATEWAY_TOKEN".into());
    config.bearer_token_env.insert("capability.email".into(),"CONNECTOR_GATEWAY_TOKEN".into());
    config.bearer_token_env.insert("capability.calendar".into(),"CONNECTOR_GATEWAY_TOKEN".into());
    config.bearer_token_env.insert("capability.crm".into(),"CONNECTOR_GATEWAY_TOKEN".into());
    for plugin_id in registry.list().map(|manifest|manifest.id.clone()).collect::<Vec<_>>() {
        if !allow_env.is_empty() { config.env_allowlist.insert(plugin_id,allow_env.to_vec()); }
    }
    Ok(Arc::new(PluginExecutor::new(registry,PolicyEngine::default(),execution_runtime,config)))
}

fn cognitive_prompt_with_outcomes(
    goal: &str,
    registry: &PluginRegistry,
    outcomes: &OutcomeStore,
    goal_fingerprint: &str,
) -> Result<String, String> {
    let mut prompt = cognitive_planner_prompt(goal, registry);
    let mut history = outcomes.stats(goal_fingerprint)
        .map_err(|error| error.to_string())?;
    history.sort_by(|a,b| b.score.total_cmp(&a.score).then_with(|| a.route_key.cmp(&b.route_key)));
    history.truncate(20);
    prompt.push_str("\n\nVERIFIED_OUTCOME_HISTORY\n");
    prompt.push_str(&serde_json::to_string(&history).map_err(|error| error.to_string())?);
    prompt.push_str("\nTreat history as advisory performance evidence only. Never bypass registry, risk, approval, contract, transform or policy verification.");
    Ok(prompt)
}


fn semantic_context_from_json(
    goal: &str,
    hints_json: &str,
    memory_json: &str,
) -> Result<MappingContext, String> {
    let hints_value: serde_json::Value = serde_json::from_str(hints_json)
        .map_err(|error| format!("invalid --hints-json: {error}"))?;
    let hints = hints_value.as_object()
        .ok_or_else(|| "--hints-json must be a JSON object".to_string())?
        .iter().map(|(key,value)|(key.clone(),value.clone())).collect();
    let memory = serde_json::from_str(memory_json)
        .map_err(|error| format!("invalid --memory-json: {error}"))?;
    Ok(MappingContext {
        goal: goal.to_string(),
        hints,
        memory,
        prior_outputs: Vec::new(),
    })
}

fn parse_cognitive_proposal_output(output: &serde_json::Value) -> Result<CognitivePlanProposal, String> {
    let proposal_value = output.get("result").cloned()
        .ok_or_else(|| "NEXUS cognitive planner returned no result".to_string())?;
    if let Some(text) = proposal_value.as_str() {
        serde_json::from_str(text)
            .map_err(|error| format!("NEXUS returned invalid cognitive plan JSON: {error}"))
    } else if let Some(text) = proposal_value.get("text").and_then(serde_json::Value::as_str) {
        serde_json::from_str(text)
            .map_err(|error| format!("NEXUS result.text is not a valid cognitive plan: {error}"))
    } else {
        serde_json::from_value(proposal_value)
            .map_err(|error| format!("NEXUS result does not match cognitive plan schema: {error}"))
    }
}

fn parse_step_value_map(values: &[String], flag: &str) -> Result<BTreeMap<String,String>, String> {
    let mut out = BTreeMap::new();
    for value in values {
        let (step, item) = value.split_once('=')
            .ok_or_else(|| format!("{flag} must use step-id=value syntax: {value}"))?;
        if step.trim().is_empty() || item.trim().is_empty() {
            return Err(format!("{flag} contains an empty step or value: {value}"));
        }
        out.insert(step.to_string(), item.to_string());
    }
    Ok(out)
}

fn workflow_to_plugin_steps(
    plan: &WorkflowPlan,
    approved_steps: &BTreeSet<String>,
    approval_ids: &BTreeMap<String,String>,
    require_persisted_approval: bool,
) -> Result<Vec<PluginGoalStep>, String> {
    let mut steps = Vec::new();
    let ordered_ids = if plan.execution_layers.is_empty() {
        plan.steps.iter().map(|step| step.id.clone()).collect::<Vec<_>>()
    } else {
        plan.execution_layers.iter().flatten().cloned().collect::<Vec<_>>()
    };
    for step_id in ordered_ids {
        let step = plan.steps.iter().find(|step| step.id == step_id)
            .ok_or_else(|| format!("execution layer references unknown step: {step_id}"))?;
        if step.requires_approval && !approved_steps.contains(&step.id) {
            return Err(format!(
                "workflow step {} requires approval; add --approve-step {}",
                step.id, step.id
            ));
        }
        if step.requires_approval
            && require_persisted_approval
            && !approval_ids.contains_key(&step.id)
        {
            return Err(format!(
                "workflow step {} requires persisted approval; add --approval-id {}=<uuid>",
                step.id, step.id
            ));
        }
        let typed_bindings = plan.typed_edges.iter()
            .filter(|edge| edge.to_step == step.id)
            .map(|edge| serde_json::json!({
                "from_step":edge.from_step,
                "from_path":edge.from_path,
                "to_field":edge.to_field,
                "compatibility":edge.compatibility,
                "score":edge.score,
                "transform":edge.transform
            }))
            .collect::<Vec<_>>();
        let mut input = serde_json::json!({
            "$semantic_contract": step.input_contract,
            "$semantic_goal": step.objective,
            "$semantic_hints": step.mapped_arguments,
            "$typed_bindings": typed_bindings,
        });
        if let Some(approval_id) = approval_ids.get(&step.id) {
            input["_approval_id"] = serde_json::Value::String(approval_id.clone());
        }
        steps.push(PluginGoalStep {
            id: Some(step.id.clone()),
            role: AgentRole::Custom("SemanticWorkflow".into()),
            plugin_id: step.plugin_id.clone(),
            capability: step.capability.clone(),
            input,
            approved: approved_steps.contains(&step.id),
            risk: step.risk,
        
            employee_id: None,
        department_id: None,
        });
    }
    Ok(steps)
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
        
            employee_id: None,
        department_id: None,
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
    
        employee_id: None,
        department_id: None,
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
        Commands::ArgumentMap { plugin, capability, goal, hints_json, memory_json } => {
            let registry = initial_plugin_registry()?;
            let manifest = registry.get(&plugin)
                .ok_or_else(|| format!("unknown plugin: {plugin}"))?;
            let contract = manifest.input_contract(&capability)
                .ok_or_else(|| format!("plugin {plugin} exposes no input contract for {capability}"))?;
            let context = semantic_context_from_json(&goal, &hints_json, &memory_json)?;
            let result = SemanticArgumentMapper::map(contract, &context)
                .map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&result).map_err(|error| error.to_string())?);
            if !result.valid() {
                return Err("semantic argument dry-run is not valid".into());
            }
        },
        Commands::WorkflowPlan { goal, hints_json, memory_json } => {
            let registry = initial_plugin_registry()?;
            let mut graph = initial_capability_graph()?;
            graph.attach_plugins(&registry);
            let context = semantic_context_from_json(&goal, &hints_json, &memory_json)?;
            let plan = AdaptiveWorkflowSynthesizer::synthesize(&goal, &graph, &registry, &context)
                .map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?);
            if !plan.ready {
                return Err("workflow dry-run is not ready; inspect unresolved_required/validation_errors".into());
            }
        },
        Commands::CognitivePlan { goal, hints_json, memory_json } => {
            let registry = initial_plugin_registry()?;
            let prompt = cognitive_planner_prompt(&goal, &registry);
            let mut config = PluginRuntimeConfig::default();
            config.timeout_ms = Some(120_000);
            config.bearer_token_env.insert("intelligence.nexus".into(), "NEXUS_BRIDGE_TOKEN".into());
            let executor = PluginExecutor::new(
                registry.clone(),
                PolicyEngine::default(),
                Arc::new(NativeRuntime::default()),
                config,
            );
            let result = executor.execute(PluginCall {
                plugin_id:"intelligence.nexus".into(),
                capability:"intelligence.reason".into(),
                input:serde_json::json!({"prompt":prompt,"context":{"mode":"cognitive-plan-v1"}}),
                approved:false,
            }).await.map_err(|error| error.to_string())?;

            let proposal = parse_cognitive_proposal_output(&result.output)?;

            let context = semantic_context_from_json(&goal, &hints_json, &memory_json)?;
            let plan = VerifiedCognitivePlanner::verify(&proposal, &registry, &context)
                .map_err(|error| error.to_string())?;
            println!("COGNITIVE_PROPOSAL\n{}", serde_json::to_string_pretty(&proposal).map_err(|error| error.to_string())?);
            println!("VERIFIED_COGNITIVE_DAG\n{}", serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?);
            if !plan.ready {
                return Err("verified cognitive plan is not execution-ready".into());
            }
        },
        Commands::CognitivePlanVerify { proposal, hints_json, memory_json } => {
            let raw = fs::read_to_string(&proposal)
                .map_err(|error| format!("cannot read cognitive proposal {}: {error}", proposal.display()))?;
            let proposal: CognitivePlanProposal = serde_json::from_str(&raw)
                .map_err(|error| format!("invalid cognitive proposal {}: {error}", proposal.display()))?;
            let registry = initial_plugin_registry()?;
            let context = semantic_context_from_json(&proposal.goal, &hints_json, &memory_json)?;
            let plan = VerifiedCognitivePlanner::verify(&proposal, &registry, &context)
                .map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?);
            if !plan.ready {
                return Err("verified cognitive plan is not execution-ready".into());
            }
        },
        Commands::WorkflowRun {
            goal, hints_json, memory_json, workspace, control_db,
            approve_step, approval_id, allow_env, allow_generated_program,
            runtime, docker_image, wsl_distro
        } => {
            let registry = initial_plugin_registry()?;
            let mut graph = initial_capability_graph()?;
            graph.attach_plugins(&registry);
            let context = semantic_context_from_json(&goal, &hints_json, &memory_json)?;
            let plan = AdaptiveWorkflowSynthesizer::synthesize(&goal, &graph, &registry, &context)
                .map_err(|error| error.to_string())?;
            println!("{}", serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?);
            if !plan.ready {
                return Err("workflow dry-run is not ready; execution refused".into());
            }

            let approved_steps = approve_step.into_iter().collect::<BTreeSet<_>>();
            let approval_ids = parse_step_value_map(&approval_id, "--approval-id")?;
            let steps = workflow_to_plugin_steps(
                &plan,
                &approved_steps,
                &approval_ids,
                control_db.is_some(),
            )?;

            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;
            let mut config = PluginRuntimeConfig::default();
            config.local_workspace = workspace;
            config.control_plane_db = control_db;
            config.generated_program_allowlist = allow_generated_program.into_iter().collect();
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
                "M9_WORKFLOW\nfinal_state={:?}\nevidence={}\naudit_events={}\nblocked_by_policy={}",
                summary.final_state,
                summary.evidence.len(),
                audit.list_by_correlation(summary.correlation_id).await?.len(),
                summary.blocked_by_policy,
            );
        },
        Commands::SemanticGap { goal } => {
            let registry = initial_plugin_registry()?;
            let mut graph = initial_capability_graph()?;
            graph.attach_plugins(&registry);
            match SemanticGapDetector::self_build_plan_typed(&graph, &registry, &goal) {
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
            max_routes: _, workspace, control_db, allow_env, allow_generated_program,
            runtime, docker_image, wsl_distro, organization, outcome_db, parallel
        } => {
            let mut graph = initial_capability_graph()?;
            let mut registry = initial_plugin_registry()?;
            let execution_runtime = build_plugin_runtime(
                &runtime,
                workspace.as_deref(),
                docker_image.as_deref(),
                wsl_distro.as_deref(),
            )?;
            let organization = load_organization(organization.as_deref())?;
            let outcomes = open_outcome_store(&outcome_db)?;
            let goal_fingerprint = fingerprint_goal(&goal);

            {
                let allowed_programs = allow_generated_program.iter().cloned().collect::<BTreeSet<_>>();
                let discovery = BuildDiscoveryChain::new(vec![
                    Arc::new(FilesystemOpenApiDiscovery::new(discovery_root.clone(), base_url.clone())),
                    Arc::new(FilesystemMcpDiscovery::new(
                        discovery_root.clone(),
                        execution_runtime.clone(),
                        allowed_programs.clone(),
                    )),
                    Arc::new(FilesystemCliDescriptorDiscovery::new(discovery_root.clone())),
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

                for _ in 0..16 {
                    let gaps = SemanticGapDetector::typed_gaps_for_goal(&graph, &registry, &goal);
                    if gaps.is_empty() {
                        break;
                    }
                    let mut progressed = false;
                    for gap in gaps {
                        let objective = gap.goal.clone();
                        let outcome = coordinator.ensure_typed_for_goal(&graph, &registry, &objective).await
                            .map_err(|error| error.to_string())?;
                        println!(
                            "self_build_objective={}\nself_build_outcome={}\n",
                            objective,
                            serde_json::to_string_pretty(&outcome).map_err(|error| error.to_string())?
                        );
                        match outcome {
                            SelfBuildOutcome::Promoted { manifest_path, .. } => {
                                let raw = fs::read_to_string(&manifest_path)
                                    .map_err(|error| format!(
                                        "cannot read promoted manifest {}: {error}",
                                        manifest_path.display()
                                    ))?;
                                let manifest = parse_manifest_json(&raw).map_err(|error| error.to_string())?;
                                registry.upsert(manifest).map_err(|error| error.to_string())?;
                                registry.validate_dependencies().map_err(|error| error.to_string())?;
                                graph.attach_plugins(&registry);
                                progressed = true;
                            }
                            SelfBuildOutcome::ExistingProvider => {}
                            SelfBuildOutcome::QualifiedAwaitingPromotion { candidate } => {
                                return Err(format!(
                                    "typed capability for objective {:?} is qualified but requires explicit promotion/approval: {}",
                                    objective, candidate.manifest.id
                                ));
                            }
                            SelfBuildOutcome::DiscoveryEmpty { .. } => {
                                eprintln!(
                                    "self_build_deferred objective={:?} reason=no typed provider or buildable adapter; M10 cognitive fallback may reuse existing typed capabilities",
                                    objective
                                );
                            }
                            SelfBuildOutcome::CandidateRejected { candidate } => {
                                eprintln!(
                                    "self_build_deferred objective={:?} reason=last discovered candidate failed qualification candidate={}; M10 cognitive fallback may choose another existing typed route",
                                    objective, candidate.manifest.id
                                );
                            }
                        }
                    }
                    if !progressed {
                        break;
                    }
                }

                let remaining = SemanticGapDetector::typed_gaps_for_goal(&graph, &registry, &goal);
                if !remaining.is_empty() {
                    eprintln!(
                        "typed_coverage_remaining={:?}; continuing to M10 verified cognitive fallback",
                        remaining.iter().map(|gap| gap.goal.clone()).collect::<Vec<_>>()
                    );
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
                registry.clone(),
                PolicyEngine::default(),
                execution_runtime,
                config,
            ));
            let audit = Arc::new(InMemoryAuditStore::default());
            let semantic_context = MappingContext {
                goal: goal.clone(),
                hints: BTreeMap::new(),
                memory: serde_json::Value::Null,
                prior_outputs: Vec::new(),
            };

            let deterministic = AdaptiveWorkflowSynthesizer::synthesize(
                &goal,
                &graph,
                &registry,
                &semantic_context,
            );

            let plan = match deterministic {
                Ok(plan) if plan.ready => {
                    println!(
                        "semantic_workflow={}\n",
                        serde_json::to_string_pretty(&plan).map_err(|error| error.to_string())?
                    );
                    plan
                }
                Ok(plan) => {
                    eprintln!(
                        "M9 deterministic workflow not ready: unresolved_objectives={:?}; invoking M10 verified cognitive fallback",
                        plan.unresolved_objectives
                    );
                    let prompt = cognitive_prompt_with_outcomes(&goal, &registry, &outcomes, &goal_fingerprint)?;
                    let result = executor.execute(PluginCall {
                        plugin_id:"intelligence.nexus".into(),
                        capability:"intelligence.reason".into(),
                        input:serde_json::json!({
                            "prompt":prompt,
                            "context":{"mode":"universal-goal-cognitive-fallback-v1"}
                        }),
                        approved:false,
                    }).await.map_err(|error| format!(
                        "M10 cognitive fallback unavailable after deterministic plan was not ready: {error}"
                    ))?;
                    let proposal = parse_cognitive_proposal_output(&result.output)?;
                    let verified = VerifiedCognitivePlanner::verify(
                        &proposal,
                        &registry,
                        &semantic_context,
                    ).map_err(|error| format!("M10 cognitive proposal rejected: {error}"))?;
                    println!(
                        "cognitive_proposal={}\nverified_cognitive_dag={}\n",
                        serde_json::to_string_pretty(&proposal).map_err(|error| error.to_string())?,
                        serde_json::to_string_pretty(&verified).map_err(|error| error.to_string())?,
                    );
                    if !verified.ready {
                        return Err(format!(
                            "M10 verified cognitive DAG is not execution-ready: unresolved_objectives={:?}; unresolved_fields={:?}",
                            verified.unresolved_objectives,
                            verified.steps.iter()
                                .filter(|step| !step.unresolved_required.is_empty())
                                .map(|step| (step.id.clone(), step.unresolved_required.clone()))
                                .collect::<Vec<_>>()
                        ));
                    }
                    verified
                }
                Err(error) => {
                    eprintln!("M9 deterministic workflow synthesis failed: {error}; invoking M10 verified cognitive fallback");
                    let prompt = cognitive_prompt_with_outcomes(&goal, &registry, &outcomes, &goal_fingerprint)?;
                    let result = executor.execute(PluginCall {
                        plugin_id:"intelligence.nexus".into(),
                        capability:"intelligence.reason".into(),
                        input:serde_json::json!({
                            "prompt":prompt,
                            "context":{"mode":"universal-goal-cognitive-fallback-v1"}
                        }),
                        approved:false,
                    }).await.map_err(|cognitive_error| format!(
                        "M9 synthesis failed ({error}); M10 cognitive fallback unavailable ({cognitive_error})"
                    ))?;
                    let proposal = parse_cognitive_proposal_output(&result.output)?;
                    let verified = VerifiedCognitivePlanner::verify(
                        &proposal,
                        &registry,
                        &semantic_context,
                    ).map_err(|verify_error| format!(
                        "M9 synthesis failed ({error}); M10 cognitive proposal rejected ({verify_error})"
                    ))?;
                    println!(
                        "cognitive_proposal={}\nverified_cognitive_dag={}\n",
                        serde_json::to_string_pretty(&proposal).map_err(|error| error.to_string())?,
                        serde_json::to_string_pretty(&verified).map_err(|error| error.to_string())?,
                    );
                    if !verified.ready {
                        return Err("M10 verified cognitive DAG is not execution-ready".into());
                    }
                    verified
                }
            };
            if let Some(step) = plan.steps.iter().find(|step| step.requires_approval) {
                return Err(format!(
                    "verified workflow step {} ({}/{}) requires explicit approval; use workflow-run --approve-step {}",
                    step.id, step.plugin_id, step.capability, step.id
                ));
            }
            let mut steps = workflow_to_plugin_steps(
                &plan,
                &BTreeSet::new(),
                &BTreeMap::new(),
                false,
            )?;
            assign_ai_employees(
                &mut steps,
                &organization,
                Some(&outcomes),
                &goal_fingerprint,
            )?;

            println!(
                "AI_STAFFING\n{}",
                serde_json::to_string_pretty(
                    &steps.iter().map(|step| serde_json::json!({
                        "step_id":step.id,
                        "plugin_id":step.plugin_id,
                        "capability":step.capability,
                        "employee_id":step.employee_id,
                        "department_id":step.department_id,
                    })).collect::<Vec<_>>()
                ).map_err(|error| error.to_string())?
            );

            if parallel {
                let engine = ParallelPluginWorkflowEngine::new(
                    executor,
                    audit.clone(),
                    Some(outcomes),
                );
                let summary = engine.run(
                    &goal_fingerprint,
                    steps,
                    plan.execution_layers.clone(),
                ).await?;
                println!(
                    "UNIVERSAL_GOAL_M11\nsuccess={}\nlayers_executed={}\noutcomes={}",
                    summary.success,
                    summary.layers_executed,
                    serde_json::to_string_pretty(&summary.outcomes).map_err(|error| error.to_string())?,
                );
                if !summary.success {
                    return Err("M11 parallel verified workflow failed".into());
                }
            } else {
                let engine = PluginGoalEngine::new(audit.clone(), executor, steps, 0);
                let summary = engine.run(goal.clone()).await.map_err(|error| error.to_string())?;
                println!(
                    "UNIVERSAL_GOAL\nfinal_state={:?}\nevidence={}\naudit_events={}\nblocked_by_policy={}",
                    summary.final_state,
                    summary.evidence.len(),
                    audit.list_by_correlation(summary.correlation_id).await?.len(),
                    summary.blocked_by_policy,
                );
            }
        },
        Commands::OrgValidate { organization } => {
            let org = load_organization(organization.as_deref())?;
            println!(
                "{}",
                serde_json::to_string_pretty(org.spec()).map_err(|error| error.to_string())?
            );
        },
        Commands::OrgSwarm { organization } => {
            let org = load_organization(organization.as_deref())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&org.agent_fabric_descriptors())
                    .map_err(|error| error.to_string())?
            );
        },
        Commands::OrgRoute { capability, risk, organization, outcome_db, goal } => {
            let org = load_organization(organization.as_deref())?;
            let risk = parse_risk_class(&risk)?;
            let eligible = org.eligible_employees(&capability, risk);
            let fingerprint = goal.as_deref().map(fingerprint_goal);
            let store = outcome_db.as_deref().map(open_outcome_store).transpose()?;
            let candidate_keys = eligible.iter().map(|employee| {
                format!("employee:{}|*/{}", employee.id, capability)
            }).collect::<Vec<_>>();

            let history = match (&store, &fingerprint) {
                (Some(store), Some(fingerprint)) => Some(
                    store.stats(fingerprint).map_err(|error| error.to_string())?
                ),
                _ => None,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "capability":capability,
                    "risk":risk,
                    "goal_fingerprint":fingerprint,
                    "eligible":eligible.iter().map(|employee| serde_json::json!({
                        "employee_id":employee.id,
                        "name":employee.name,
                        "title":employee.title,
                        "department_id":employee.department_id,
                        "model_policy":employee.model_policy,
                        "device_policy":employee.device_policy
                    })).collect::<Vec<_>>(),
                    "history":history,
                    "candidate_pattern_keys":candidate_keys
                })).map_err(|error| error.to_string())?
            );
        },
        Commands::OutcomeBest { db, goal } => {
            let store = open_outcome_store(&db)?;
            let fingerprint = fingerprint_goal(&goal);
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "goal_fingerprint":fingerprint,
                    "best":store.best_route(&fingerprint,2).map_err(|error| error.to_string())?,
                    "stats":store.stats(&fingerprint).map_err(|error| error.to_string())?
                })).map_err(|error| error.to_string())?
            );
        },
        Commands::WorkforceInit { db, organization } => {
            if let Some(parent)=db.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
            let org=load_organization(organization.as_deref())?;
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let count=store.seed_organization(&org).map_err(|e|e.to_string())?;
            println!("workforce_seeded={}\ndb={}",count,db.display());
        },
        Commands::WorkforceStatus { db } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&serde_json::json!({
              "employees":store.list_employee_states().map_err(|e|e.to_string())?,
              "nodes":store.list_nodes().map_err(|e|e.to_string())?
            })).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceNodeRegister { db,node_id,kind,endpoint,capabilities,cpu_millis,memory_mib } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let node=WorkforceNode{
              node_id,kind,endpoint,capabilities:capabilities.into_iter().collect(),
              cpu_millis,memory_mib,online:true,last_heartbeat:Utc::now()
            };
            store.register_node(&node).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&node).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceBindNode { db,employee,node_id } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.bind_employee_node(&employee,&node_id).map_err(|e|e.to_string())?;
            println!("bound_employee={}\nnode_id={}",employee,node_id);
        },
        Commands::WorkforceEnqueue { db,department,created_by,goal,plugin,capability,input_json,risk,employee,priority,budget_id,budget_microusd,approval_id,skills,tools } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let input:serde_json::Value=serde_json::from_str(&input_json).map_err(|e|format!("invalid --input-json: {e}"))?;
            let mut item=WorkItem::new(department,created_by,goal,plugin,capability,input,parse_risk_class(&risk)?);
            item.assigned_employee_id=employee;
            item.priority=priority;
            item.budget_id=budget_id;
            item.budget_microusd=budget_microusd.max(0);
            item.approval_id=approval_id;
            item.required_skills=parse_skill_requirements(&skills)?;
            item.required_tools=tools.into_iter().collect();
            store.enqueue(&item).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&item).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceSkillSet { db,employee,skill,level,evidence_count } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.put_skill(&employee,&skill,level,evidence_count).map_err(|e|e.to_string())?;
            println!("skill_set employee={} skill={} level={}",employee,skill,level.clamp(0.0,1.0));
        },
        Commands::WorkforceKpiSet { db,employee,key,target,current,weight } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.put_kpi(&employee,&key,target,current,weight).map_err(|e|e.to_string())?;
            println!("kpi_set employee={} key={} score={}",employee,key,store.employee_score(&employee).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceInitiativeAdd { db,employee,department,title,objective,score } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let now=Utc::now();
            let initiative=Initiative{
              id:Uuid::new_v4(),owner_employee_id:employee,department_id:department,title,objective,
              status:"proposed".into(),score:score.clamp(0.0,1.0),created_at:now,updated_at:now
            };
            store.add_initiative(&initiative).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&initiative).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceInitiativeList { db,department } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let values=store.list_initiatives(department.as_deref()).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&values).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceInitiativePromote { db,initiative_id,organization,workforce_config } => {
            let org=load_organization(organization.as_deref())?;
            let config=load_workforce_config(&workforce_config)?;
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.seed_organization(&org).map_err(|e|e.to_string())?;
            let id=Uuid::parse_str(&initiative_id).map_err(|e|format!("invalid initiative id: {e}"))?;
            let initiative=store.get_initiative(id).map_err(|e|e.to_string())?
              .ok_or_else(||format!("initiative not found: {id}"))?;
            let policy=config.policy_for(&initiative.department_id);
            let work=store.promote_initiative_to_manager_work(&org,id,Utc::now(),&policy).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&work).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceScheduleLink { db,employee,automation_id,purpose } => {
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.link_schedule(&employee,&automation_id,&purpose).map_err(|e|e.to_string())?;
            println!("schedule_linked employee={} automation_id={}",employee,automation_id);
        },
        Commands::WorkforceHeartbeat { db,employee,node_id } => {
            if employee.is_none() && node_id.is_none(){return Err("provide --employee and/or --node-id".into());}
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            let now=Utc::now();
            if let Some(employee)=employee { store.heartbeat_employee(&employee,now).map_err(|e|e.to_string())?; }
            if let Some(node_id)=node_id { store.heartbeat_node(&node_id,now).map_err(|e|e.to_string())?; }
            println!("heartbeat={}",now.to_rfc3339());
        },
        Commands::WorkforceRun { db,employee,owner,organization,workforce_config,workspace,control_db,allow_env,allow_generated_program,runtime,docker_image,wsl_distro } => {
            let org=Arc::new(load_organization(organization.as_deref())?);
            let config=load_workforce_config(&workforce_config)?;
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.seed_organization(&org).map_err(|e|e.to_string())?;
            store.heartbeat_employee(&employee,Utc::now()).map_err(|e|e.to_string())?;
            let executor=configured_plugin_executor(workspace,control_db,&allow_env,&allow_generated_program,&runtime,docker_image.as_deref(),wsl_distro.as_deref())?;
            let workforce=WorkforceExecutor::new(store,org,executor,config);
            let result=workforce.run_employee_once(&employee,&owner).await.map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&result).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceDepartmentCycle { db,department,owner_prefix,organization,workforce_config,workspace,control_db,allow_env,allow_generated_program,runtime,docker_image,wsl_distro } => {
            let org=Arc::new(load_organization(organization.as_deref())?);
            if org.department(&department).is_none(){return Err(format!("unknown department: {department}"));}
            let config=load_workforce_config(&workforce_config)?;
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.seed_organization(&org).map_err(|e|e.to_string())?;
            for employee in org.spec().employees.iter().filter(|e|e.department_id==department && e.autonomous) {
                store.heartbeat_employee(&employee.id,Utc::now()).map_err(|e|e.to_string())?;
            }
            let executor=configured_plugin_executor(workspace,control_db,&allow_env,&allow_generated_program,&runtime,docker_image.as_deref(),wsl_distro.as_deref())?;
            let workforce=WorkforceExecutor::new(store,org,executor,config);
            let results=workforce.run_department_once(&department,&owner_prefix).await.map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&results).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceMaintain { db,organization,workforce_config } => {
            let org=Arc::new(load_organization(organization.as_deref())?);
            let config=load_workforce_config(&workforce_config)?;
            let store=WorkforceStore::open(&db).map_err(|e|e.to_string())?;
            store.seed_organization(&org).map_err(|e|e.to_string())?;
            let dummy=configured_plugin_executor(None,None,&[],&[],"native",None,None)?;
            let workforce=WorkforceExecutor::new(store,org,dummy,config);
            let report=workforce.maintain(Utc::now()).map_err(|e|e.to_string())?;
            println!("{}",serde_json::to_string_pretty(&report).map_err(|e|e.to_string())?);
        },
        Commands::WorkforceSyncSwarm { organization,workspace,allow_env,runtime,docker_image,wsl_distro } => {
            let org=load_organization(organization.as_deref())?;
            let executor=configured_plugin_executor(workspace,None,&allow_env,&[],&runtime,docker_image.as_deref(),wsl_distro.as_deref())?;
            let mut results=Vec::new();
            for agent in org.agent_fabric_descriptors() {
                let input=serde_json::json!({
                  "id":agent.agent_id,"role":agent.role,"department_id":agent.department_id,
                  "supervisor_id":agent.supervisor_id,"cpu_millis":1000,"memory_mib":1024,
                  "tools":agent.tools,"capabilities":agent.capabilities,"remote":agent.remote_eligible,
                  "workspace":agent.workspace,"memory_namespace":agent.memory_namespace,
                  "model_policy":agent.model_policy,"device_policy":agent.device_policy
                });
                let result=executor.execute(PluginCall{
                  plugin_id:"workers.agent.fabric".into(),capability:"worker.register".into(),input,approved:false
                }).await.map_err(|e|e.to_string())?;
                results.push(result);
            }
            println!("{}",serde_json::to_string_pretty(&results).map_err(|e|e.to_string())?);
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
