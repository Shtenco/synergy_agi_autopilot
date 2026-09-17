use std::str::FromStr;

use autopilot_domain::{CheckpointRecord, MissionId, MissionRecord, MissionState};
use autopilot_store::{SqliteStore, StoreError};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use thiserror::Error;
use uuid::Uuid;

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
        let now = Utc::now();
        let lease_until = now + self.lease_duration;
        let queued_json = serde_json::to_string(&MissionState::Queued).map_err(StoreError::from)?;
        let leased_json = serde_json::to_string(&MissionState::Leased).map_err(StoreError::from)?;
        let owner_for_event = owner.clone();
        let leased = self.store.transaction(|tx| {
            let sql = r#"
                UPDATE missions
                SET state_json=?1,
                    attempt=attempt+1,
                    lease_owner=?2,
                    lease_until=?3,
                    updated_at=?4
                WHERE id=(
                    SELECT id FROM missions
                    WHERE state_json=?5
                    ORDER BY created_at,id
                    LIMIT 1
                )
                AND state_json=?5
                RETURNING id,objective,project,state_json,phase,attempt,lease_owner,lease_until,created_at,updated_at
            "#;
            let mission = tx.query_row(
                sql,
                params![leased_json, owner, lease_until.to_rfc3339(), now.to_rfc3339(), queued_json],
                mission_from_row,
            ).optional()?;
            if let Some(mission) = &mission {
                tx.execute(
                    "INSERT INTO mission_events(mission_id,timestamp,event_type,payload_json) VALUES (?1,?2,'leased',?3)",
                    params![mission.id.0.to_string(), now.to_rfc3339(), serde_json::json!({"owner": owner_for_event, "attempt": mission.attempt}).to_string()],
                )?;
            }
            Ok(mission)
        })?;
        Ok(leased.map(|mission| MissionLease { mission, owner: owner_for_event }))
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
        let phase = phase.into();
        self.finish(id, owner, MissionState::Failed, &phase)
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

fn mission_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MissionRecord> {
    let id_text: String = row.get(0)?;
    let id = Uuid::from_str(&id_text).map_err(|error| rusqlite::Error::FromSqlConversionFailure(id_text.len(), rusqlite::types::Type::Text, Box::new(error)))?;
    let state_text: String = row.get(3)?;
    let state: MissionState = serde_json::from_str(&state_text)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(state_text.len(), rusqlite::types::Type::Text, Box::new(error)))?;
    let parse_time = |value: String| -> rusqlite::Result<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(&value)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| rusqlite::Error::FromSqlConversionFailure(value.len(), rusqlite::types::Type::Text, Box::new(error)))
    };
    Ok(MissionRecord {
        id: MissionId(id),
        objective: row.get(1)?,
        project: row.get(2)?,
        state,
        phase: row.get(4)?,
        attempt: row.get::<_, i64>(5)? as u32,
        lease_owner: row.get(6)?,
        lease_until: row.get::<_, Option<String>>(7)?.map(parse_time).transpose()?,
        created_at: parse_time(row.get(8)?)?,
        updated_at: parse_time(row.get(9)?)?,
    })
}
