use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use autopilot_audit::InMemoryAuditStore;
use autopilot_domain::AgentRole;
use autopilot_engine::{FleetEngine, FleetEngineConfig, VerificationCommand};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_router::{ModelCandidate, RoutingProfile};
use autopilot_runtime::{NativeRuntime, RuntimeAdapter};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Clone)]
struct AppState {
    runtime: NativeRuntime,
}

#[derive(Serialize)]
struct SystemOverview {
    product: &'static str,
    autonomy: &'static str,
    deployment: &'static str,
    runtime: &'static str,
    policy: &'static str,
    version: &'static str,
}

#[derive(Debug, Deserialize)]
struct AgentRunRequest {
    backend: String,
    prompt: String,
    workdir: String,
    model: Option<String>,
    #[serde(default)]
    allow_env: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct AgentRunResponse {
    backend: String,
    termination: String,
    stdout: String,
    stderr: String,
    duration_ms: u128,
}

#[derive(Debug, Deserialize)]
struct FleetRunRequest {
    goal: String,
    repo: String,
    worktree_root: Option<String>,
    base_ref: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    #[serde(default)]
    routes: Vec<String>,
    profile: Option<String>,
    #[serde(default)]
    allow_env: Vec<String>,
    timeout_ms: Option<u64>,
    max_repairs: Option<usize>,
    verify_program: Option<String>,
    #[serde(default)]
    verify_args: Vec<String>,
}

#[derive(Serialize)]
struct FleetRoleResponse {
    role: String,
    provider: String,
    model: String,
    backend: String,
    success: bool,
    summary: String,
}

#[derive(Serialize)]
struct FleetRunResponse {
    final_state: String,
    repairs: usize,
    evidence: Vec<String>,
    role_runs: Vec<FleetRoleResponse>,
    worktree: Option<String>,
}

fn backend_from_name(name: &str) -> Result<BackendKind, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "opencode" => Ok(BackendKind::OpenCode),
        "codex" => Ok(BackendKind::Codex),
        "claude" | "claude-code" => Ok(BackendKind::Claude),
        "gemini" => Ok(BackendKind::Gemini),
        other => Err(format!("unsupported builtin backend: {other}")),
    }
}

fn parse_role(value: &str) -> Result<AgentRole, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "architect" => Ok(AgentRole::Architect),
        "coder" => Ok(AgentRole::Coder),
        "tester" => Ok(AgentRole::Tester),
        "reviewer" => Ok(AgentRole::Reviewer),
        "security" => Ok(AgentRole::Security),
        "debugger" => Ok(AgentRole::Debugger),
        other => Err(format!("unsupported fleet role: {other}")),
    }
}

fn parse_profile(value: Option<&str>) -> Result<RoutingProfile, String> {
    match value.unwrap_or("best-quality").trim().to_ascii_lowercase().as_str() {
        "best" | "best-quality" | "quality" => Ok(RoutingProfile::BestQuality),
        "balanced" => Ok(RoutingProfile::Balanced),
        "budget" => Ok(RoutingProfile::Budget),
        other => Err(format!("unsupported routing profile: {other}")),
    }
}

fn parse_route(value: &str) -> Result<(AgentRole, String, String), String> {
    let (role, target) = value
        .split_once('=')
        .ok_or_else(|| format!("route must use role=provider[:model], got: {value}"))?;
    let role = parse_role(role)?;
    let (provider, model) = match target.split_once(':') {
        Some((provider, model)) => (provider.to_string(), model.to_string()),
        None => (target.to_string(), String::new()),
    };
    if provider.trim().is_empty() {
        return Err(format!("route provider is empty: {value}"));
    }
    Ok((role, provider, model))
}

fn candidate(provider: String, model: String, role: AgentRole) -> ModelCandidate {
    let mut affinities = BTreeMap::new();
    affinities.insert(role, 100);
    ModelCandidate {
        provider,
        model,
        quality: 90,
        cost_per_million: 1.0,
        latency_ms: 500,
        healthy: true,
        role_affinities: affinities,
    }
}

fn fleet_candidates(
    default_provider: String,
    default_model: Option<String>,
    routes: Vec<String>,
) -> Result<Vec<ModelCandidate>, String> {
    let mut overrides = BTreeMap::new();
    for route in routes {
        let (role, provider, model) = parse_route(&route)?;
        overrides.insert(role, (provider, model));
    }

    let default_model = default_model.unwrap_or_default();
    Ok([
        AgentRole::Architect,
        AgentRole::Coder,
        AgentRole::Tester,
        AgentRole::Reviewer,
        AgentRole::Security,
        AgentRole::Debugger,
    ]
    .into_iter()
    .map(|role| {
        let (provider, model) = overrides
            .get(&role)
            .cloned()
            .unwrap_or_else(|| (default_provider.clone(), default_model.clone()));
        candidate(provider, model, role)
    })
    .collect())
}

