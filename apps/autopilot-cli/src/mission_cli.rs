use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use autopilot_audit::AuditStore;
use autopilot_domain::{AgentRole, GoalState, MissionId};
use autopilot_engine::{FleetEngine, FleetEngineConfig, VerificationCommand};
use autopilot_missions::MissionQueue;
use autopilot_recovery::{MissionCheckpoint, RecoveryManager, RecoveryPhase};
use autopilot_router::{ModelCandidate, RoutingProfile};
use autopilot_runtime::{DockerRuntime, NativeRuntime, RuntimeAdapter};
use autopilot_store::SqliteStore;
use chrono::Duration;
use clap::Subcommand;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum MissionCommands {
    Enqueue {
        #[arg(long)] goal: String,
        #[arg(long)] project: PathBuf,
        #[arg(long)] db: Option<PathBuf>,
    },
    List {
        #[arg(long)] db: Option<PathBuf>,
    },
    Show {
        #[arg(long)] id: String,
        #[arg(long)] db: Option<PathBuf>,
    },
    Recover {
        #[arg(long)] db: Option<PathBuf>,
    },
    RunNext {
        #[arg(long, default_value = "local-cli")] owner: String,
        #[arg(long)] db: Option<PathBuf>,
        #[arg(long, default_value = "opencode")] provider: String,
        #[arg(long)] model: Option<String>,
        #[arg(long)] docker_image: String,
        #[arg(long)] verify_program: Option<String>,
        #[arg(long = "verify-arg")] verify_args: Vec<String>,
        #[arg(long, default_value_t = 2)] max_repairs: usize,
        #[arg(long, default_value_t = 300)] lease_seconds: i64,
    },
}

