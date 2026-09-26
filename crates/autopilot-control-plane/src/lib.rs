use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

use autopilot_automation::{AutomationEvent, AutomationSpec, Trigger};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ControlPlaneError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("control-plane lock poisoned")]
    Poisoned,
    #[error("invalid uuid: {0}")]
    Uuid(String),
    #[error("invalid timestamp: {0}")]
    Timestamp(String),
    #[error("approval request not found")]
    ApprovalNotFound,
    #[error("approval request is already decided")]
    ApprovalAlreadyDecided,
    #[error("budget exceeded for {id}: requested={requested_microusd} remaining={remaining_microusd}")]
    BudgetExceeded { id: String, requested_microusd: i64, remaining_microusd: i64 },
}

pub type Result<T> = std::result::Result<T, ControlPlaneError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus { Pending, Approved, Denied }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub subject: String,
    pub action: Value,
    pub requested_at: DateTime<Utc>,
    pub status: ApprovalStatus,
    pub decided_at: Option<DateTime<Utc>>,
    pub decided_by: Option<String>,
    pub reason: Option<String>,
}

impl ApprovalRequest {
    pub fn pending(subject: impl Into<String>, action: Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            subject: subject.into(),
            action,
            requested_at: Utc::now(),
            status: ApprovalStatus::Pending,
            decided_at: None,
            decided_by: None,
            reason: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub multiplier: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 5, initial_delay_ms: 1_000, max_delay_ms: 60_000, multiplier: 2 }
    }
}

