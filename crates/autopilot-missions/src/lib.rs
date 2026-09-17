use autopilot_domain::{CheckpointRecord, MissionId, MissionRecord, MissionState};
use autopilot_store::{SqliteStore, StoreError};
use chrono::{Duration, Utc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MissionError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("mission not found")]
    NotFound,
    #[error("mission is not leasable")]
    NotLeasable,
    #[error("mission lease owner mismatch")]
    LeaseOwnerMismatch,
    #[error("illegal mission state transition {from:?} -> {to:?}")]
    IllegalTransition { from: MissionState, to: MissionState },
}

pub type MissionResult<T> = Result<T, MissionError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionLease {
    pub mission: MissionRecord,
    pub owner: String,
}

#[derive(Clone)]
pub struct MissionQueue {
    store: SqliteStore,
    lease_duration: Duration,
}

impl MissionQueue {
    pub fn new(store: SqliteStore, lease_duration: Duration) -> Self {
        Self { store, lease_duration }
    }

    pub fn store(&self) -> &SqliteStore { &self.store }

    pub fn enqueue(&self, objective: impl Into<String>, project: impl Into<String>) -> MissionResult<MissionRecord> {
        let mission = MissionRecord::queued(objective, project);
        self.store.insert_mission(&mission)?;
        self.store.append_mission_event(mission.id, "enqueued", "{}")?;
        Ok(mission)
    }

    pub fn list(&self) -> MissionResult<Vec<MissionRecord>> { Ok(self.store.list_missions()?) }

    pub fn lease_next(&self, owner: impl Into<String>) -> MissionResult<Option<MissionLease>> {
        let owner = owner.into();
        let mut missions = self.store.list_missions()?;
        missions.sort_by_key(|mission| mission.created_at);
        let Some(mut mission) = missions.into_iter().find(|mission| mission.state == MissionState::Queued) else {
            return Ok(None);
        };
        let now = Utc::now();
        transition(&mut mission, MissionState::Leased)?;
        mission.attempt = mission.attempt.saturating_add(1);
        mission.lease_owner = Some(owner.clone());
        mission.lease_until = Some(now + self.lease_duration);
        mission.updated_at = now;
        self.store.update_mission(&mission)?;
        self.store.append_mission_event(mission.id, "leased", &serde_json::json!({"owner": owner, "attempt": mission.attempt}).to_string())?;
        Ok(Some(MissionLease { mission, owner }))
    }

    pub fn start(&self, id: MissionId, owner: &str, phase: impl Into<String>) -> MissionResult<MissionRecord> {
        let mut mission = self.require_owned(id, owner)?;
        transition(&mut mission, MissionState::Running)?;
        mission.phase = phase.into();
        mission.updated_at = Utc::now();
        self.store.update_mission(&mission)?;
        self.store.append_mission_event(id, "started", &serde_json::json!({"phase": mission.phase}).to_string())?;
        Ok(mission)
    }

    pub fn heartbeat(&self, id: MissionId, owner: &str) -> MissionResult<MissionRecord> {
        let mut mission = self.require_owned(id, owner)?;
        if !matches!(mission.state, MissionState::Leased | MissionState::Running | MissionState::Waiting) {
            return Err(MissionError::NotLeasable);
        }
        let now = Utc::now();
        mission.lease_until = Some(now + self.lease_duration);
        mission.updated_at = now;
        self.store.update_mission(&mission)?;
        Ok(mission)
    }

    pub fn checkpoint(&self, id: MissionId, owner: &str, phase: impl Into<String>, payload_json: impl Into<String>) -> MissionResult<()> {
        let mut mission = self.require_owned(id, owner)?;
        let phase = phase.into();
        mission.phase = phase.clone();
        mission.updated_at = Utc::now();
        self.store.update_mission(&mission)?;
        self.store.save_checkpoint(&CheckpointRecord { mission_id: id, phase, payload_json: payload_json.into(), updated_at: Utc::now() })?;
        self.store.append_mission_event(id, "checkpoint", "{}")?;
        Ok(())
    }

    pub fn complete(&self, id: MissionId, owner: &str) -> MissionResult<MissionRecord> {
        self.finish(id, owner, MissionState::Completed, "Completed")
    }

    pub fn fail(&self, id: MissionId, owner: &str, phase: impl Into<String>) -> MissionResult<MissionRecord> {
        self.finish(id, owner, MissionState::Failed, &phase.into())
    }

    pub fn release(&self, id: MissionId, owner: &str) -> MissionResult<MissionRecord> {
        let mut mission = self.require_owned(id, owner)?;
        transition(&mut mission, MissionState::Queued)?;
        mission.lease_owner = None;
        mission.lease_until = None;
        mission.updated_at = Utc::now();
        self.store.update_mission(&mission)?;
        self.store.append_mission_event(id, "released", "{}")?;
        Ok(mission)
    }

    pub fn recover_expired(&self) -> MissionResult<Vec<MissionId>> {
        let now = Utc::now();
        let mut recovered = Vec::new();
        for mut mission in self.store.list_missions()? {
            if matches!(mission.state, MissionState::Leased | MissionState::Running | MissionState::Waiting)
                && mission.lease_until.map(|deadline| deadline < now).unwrap_or(true)
            {
                if mission.state.can_transition_to(MissionState::Queued) {
                    mission.state = MissionState::Queued;
                    mission.lease_owner = None;
                    mission.lease_until = None;
                    mission.updated_at = now;
                    self.store.update_mission(&mission)?;
                    self.store.append_mission_event(mission.id, "recovered-expired", "{}")?;
                    recovered.push(mission.id);
                }
            }
        }
        Ok(recovered)
    }

    fn require_owned(&self, id: MissionId, owner: &str) -> MissionResult<MissionRecord> {
        let mission = self.store.get_mission(id)?.ok_or(MissionError::NotFound)?;
        if mission.lease_owner.as_deref() != Some(owner) { return Err(MissionError::LeaseOwnerMismatch); }
        Ok(mission)
    }

    fn finish(&self, id: MissionId, owner: &str, state: MissionState, phase: &str) -> MissionResult<MissionRecord> {
        let mut mission = self.require_owned(id, owner)?;
        transition(&mut mission, state)?;
        mission.phase = phase.into();
        mission.lease_owner = None;
        mission.lease_until = None;
        mission.updated_at = Utc::now();
        self.store.update_mission(&mission)?;
        self.store.append_mission_event(id, "terminal", &serde_json::json!({"state": format!("{state:?}")}).to_string())?;
        Ok(mission)
    }
}

fn transition(mission: &mut MissionRecord, next: MissionState) -> MissionResult<()> {
    if !mission.state.can_transition_to(next) {
        return Err(MissionError::IllegalTransition { from: mission.state, to: next });
    }
    mission.state = next;
    Ok(())
}