#[tauri::command]
fn system_overview() -> SystemOverview {
    SystemOverview {
        product: "OPEN AUTOPILOT",
        autonomy: "Full Autonomy",
        deployment: "Hybrid local-first",
        runtime: "Native Windows",
        policy: "Force-push/release denied; high-risk secrets/deletion guarded",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[tauri::command]
fn list_backends() -> Vec<&'static str> {
    vec!["OpenCode", "Codex", "Claude", "Gemini"]
}

#[tauri::command]
async fn run_agent(request: AgentRunRequest, state: State<'_, AppState>) -> Result<AgentRunResponse, String> {
    if request.workdir.trim().is_empty() {
        return Err("workdir cannot be empty".into());
    }
    if request.prompt.trim().is_empty() {
        return Err("prompt cannot be empty".into());
    }

    let backend = backend_from_name(&request.backend)?;
    let mut spec = RunSpec::new(backend, request.prompt, request.workdir.into());
    if let Some(model) = request.model.filter(|value| !value.trim().is_empty()) {
        spec = spec.model(model);
    }
    for key in request.allow_env.into_iter().filter(|key| !key.trim().is_empty()) {
        spec = spec.allow_env(key);
    }
    if let Some(timeout_ms) = request.timeout_ms {
        spec = spec.timeout_ms(timeout_ms);
    }

    let harness = AgentHarness::new(Arc::new(state.runtime.clone()), BackendRegistry::default());
    let result = harness.run(spec).await.map_err(|error| error.to_string())?;
    let termination = match result.termination {
        Termination::Exited => "Exited".to_string(),
        Termination::NonZero(code) => format!("NonZero({code})"),
    };

    Ok(AgentRunResponse {
        backend: format!("{:?}", result.backend),
        termination,
        stdout: result.stdout,
        stderr: result.stderr,
        duration_ms: result.duration_ms,
    })
}

#[tauri::command]
async fn run_fleet(request: FleetRunRequest, state: State<'_, AppState>) -> Result<FleetRunResponse, String> {
    if request.goal.trim().is_empty() {
        return Err("goal cannot be empty".into());
    }
    if request.repo.trim().is_empty() {
        return Err("repo cannot be empty".into());
    }

    let repo = PathBuf::from(request.repo);
    let worktree_root = request
        .worktree_root
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(".open-autopilot").join("worktrees"));
    let verification = request
        .verify_program
        .filter(|program| !program.trim().is_empty())
        .map(|program| vec![VerificationCommand::new("acceptance", program, request.verify_args)])
        .unwrap_or_default();
    let candidates = fleet_candidates(
        request.provider.unwrap_or_else(|| "opencode".into()),
        request.model,
        request.routes,
    )?;
    let audit = Arc::new(InMemoryAuditStore::default());
    let engine = FleetEngine::new(
        Arc::new(state.runtime.clone()),
        audit,
        FleetEngineConfig {
            repo_root: repo,
            worktree_root,
            base_ref: request.base_ref.unwrap_or_else(|| "main".into()),
            routing_profile: parse_profile(request.profile.as_deref())?,
            candidates,
            allowed_env: request.allow_env,
            timeout_ms: request.timeout_ms,
            verification,
            max_repairs: request.max_repairs.unwrap_or(2),
        },
    );
    let summary = engine.run(request.goal).await?;

    Ok(FleetRunResponse {
        final_state: format!("{:?}", summary.final_state),
        repairs: summary.repairs,
        evidence: summary
            .evidence
            .into_iter()
            .map(|item| format!("{}:{}:{}", item.kind, item.passed, item.summary))
            .collect(),
        role_runs: summary
            .role_runs
            .into_iter()
            .map(|run| FleetRoleResponse {
                role: format!("{:?}", run.role),
                provider: run.provider,
                model: run.model,
                backend: format!("{:?}", run.backend),
                success: run.success,
                summary: run.summary,
            })
            .collect(),
        worktree: summary.worktree.map(|path| path.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
async fn stop_all(state: State<'_, AppState>) -> Result<String, String> {
    state.runtime.cancel().await.map_err(|error| error.to_string())?;
    Ok("Emergency stop sent to active local execution".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState { runtime: NativeRuntime::default() })
        .invoke_handler(tauri::generate_handler![
            system_overview,
            list_backends,
            run_agent,
            run_fleet,
            stop_all
        ])
        .run(tauri::generate_context!())
        .expect("error while running OPEN AUTOPILOT desktop");
}