impl RetryPolicy {
    pub fn delay_ms(&self, failed_attempts: u32) -> Option<u64> {
        if failed_attempts >= self.max_attempts { return None; }
        let exponent = failed_attempts.saturating_sub(1).min(31);
        let factor = (self.multiplier as u64).saturating_pow(exponent);
        Some(self.initial_delay_ms.saturating_mul(factor).min(self.max_delay_ms))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EventLease {
    pub id: Uuid,
    pub topic: String,
    pub payload: Value,
    pub attempts: u32,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct ControlPlaneStore {
    connection: Arc<Mutex<Connection>>,
}

impl ControlPlaneStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { connection: Arc::new(Mutex::new(connection)) };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let store = Self { connection: Arc::new(Mutex::new(connection)) };
        store.migrate()?;
        Ok(store)
    }

    fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| ControlPlaneError::Poisoned)
    }

    pub fn migrate(&self) -> Result<()> {
        self.conn()?.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS automations (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                goal TEXT NOT NULL,
                trigger_json TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                last_fired_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS automation_events (
                id TEXT PRIMARY KEY,
                topic TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt_at TEXT,
                lease_owner TEXT,
                lease_until TEXT,
                error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_automation_events_due
                ON automation_events(status,next_attempt_at,occurred_at);
            CREATE TABLE IF NOT EXISTS approvals (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                action_json TEXT NOT NULL,
                requested_at TEXT NOT NULL,
                status TEXT NOT NULL,
                decided_at TEXT,
                decided_by TEXT,
                reason TEXT
            );
            CREATE TABLE IF NOT EXISTS budgets (
                id TEXT NOT NULL,
                period_day TEXT NOT NULL,
                limit_microusd INTEGER NOT NULL,
                spent_microusd INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(id, period_day)
            );
            "#,
        )?;
        Ok(())
    }

    pub fn put_automation(&self, spec: &AutomationSpec) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn()?.execute(
            r#"INSERT INTO automations(id,name,goal,trigger_json,enabled,created_at,updated_at)
               VALUES(?1,?2,?3,?4,?5,?6,?6)
               ON CONFLICT(id) DO UPDATE SET
                 name=excluded.name,goal=excluded.goal,trigger_json=excluded.trigger_json,
                 enabled=excluded.enabled,updated_at=excluded.updated_at"#,
            params![
                spec.id.to_string(),
                spec.name,
                spec.goal,
                serde_json::to_string(&spec.trigger)?,
                if spec.enabled { 1i64 } else { 0i64 },
                now,
            ],
        )?;
        Ok(())
    }

    pub fn list_automations(&self) -> Result<Vec<AutomationSpec>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(
            "SELECT id,name,goal,trigger_json,enabled FROM automations ORDER BY created_at,id"
        )?;
        let rows = statement.query_map([], |row| {
            let id_text: String = row.get(0)?;
            let trigger_text: String = row.get(3)?;
            let id = parse_uuid_sql(&id_text)?;
            let trigger: Trigger = serde_json::from_str(&trigger_text).map_err(json_sql_error)?;
            Ok(AutomationSpec {
                id,
                name: row.get(1)?,
                goal: row.get(2)?,
                trigger,
                enabled: row.get::<_, i64>(4)? != 0,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn mark_automation_fired(&self, id: Uuid, at: DateTime<Utc>) -> Result<()> {
        self.conn()?.execute(
            "UPDATE automations SET last_fired_at=?2,updated_at=?2 WHERE id=?1",
            params![id.to_string(), at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn due_automations(&self, now: DateTime<Utc>) -> Result<Vec<AutomationSpec>> {
        let conn = self.conn()?;
        let mut statement = conn.prepare(
            "SELECT id,name,goal,trigger_json,enabled,last_fired_at FROM automations WHERE enabled=1 ORDER BY created_at,id"
        )?;
        let rows = statement.query_map([], |row| {
            let id_text: String = row.get(0)?;
            let trigger_text: String = row.get(3)?;
            let last: Option<String> = row.get(5)?;
            Ok((
                AutomationSpec {
                    id: parse_uuid_sql(&id_text)?,
                    name: row.get(1)?,
                    goal: row.get(2)?,
                    trigger: serde_json::from_str(&trigger_text).map_err(json_sql_error)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                },
                last.map(|value| parse_time_sql(&value)).transpose()?,
            ))
        })?;
        let mut due = Vec::new();
        for row in rows {
            let (spec, last_fired) = row?;
            let ready = match &spec.trigger {
                Trigger::Once { at } => at <= &now && last_fired.is_none(),
                Trigger::Interval { every_seconds, start_at } => {
                    *every_seconds > 0
                        && start_at.as_ref().is_none_or(|start| start <= &now)
                        && last_fired.as_ref().is_none_or(|last| {
                            last.clone() + Duration::seconds(*every_seconds as i64) <= now
                        })
                }
                Trigger::Manual | Trigger::Webhook { .. } | Trigger::Event { .. } => false,
            };
            if ready { due.push(spec); }
        }
        Ok(due)
    }

    pub fn matching_event_automations(&self, event: &AutomationEvent) -> Result<Vec<AutomationSpec>> {
        let mut matched = Vec::new();
        for spec in self.list_automations()? {
            if !spec.enabled { continue; }
            let matches = match &spec.trigger {
                Trigger::Webhook { topic } => topic == &event.topic,
                Trigger::Event { topic, equals } => {
                    topic == &event.topic && equals.iter().all(|(key, expected)| event.payload.get(key) == Some(expected))
                }
                _ => false,
            };
            if matches { matched.push(spec); }
        }
        Ok(matched)
    }

    pub fn enqueue_event(&self, event: &AutomationEvent) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.conn()?.execute(
            "INSERT INTO automation_events(id,topic,payload_json,occurred_at,status,attempts,next_attempt_at) VALUES(?1,?2,?3,?4,'pending',0,?4)",
            params![
                id.to_string(),
                event.topic,
                serde_json::to_string(&event.payload)?,
                event.occurred_at.to_rfc3339(),
            ],
        )?;
        Ok(id)
    }

    pub fn lease_event(&self, owner: &str, now: DateTime<Utc>, lease_seconds: i64) -> Result<Option<EventLease>> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let row = tx.query_row(
            r#"SELECT id,topic,payload_json,attempts,occurred_at
               FROM automation_events
               WHERE status='pending' AND (next_attempt_at IS NULL OR next_attempt_at<=?1)
               ORDER BY occurred_at,id LIMIT 1"#,
            [now.to_rfc3339()],
            |row| {
                let id_text: String = row.get(0)?;
                let payload_text: String = row.get(2)?;
                Ok(EventLease {
                    id: parse_uuid_sql(&id_text)?,
                    topic: row.get(1)?,
                    payload: serde_json::from_str(&payload_text).map_err(json_sql_error)?,
                    attempts: row.get::<_, i64>(3)? as u32,
                    occurred_at: parse_time_sql(&row.get::<_, String>(4)?)?,
                })
            },
        ).optional()?;
        let Some(mut lease) = row else { tx.commit()?; return Ok(None); };
        lease.attempts = lease.attempts.saturating_add(1);
        tx.execute(
            "UPDATE automation_events SET status='leased',attempts=?2,lease_owner=?3,lease_until=?4 WHERE id=?1",
            params![
                lease.id.to_string(),
                lease.attempts as i64,
                owner,
                (now + Duration::seconds(lease_seconds.max(1))).to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(Some(lease))
    }

    pub fn complete_event(&self, id: Uuid) -> Result<()> {
        self.conn()?.execute(
            "UPDATE automation_events SET status='completed',lease_owner=NULL,lease_until=NULL,error=NULL WHERE id=?1",
            [id.to_string()],
        )?;
        Ok(())
    }

    pub fn retry_event(&self, lease: &EventLease, policy: RetryPolicy, now: DateTime<Utc>, error: &str) -> Result<bool> {
        let Some(delay_ms) = policy.delay_ms(lease.attempts) else {
            self.conn()?.execute(
                "UPDATE automation_events SET status='dead',lease_owner=NULL,lease_until=NULL,error=?2 WHERE id=?1",
                params![lease.id.to_string(), error],
            )?;
            return Ok(false);
        };
        let next = now + Duration::milliseconds(delay_ms as i64);
        self.conn()?.execute(
            "UPDATE automation_events SET status='pending',next_attempt_at=?2,lease_owner=NULL,lease_until=NULL,error=?3 WHERE id=?1",
            params![lease.id.to_string(), next.to_rfc3339(), error],
        )?;
        Ok(true)
    }

    pub fn recover_expired_event_leases(&self, now: DateTime<Utc>) -> Result<usize> {
        let changed = self.conn()?.execute(
            "UPDATE automation_events SET status='pending',lease_owner=NULL,lease_until=NULL WHERE status='leased' AND lease_until<?1",
            [now.to_rfc3339()],
        )?;
        Ok(changed)
    }

    pub fn request_approval(&self, request: &ApprovalRequest) -> Result<()> {
        self.conn()?.execute(
            "INSERT INTO approvals(id,subject,action_json,requested_at,status) VALUES(?1,?2,?3,?4,'pending')",
            params![
                request.id.to_string(),
                request.subject,
                serde_json::to_string(&request.action)?,
                request.requested_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn decide_approval(&self, id: Uuid, approved: bool, actor: &str, reason: Option<&str>) -> Result<ApprovalRequest> {
        let mut request = self.get_approval(id)?.ok_or(ControlPlaneError::ApprovalNotFound)?;
        if request.status != ApprovalStatus::Pending { return Err(ControlPlaneError::ApprovalAlreadyDecided); }
        let now = Utc::now();
        request.status = if approved { ApprovalStatus::Approved } else { ApprovalStatus::Denied };
        request.decided_at = Some(now);
        request.decided_by = Some(actor.to_string());
        request.reason = reason.map(str::to_string);
        self.conn()?.execute(
            "UPDATE approvals SET status=?2,decided_at=?3,decided_by=?4,reason=?5 WHERE id=?1",
            params![
                id.to_string(),
                if approved { "approved" } else { "denied" },
                now.to_rfc3339(),
                actor,
                reason,
            ],
        )?;
        Ok(request)
    }

    pub fn get_approval(&self, id: Uuid) -> Result<Option<ApprovalRequest>> {
        self.conn()?.query_row(
            "SELECT id,subject,action_json,requested_at,status,decided_at,decided_by,reason FROM approvals WHERE id=?1",
            [id.to_string()],
            approval_from_row,
        ).optional().map_err(ControlPlaneError::from)
    }

    pub fn set_daily_budget(&self, id: &str, day: NaiveDate, limit_microusd: i64) -> Result<()> {
        self.conn()?.execute(
            r#"INSERT INTO budgets(id,period_day,limit_microusd,spent_microusd)
               VALUES(?1,?2,?3,0)
               ON CONFLICT(id,period_day) DO UPDATE SET limit_microusd=excluded.limit_microusd"#,
            params![id, day.to_string(), limit_microusd.max(0)],
        )?;
        Ok(())
    }

    pub fn reserve_budget(&self, id: &str, day: NaiveDate, amount_microusd: i64) -> Result<i64> {
        let amount = amount_microusd.max(0);
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let (limit, spent): (i64, i64) = tx.query_row(
            "SELECT limit_microusd,spent_microusd FROM budgets WHERE id=?1 AND period_day=?2",
            params![id, day.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let remaining = limit.saturating_sub(spent);
        if amount > remaining {
            return Err(ControlPlaneError::BudgetExceeded {
                id: id.to_string(), requested_microusd: amount, remaining_microusd: remaining,
            });
        }
        tx.execute(
            "UPDATE budgets SET spent_microusd=spent_microusd+?3 WHERE id=?1 AND period_day=?2",
            params![id, day.to_string(), amount],
        )?;
        tx.commit()?;
        Ok(remaining - amount)
    }
}

fn approval_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalRequest> {
    let id_text: String = row.get(0)?;
    let action_text: String = row.get(2)?;
    let status_text: String = row.get(4)?;
    let status = match status_text.as_str() {
        "pending" => ApprovalStatus::Pending,
        "approved" => ApprovalStatus::Approved,
        "denied" => ApprovalStatus::Denied,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(ApprovalRequest {
        id: parse_uuid_sql(&id_text)?,
        subject: row.get(1)?,
        action: serde_json::from_str(&action_text).map_err(json_sql_error)?,
        requested_at: parse_time_sql(&row.get::<_, String>(3)?)?,
        status,
        decided_at: row.get::<_, Option<String>>(5)?.map(|v| parse_time_sql(&v)).transpose()?,
        decided_by: row.get(6)?,
        reason: row.get(7)?,
    })
}

fn parse_uuid_sql(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::from_str(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(value.len(), rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_time_sql(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(value.len(), rusqlite::types::Type::Text, Box::new(error)))
}

fn json_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn persistent_interval_and_event_matching_work() {
        let store = ControlPlaneStore::open_in_memory().unwrap();
        let now = Utc::now();
        let interval = AutomationSpec::new(
            "hourly", "report", Trigger::Interval { every_seconds: 3600, start_at: None },
        );
        store.put_automation(&interval).unwrap();
        assert_eq!(store.due_automations(now).unwrap().len(), 1);
        store.mark_automation_fired(interval.id, now).unwrap();
        assert!(store.due_automations(now + Duration::seconds(10)).unwrap().is_empty());

        let mut equals = BTreeMap::new();
        equals.insert("status".into(), Value::String("paid".into()));
        let event_spec = AutomationSpec::new(
            "paid", "receipt", Trigger::Event { topic: "invoice.updated".into(), equals },
        );
        store.put_automation(&event_spec).unwrap();
        let mut payload = BTreeMap::new();
        payload.insert("status".into(), Value::String("paid".into()));
        let event = AutomationEvent::new("invoice.updated", payload);
        assert_eq!(store.matching_event_automations(&event).unwrap().len(), 1);
    }

    #[test]
    fn event_retry_backoff_and_dead_letter_are_persistent() {
        let store = ControlPlaneStore::open_in_memory().unwrap();
        let now = Utc::now();
        let event = AutomationEvent::new("x", BTreeMap::new());
        store.enqueue_event(&event).unwrap();
        let lease = store.lease_event("worker-1", now, 30).unwrap().unwrap();
        assert_eq!(lease.attempts, 1);
        let policy = RetryPolicy { max_attempts: 2, initial_delay_ms: 100, max_delay_ms: 100, multiplier: 2 };
        assert!(store.retry_event(&lease, policy, now, "boom").unwrap());
        let lease2 = store.lease_event("worker-1", now + Duration::milliseconds(100), 30).unwrap().unwrap();
        assert_eq!(lease2.attempts, 2);
        assert!(!store.retry_event(&lease2, policy, now, "boom2").unwrap());
    }

    #[test]
    fn approvals_and_budget_fail_closed() {
        let store = ControlPlaneStore::open_in_memory().unwrap();
        let request = ApprovalRequest::pending("send email", serde_json::json!({"to":"a@example.com"}));
        store.request_approval(&request).unwrap();
        let decided = store.decide_approval(request.id, true, "human", Some("ok")).unwrap();
        assert_eq!(decided.status, ApprovalStatus::Approved);

        let day = Utc::now().date_naive();
        store.set_daily_budget("paid-api", day, 1_000_000).unwrap();
        assert_eq!(store.reserve_budget("paid-api", day, 400_000).unwrap(), 600_000);
        assert!(matches!(
            store.reserve_budget("paid-api", day, 700_000),
            Err(ControlPlaneError::BudgetExceeded { .. })
        ));
    }
}
