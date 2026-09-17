use std::path::Path;

use autopilot_domain::{CheckpointRecord, CorrelationId, MissionId};
use autopilot_store::{SqliteStore, StoreError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryPhase {
    Planned,
    WorktreeReady,
    FleetVerified,
    Published,
    WaitingCi,
    RepairingCi,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionCheckpoint {
    pub phase: RecoveryPhase,
    pub worktree: Option<String>,
    pub correlation_id: Option<CorrelationId>,
    pub repairs: usize,
    pub ci_repairs: usize,
    pub pr_url: Option<String>,
    pub metadata: serde_json::Value,
}

impl MissionCheckpoint {
    pub fn planned() -> Self {
        Self {
            phase: RecoveryPhase::Planned,
            worktree: None,
            correlation_id: None,
            repairs: 0,
            ci_repairs: 0,
            pr_url: None,
            metadata: serde_json::json!({}),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryDecision {
    RestartFleet,
    ResumeWorktree(String),
    ResumePublish(String),
    ResumeCi { worktree: String, pr_url: String },
    FailMissingWorktree(String),
}

#[derive(Debug, Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct RecoveryManager {
    store: SqliteStore,
}

impl RecoveryManager {
    pub fn new(store: SqliteStore) -> Self { Self { store } }

    pub fn save(&self, mission_id: MissionId, checkpoint: &MissionCheckpoint) -> Result<(), RecoveryError> {
        self.store.save_checkpoint(&CheckpointRecord {
            mission_id,
            phase: format!("{:?}", checkpoint.phase),
            payload_json: serde_json::to_string(checkpoint)?,
            updated_at: Utc::now(),
        })?;
        Ok(())
    }

    pub fn load(&self, mission_id: MissionId) -> Result<Option<MissionCheckpoint>, RecoveryError> {
        self.store
            .load_checkpoint(mission_id)?
            .map(|record| serde_json::from_str(&record.payload_json).map_err(RecoveryError::from))
            .transpose()
    }

    pub fn decide(&self, checkpoint: &MissionCheckpoint) -> RecoveryDecision {
        match checkpoint.phase {
            RecoveryPhase::Planned => RecoveryDecision::RestartFleet,
            RecoveryPhase::WorktreeReady | RecoveryPhase::FleetVerified => {
                match checkpoint.worktree.as_deref() {
                    Some(path) if Path::new(path).exists() => RecoveryDecision::ResumeWorktree(path.to_string()),
                    Some(path) => RecoveryDecision::FailMissingWorktree(path.to_string()),
                    None => RecoveryDecision::RestartFleet,
                }
            }
            RecoveryPhase::Published => {
                match checkpoint.worktree.as_deref() {
                    Some(path) if Path::new(path).exists() => RecoveryDecision::ResumePublish(path.to_string()),
                    Some(path) => RecoveryDecision::FailMissingWorktree(path.to_string()),
                    None => RecoveryDecision::RestartFleet,
                }
            }
            RecoveryPhase::WaitingCi | RecoveryPhase::RepairingCi => {
                match (checkpoint.worktree.as_deref(), checkpoint.pr_url.as_deref()) {
                    (Some(path), Some(pr_url)) if Path::new(path).exists() => RecoveryDecision::ResumeCi {
                        worktree: path.to_string(),
                        pr_url: pr_url.to_string(),
                    },
                    (Some(path), _) => RecoveryDecision::FailMissingWorktree(path.to_string()),
                    _ => RecoveryDecision::RestartFleet,
                }
            }
        }
    }
}
