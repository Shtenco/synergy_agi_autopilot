use std::path::{Path, PathBuf};

use autopilot_domain::{MissionId, MissionState};
use autopilot_missions::MissionQueue;
use autopilot_store::SqliteStore;
use chrono::Duration;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

#[derive(Clone)]
pub struct MissionAppState {
    store: SqliteStore,
}

#[derive(Serialize)]
pub struct MissionView {
    id: String,
    objective: String,
    project: String,
    state: String,
    phase: String,
    attempt: u32,
    lease_owner: Option<String>,
    lease_until: Option<String>,
    checkpoint_phase: Option<String>,
}

pub fn init_state() -> Result<MissionAppState, String> {
    let path = default_db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    Ok(MissionAppState { store: SqliteStore::open(path).map_err(|error| error.to_string())? })
}

fn queue(state: &MissionAppState) -> MissionQueue {
    MissionQueue::new(state.store.clone(), Duration::minutes(5))
}

fn to_view(state: &MissionAppState, mission: autopilot_domain::MissionRecord) -> MissionView {
    let checkpoint_phase = state.store.load_checkpoint(mission.id).ok().flatten().map(|item| item.phase);
    MissionView {
        id: mission.id.0.to_string(),
        objective: mission.objective,
        project: mission.project,
        state: format!("{:?}", mission.state),
        phase: mission.phase,
        attempt: mission.attempt,
        lease_owner: mission.lease_owner,
        lease_until: mission.lease_until.map(|value| value.to_rfc3339()),
        checkpoint_phase,
    }
}

#[tauri::command]
pub fn mission_enqueue(goal: String, project: String, state: State<'_, MissionAppState>) -> Result<MissionView, String> {
    if goal.trim().is_empty() { return Err("mission goal cannot be empty".into()); }
    if project.trim().is_empty() { return Err("mission project cannot be empty".into()); }
    let mission = queue(&state).enqueue(goal, project).map_err(|error| error.to_string())?;
    Ok(to_view(&state, mission))
}

#[tauri::command]
pub fn mission_list(state: State<'_, MissionAppState>) -> Result<Vec<MissionView>, String> {
    let missions = queue(&state).list().map_err(|error| error.to_string())?;
    Ok(missions.into_iter().map(|mission| to_view(&state, mission)).collect())
}

#[tauri::command]
pub fn mission_recover(state: State<'_, MissionAppState>) -> Result<Vec<String>, String> {
    Ok(queue(&state)
        .recover_expired()
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|id| id.0.to_string())
        .collect())
}

#[tauri::command]
pub fn mission_cancel(id: String, state: State<'_, MissionAppState>) -> Result<MissionView, String> {
    let id = MissionId(Uuid::parse_str(&id).map_err(|error| error.to_string())?);
    let mut mission = state.store.get_mission(id).map_err(|error| error.to_string())?
        .ok_or_else(|| "mission not found".to_string())?;
    if mission.state.is_terminal() { return Ok(to_view(&state, mission)); }
    if !mission.state.can_transition_to(MissionState::Cancelled) {
        return Err(format!("mission cannot transition from {:?} to Cancelled", mission.state));
    }
    mission.state = MissionState::Cancelled;
    mission.phase = "Cancelled".into();
    mission.lease_owner = None;
    mission.lease_until = None;
    mission.updated_at = chrono::Utc::now();
    state.store.update_mission(&mission).map_err(|error| error.to_string())?;
    state.store.append_mission_event(id, "cancelled", "{}").map_err(|error| error.to_string())?;
    Ok(to_view(&state, mission))
}

pub fn default_db_path() -> PathBuf {
    if let Ok(root) = std::env::var("LOCALAPPDATA") {
        return Path::new(&root).join("OpenAutopilot").join("state.db");
    }
    if let Ok(home) = std::env::var("HOME") {
        return Path::new(&home).join(".open-autopilot").join("state.db");
    }
    PathBuf::from(".open-autopilot/state.db")
}
