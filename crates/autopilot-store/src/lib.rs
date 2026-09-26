use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use async_trait::async_trait;
use autopilot_audit::{AuditResult, AuditStore};
use autopilot_domain::{
    AuditEvent, CheckpointRecord, CorrelationId, MemoryRecord, MissionId, MissionRecord, MissionState,
    RiskClass,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid stored uuid: {0}")]
    Uuid(String),
    #[error("invalid stored timestamp: {0}")]
    Timestamp(String),
    #[error("store lock poisoned")]
    Poisoned,
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Clone)]
pub struct SqliteStore {
    connection: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { connection: Arc::new(Mutex::new(connection)) };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> StoreResult<Self> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { connection: Arc::new(Mutex::new(connection)) };
        store.migrate()?;
        Ok(store)
    }

    fn conn(&self) -> StoreResult<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| StoreError::Poisoned)
    }

    pub fn migrate(&self) -> StoreResult<()> {
        self.conn()?.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS audit_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                correlation_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                actor TEXT NOT NULL,
                event_type TEXT NOT NULL,
                risk_json TEXT NOT NULL,
                status TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_audit_correlation
                ON audit_events(correlation_id, timestamp, seq);

            CREATE TABLE IF NOT EXISTS missions (
                id TEXT PRIMARY KEY,
                objective TEXT NOT NULL,
                project TEXT NOT NULL,
                state_json TEXT NOT NULL,
                phase TEXT NOT NULL,
                attempt INTEGER NOT NULL,
                lease_owner TEXT,
                lease_until TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_missions_state_created
                ON missions(state_json, created_at);
            CREATE INDEX IF NOT EXISTS idx_missions_project
                ON missions(project, updated_at DESC);

            CREATE TABLE IF NOT EXISTS mission_events (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                mission_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                FOREIGN KEY(mission_id) REFERENCES missions(id)
            );

            CREATE TABLE IF NOT EXISTS checkpoints (
                mission_id TEXT PRIMARY KEY,
                phase TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY(mission_id) REFERENCES missions(id)
            );

            CREATE TABLE IF NOT EXISTS memory_records (
                project TEXT NOT NULL,
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                value_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                expires_at TEXT,
                PRIMARY KEY(project, category, key)
            );
            CREATE INDEX IF NOT EXISTS idx_memory_project_updated
                ON memory_records(project, updated_at DESC);

            INSERT OR IGNORE INTO schema_migrations(version, applied_at)
            VALUES (1, CURRENT_TIMESTAMP);
            "#,
        )?;
        Ok(())
    }

    pub fn insert_mission(&self, mission: &MissionRecord) -> StoreResult<()> {
        self.conn()?.execute(
            "INSERT INTO missions(id, objective, project, state_json, phase, attempt, lease_owner, lease_until, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                mission.id.0.to_string(), mission.objective, mission.project,
                serde_json::to_string(&mission.state)?, mission.phase, mission.attempt,
                mission.lease_owner, mission.lease_until.map(|v| v.to_rfc3339()),
                mission.created_at.to_rfc3339(), mission.updated_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn update_mission(&self, mission: &MissionRecord) -> StoreResult<()> {
        self.conn()?.execute(
            "UPDATE missions SET objective=?2, project=?3, state_json=?4, phase=?5, attempt=?6, lease_owner=?7, lease_until=?8, updated_at=?9 WHERE id=?1",
            params![
                mission.id.0.to_string(), mission.objective, mission.project,
                serde_json::to_string(&mission.state)?, mission.phase, mission.attempt,
                mission.lease_owner, mission.lease_until.map(|v| v.to_rfc3339()), mission.updated_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_mission(&self, id: MissionId) -> StoreResult<Option<MissionRecord>> {
        self.conn()?.query_row(
            "SELECT id, objective, project, state_json, phase, attempt, lease_owner, lease_until, created_at, updated_at FROM missions WHERE id=?1",
            [id.0.to_string()],
            mission_from_row,
        ).optional().map_err(StoreError::from)
    }

    pub fn list_missions(&self) -> StoreResult<Vec<MissionRecord>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare("SELECT id, objective, project, state_json, phase, attempt, lease_owner, lease_until, created_at, updated_at FROM missions ORDER BY created_at, id")?;
        let rows = statement.query_map([], mission_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
    }

    pub fn transaction<T>(&self, f: impl FnOnce(&Transaction<'_>) -> StoreResult<T>) -> StoreResult<T> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    pub fn save_checkpoint(&self, checkpoint: &CheckpointRecord) -> StoreResult<()> {
        self.conn()?.execute(
            "INSERT INTO checkpoints(mission_id,phase,payload_json,updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(mission_id) DO UPDATE SET phase=excluded.phase,payload_json=excluded.payload_json,updated_at=excluded.updated_at",
            params![checkpoint.mission_id.0.to_string(), checkpoint.phase, checkpoint.payload_json, checkpoint.updated_at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn load_checkpoint(&self, mission_id: MissionId) -> StoreResult<Option<CheckpointRecord>> {
        self.conn()?.query_row(
            "SELECT mission_id,phase,payload_json,updated_at FROM checkpoints WHERE mission_id=?1",
            [mission_id.0.to_string()],
            |row| Ok(CheckpointRecord {
                mission_id: MissionId(parse_uuid(row.get::<_, String>(0)?)?),
                phase: row.get(1)?, payload_json: row.get(2)?,
                updated_at: parse_time(row.get::<_, String>(3)?)?,
            }),
        ).optional().map_err(StoreError::from)
    }

    pub fn put_memory(&self, memory: &MemoryRecord) -> StoreResult<()> {
        self.conn()?.execute(
            "INSERT INTO memory_records(project,category,key,value_json,created_at,updated_at,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project,category,key) DO UPDATE SET value_json=excluded.value_json,updated_at=excluded.updated_at,expires_at=excluded.expires_at",
            params![memory.project, memory.category, memory.key, memory.value_json, memory.created_at.to_rfc3339(), memory.updated_at.to_rfc3339(), memory.expires_at.map(|v| v.to_rfc3339())],
        )?;
        Ok(())
    }

    pub fn search_memory(&self, project: &str, category: Option<&str>) -> StoreResult<Vec<MemoryRecord>> {
        let conn = self.conn()?;
        let sql = if category.is_some() {
            "SELECT project,category,key,value_json,created_at,updated_at,expires_at FROM memory_records WHERE project=?1 AND category=?2 ORDER BY updated_at DESC"
        } else {
            "SELECT project,category,key,value_json,created_at,updated_at,expires_at FROM memory_records WHERE project=?1 ORDER BY updated_at DESC"
        };
        let mut statement = conn.prepare(sql)?;
        let now = Utc::now();
        let mut result = Vec::new();
        let mut rows = if let Some(category) = category { statement.query(params![project, category])? } else { statement.query(params![project])? };
        while let Some(row) = rows.next()? {
            let record = MemoryRecord {
                project: row.get(0)?, category: row.get(1)?, key: row.get(2)?, value_json: row.get(3)?,
                created_at: parse_time(row.get::<_, String>(4)?)?, updated_at: parse_time(row.get::<_, String>(5)?)?,
                expires_at: row.get::<_, Option<String>>(6)?.map(parse_time).transpose()?,
            };
            if record.expires_at.map(|t| t > now).unwrap_or(true) { result.push(record); }
        }
        Ok(result)
    }

    pub fn append_mission_event(&self, mission_id: MissionId, event_type: &str, payload_json: &str) -> StoreResult<()> {
        self.conn()?.execute(
            "INSERT INTO mission_events(mission_id,timestamp,event_type,payload_json) VALUES (?1,?2,?3,?4)",
            params![mission_id.0.to_string(), Utc::now().to_rfc3339(), event_type, payload_json],
        )?;
        Ok(())
    }
}

#[async_trait]
impl AuditStore for SqliteStore {
    async fn append(&self, event: AuditEvent) -> AuditResult<()> {
        let result = self.conn().and_then(|conn| {
            conn.execute(
                "INSERT INTO audit_events(id,correlation_id,timestamp,actor,event_type,risk_json,status) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![event.id.to_string(), event.correlation_id.0.to_string(), event.timestamp.to_rfc3339(), event.actor, event.event_type, serde_json::to_string(&event.risk)?, event.status],
            )?;
            Ok(())
        });
        result.map_err(|error| error.to_string())
    }

    async fn list_by_correlation(&self, correlation: CorrelationId) -> AuditResult<Vec<AuditEvent>> {
        let result = self.conn().and_then(|conn| {
            let mut statement = conn.prepare("SELECT id,correlation_id,timestamp,actor,event_type,risk_json,status FROM audit_events WHERE correlation_id=?1 ORDER BY timestamp,seq")?;
            let rows = statement.query_map([correlation.0.to_string()], |row| {
                let id = parse_uuid(row.get::<_, String>(0)?)?;
                let correlation_id = CorrelationId(parse_uuid(row.get::<_, String>(1)?)?);
                let timestamp = parse_time(row.get::<_, String>(2)?)?;
                let risk: RiskClass = serde_json::from_str(&row.get::<_, String>(5)?).map_err(json_sql_error)?;
                Ok(AuditEvent { id, correlation_id, timestamp, actor: row.get(3)?, event_type: row.get(4)?, risk, status: row.get(6)? })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
        });
        result.map_err(|error| error.to_string())
    }
}

fn mission_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MissionRecord> {
    let id = MissionId(parse_uuid(row.get::<_, String>(0)?)?);
    let state: MissionState = serde_json::from_str(&row.get::<_, String>(3)?).map_err(json_sql_error)?;
    Ok(MissionRecord {
        id, objective: row.get(1)?, project: row.get(2)?, state, phase: row.get(4)?,
        attempt: row.get::<_, i64>(5)? as u32, lease_owner: row.get(6)?,
        lease_until: row.get::<_, Option<String>>(7)?.map(parse_time).transpose()?,
        created_at: parse_time(row.get::<_, String>(8)?)?, updated_at: parse_time(row.get::<_, String>(9)?)?,
    })
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::from_str(&value).map_err(|error| rusqlite::Error::FromSqlConversionFailure(value.len(), rusqlite::types::Type::Text, Box::new(error)))
}

fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(value.len(), rusqlite::types::Type::Text, Box::new(error)))
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