pub async fn handle(command: MissionCommands) -> Result<(), String> {
    match command {
        MissionCommands::Enqueue { goal, project, db } => {
            let queue = queue(db, 300)?;
            let mission = queue.enqueue(goal, project.to_string_lossy().into_owned()).map_err(|e| e.to_string())?;
            println!("mission_id={}\nstate={:?}\nproject={}", mission.id.0, mission.state, mission.project);
        }
        MissionCommands::List { db } => {
            let queue = queue(db, 300)?;
            for mission in queue.list().map_err(|e| e.to_string())? {
                println!("{}\t{:?}\tattempt={}\tphase={}\t{}", mission.id.0, mission.state, mission.attempt, mission.phase, mission.project);
            }
        }
        MissionCommands::Show { id, db } => {
            let store = open_store(db)?;
            let id = MissionId(Uuid::parse_str(&id).map_err(|e| e.to_string())?);
            let mission = store.get_mission(id).map_err(|e| e.to_string())?.ok_or_else(|| "mission not found".to_string())?;
            println!("id={}\nstate={:?}\nattempt={}\nphase={}\nproject={}\nobjective={}", mission.id.0, mission.state, mission.attempt, mission.phase, mission.project, mission.objective);
            if let Some(checkpoint) = store.load_checkpoint(id).map_err(|e| e.to_string())? {
                println!("checkpoint_phase={}\ncheckpoint={}", checkpoint.phase, checkpoint.payload_json);
            }
        }
        MissionCommands::Recover { db } => {
            let queue = queue(db, 300)?;
            let recovered = queue.recover_expired().map_err(|e| e.to_string())?;
            println!("recovered={}", recovered.len());
            for id in recovered { println!("{}", id.0); }
        }
        MissionCommands::RunNext { owner, db, provider, model, docker_image, verify_program, verify_args, max_repairs, lease_seconds } => {
            if lease_seconds <= 0 { return Err("lease_seconds must be positive".into()); }
            let store = open_store(db)?;
            let queue = MissionQueue::new(store.clone(), Duration::seconds(lease_seconds));
            let Some(lease) = queue.lease_next(owner.clone()).map_err(|e| e.to_string())? else {
                println!("mission_queue=empty");
                return Ok(());
            };
            let mission = queue.start(lease.mission.id, &owner, "Planning").map_err(|e| e.to_string())?;
            let recovery = RecoveryManager::new(store.clone());
            recovery.save(mission.id, &MissionCheckpoint::planned()).map_err(|e| e.to_string())?;

            let project = PathBuf::from(&mission.project);
            if !project.exists() {
                queue.fail(mission.id, &owner, "MissingProject").map_err(|e| e.to_string())?;
                return Err(format!("mission project does not exist: {}", project.display()));
            }
            let verification = verify_program
                .filter(|v| !v.trim().is_empty())
                .map(|program| vec![VerificationCommand::new("acceptance", program, verify_args)])
                .unwrap_or_default();
            let writer: Arc<dyn RuntimeAdapter> = Arc::new(NativeRuntime::default());
            let review: Arc<dyn RuntimeAdapter> = Arc::new(DockerRuntime::new(docker_image, project.clone()));
            if !review.probe().await {
                queue.release(mission.id, &owner).map_err(|e| e.to_string())?;
                return Err("Docker review runtime is unavailable; mission returned to queue".into());
            }
            let audit: Arc<dyn AuditStore> = Arc::new(store.clone());
            let engine = FleetEngine::with_role_runtimes(
                writer,
                review,
                audit,
                FleetEngineConfig {
                    repo_root: project.clone(),
                    worktree_root: project.join(".open-autopilot").join("worktrees"),
                    base_ref: "main".into(),
                    routing_profile: RoutingProfile::BestQuality,
                    candidates: candidates(&provider, model.as_deref()),
                    allowed_env: Vec::new(),
                    timeout_ms: Some(1_800_000),
                    verification,
                    max_repairs,
                },
            );
            let summary = engine.run(mission.objective.clone()).await;
            match summary {
                Ok(summary) if summary.final_state == GoalState::Completed => {
                    let worktree = summary.worktree.as_ref().map(|p| p.to_string_lossy().into_owned());
                    recovery.save(mission.id, &MissionCheckpoint {
                        phase: RecoveryPhase::FleetVerified,
                        worktree,
                        correlation_id: Some(summary.correlation_id),
                        repairs: summary.repairs,
                        ci_repairs: 0,
                        pr_url: None,
                        metadata: serde_json::json!({"evidence_count": summary.evidence.len()}),
                    }).map_err(|e| e.to_string())?;
                    queue.complete(mission.id, &owner).map_err(|e| e.to_string())?;
                    println!("mission_id={}\nstate=Completed\ncorrelation_id={}", mission.id.0, summary.correlation_id.0);
                }
                Ok(summary) => {
                    queue.fail(mission.id, &owner, "FleetFailed").map_err(|e| e.to_string())?;
                    return Err(format!("fleet ended in {:?}", summary.final_state));
                }
                Err(error) => {
                    queue.release(mission.id, &owner).map_err(|e| e.to_string())?;
                    return Err(format!("fleet execution interrupted; mission returned to queue: {error}"));
                }
            }
        }
    }
    Ok(())
}

fn candidates(provider: &str, model: Option<&str>) -> Vec<ModelCandidate> {
    [AgentRole::Architect, AgentRole::Coder, AgentRole::Tester, AgentRole::Reviewer, AgentRole::Security, AgentRole::Debugger]
        .into_iter()
        .map(|role| {
            let mut role_affinities = BTreeMap::new();
            role_affinities.insert(role.clone(), 100);
            ModelCandidate {
                provider: provider.into(),
                model: model.unwrap_or_default().into(),
                quality: 90,
                cost_per_million: 1.0,
                latency_ms: 500,
                healthy: true,
                role_affinities,
            }
        })
        .collect()
}

fn queue(path: Option<PathBuf>, lease_seconds: i64) -> Result<MissionQueue, String> {
    Ok(MissionQueue::new(open_store(path)?, Duration::seconds(lease_seconds)))
}

fn open_store(path: Option<PathBuf>) -> Result<SqliteStore, String> {
    let path = path.unwrap_or_else(default_db_path);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    SqliteStore::open(path).map_err(|e| e.to_string())
}

fn default_db_path() -> PathBuf {
    if let Ok(root) = std::env::var("LOCALAPPDATA") {
        return Path::new(&root).join("OpenAutopilot").join("state.db");
    }
    if let Ok(home) = std::env::var("HOME") {
        return Path::new(&home).join(".open-autopilot").join("state.db");
    }
    PathBuf::from(".open-autopilot/state.db")
}
