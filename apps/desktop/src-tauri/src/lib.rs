use std::sync::Arc;

use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
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

fn backend_from_name(name: &str) -> Result<BackendKind, String> {
    match name.trim().to_ascii_lowercase().as_str() {
        "opencode" => Ok(BackendKind::OpenCode),
        "codex" => Ok(BackendKind::Codex),
        "claude" | "claude-code" => Ok(BackendKind::Claude),
        "gemini" => Ok(BackendKind::Gemini),
        other => Err(format!("unsupported builtin backend: {other}")),
    }
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
async fn stop_all(state: State<'_, AppState>) -> Result<String, String> {
    state.runtime.cancel().await.map_err(|error| error.to_string())?;
    Ok("Emergency stop sent to active local execution".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState { runtime: NativeRuntime::default() })
        .invoke_handler(tauri::generate_handler![system_overview, list_backends, run_agent, stop_all])
        .run(tauri::generate_context!())
        .expect("error while running OPEN AUTOPILOT desktop");
}
