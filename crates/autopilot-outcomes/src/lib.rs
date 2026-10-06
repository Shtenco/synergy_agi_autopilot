use std::path::Path;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OutcomeRecord {
    pub id: Uuid,
    pub goal_fingerprint: String,
    pub route_key: String,
    pub employee_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub device: Option<String>,
    pub success: bool,
    pub verified: bool,
    pub latency_ms: u64,
    pub cost_microusd: i64,
    pub error_class: Option<String>,
    pub quality_score: f64,
    pub created_at: DateTime<Utc>,
}

impl OutcomeRecord {
    pub fn new(goal_fingerprint: impl Into<String>, route_key: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            goal_fingerprint: goal_fingerprint.into(),
            route_key: route_key.into(),
            employee_id: None,
            provider: None,
            model: None,
            device: None,
            success: false,
            verified: false,
            latency_ms: 0,
            cost_microusd: 0,
            error_class: None,
            quality_score: 0.0,
            created_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RouteStats {
    pub route_key: String,
    pub runs: u64,
    pub successes: u64,
    pub verified_successes: u64,
    pub avg_latency_ms: f64,
    pub avg_cost_microusd: f64,
    pub avg_quality_score: f64,
    pub score: f64,
}

#[derive(Debug, Error)]
pub enum OutcomeError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("outcome store lock poisoned")]
    Poisoned,
}

#[derive(Clone)]
pub struct OutcomeStore {
    conn: Arc<Mutex<Connection>>,
}

impl OutcomeStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OutcomeError> {
        let conn = Connection::open(path)?;
        let store = Self { conn: Arc::new(Mutex::new(conn)) };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, OutcomeError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn: Arc::new(Mutex::new(conn)) };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), OutcomeError> {
        self.conn.lock().map_err(|_| OutcomeError::Poisoned)?.execute_batch(r#"
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS outcomes (
                id TEXT PRIMARY KEY,
                goal_fingerprint TEXT NOT NULL,
                route_key TEXT NOT NULL,
                employee_id TEXT,
                provider TEXT,
                model TEXT,
                device TEXT,
                success INTEGER NOT NULL,
                verified INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                cost_microusd INTEGER NOT NULL,
                error_class TEXT,
                quality_score REAL NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_outcomes_goal_route
                ON outcomes(goal_fingerprint, route_key, created_at DESC);
        "#)?;
        Ok(())
    }

    pub fn record(&self, value: &OutcomeRecord) -> Result<(), OutcomeError> {
        self.conn.lock().map_err(|_| OutcomeError::Poisoned)?.execute(
            "INSERT INTO outcomes(id,goal_fingerprint,route_key,employee_id,provider,model,device,success,verified,latency_ms,cost_microusd,error_class,quality_score,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                value.id.to_string(), value.goal_fingerprint, value.route_key, value.employee_id,
                value.provider, value.model, value.device, value.success as i64, value.verified as i64,
                value.latency_ms as i64, value.cost_microusd, value.error_class, value.quality_score,
                value.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn stats(&self, goal_fingerprint: &str) -> Result<Vec<RouteStats>, OutcomeError> {
        let conn = self.conn.lock().map_err(|_| OutcomeError::Poisoned)?;
        let mut stmt = conn.prepare(
            "SELECT route_key,
                    COUNT(*) runs,
                    SUM(success) successes,
                    SUM(CASE WHEN success=1 AND verified=1 THEN 1 ELSE 0 END) verified_successes,
                    AVG(latency_ms),
                    AVG(cost_microusd),
                    AVG(quality_score)
             FROM outcomes WHERE goal_fingerprint=?1
             GROUP BY route_key ORDER BY route_key"
        )?;
        let rows = stmt.query_map([goal_fingerprint], |row| {
            let runs = row.get::<_, i64>(1)? as u64;
            let successes = row.get::<_, i64>(2)? as u64;
            let verified_successes = row.get::<_, i64>(3)? as u64;
            let latency = row.get::<_, f64>(4)?;
            let cost = row.get::<_, f64>(5)?;
            let quality = row.get::<_, f64>(6)?;
            let reliability = if runs == 0 { 0.0 } else { verified_successes as f64 / runs as f64 };
            let latency_penalty = (latency / 60_000.0).min(1.0);
            let cost_penalty = (cost / 1_000_000.0).min(1.0);
            let score = reliability * 0.65 + quality.clamp(0.0, 1.0) * 0.25
                + (1.0 - latency_penalty) * 0.05 + (1.0 - cost_penalty) * 0.05;
            Ok(RouteStats {
                route_key: row.get(0)?, runs, successes, verified_successes,
                avg_latency_ms: latency, avg_cost_microusd: cost,
                avg_quality_score: quality, score,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(OutcomeError::from)
    }

    pub fn best_route(&self, goal_fingerprint: &str, min_runs: u64) -> Result<Option<RouteStats>, OutcomeError> {
        let mut values = self.stats(goal_fingerprint)?;
        values.retain(|item| item.runs >= min_runs);
        values.sort_by(|a,b| b.score.total_cmp(&a.score).then_with(|| a.route_key.cmp(&b.route_key)));
        Ok(values.into_iter().next())
    }

    pub fn best_route_among(
        &self,
        goal_fingerprint: &str,
        candidates: &[String],
        min_runs: u64,
    ) -> Result<Option<RouteStats>, OutcomeError> {
        let allowed = candidates.iter().collect::<std::collections::BTreeSet<_>>();
        let mut values = self.stats(goal_fingerprint)?;
        values.retain(|item| item.runs >= min_runs && allowed.contains(&item.route_key));
        values.sort_by(|a,b| b.score.total_cmp(&a.score).then_with(|| a.route_key.cmp(&b.route_key)));
        Ok(values.into_iter().next())
    }
}

pub fn fingerprint_goal(goal: &str) -> String {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let normalized = goal.split_whitespace()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("goal:{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_success_beats_fast_unverified_route() {
        let store = OutcomeStore::open_in_memory().unwrap();
        for _ in 0..3 {
            let mut safe = OutcomeRecord::new("goal-a","safe");
            safe.success = true; safe.verified = true; safe.latency_ms = 2500; safe.cost_microusd = 2000; safe.quality_score = 0.9;
            store.record(&safe).unwrap();

            let mut fast = OutcomeRecord::new("goal-a","fast");
            fast.success = true; fast.verified = false; fast.latency_ms = 100; fast.cost_microusd = 10; fast.quality_score = 0.7;
            store.record(&fast).unwrap();
        }
        assert_eq!(store.best_route("goal-a",2).unwrap().unwrap().route_key,"safe");
    }

    #[test]
    fn insufficient_history_is_not_promoted() {
        let store = OutcomeStore::open_in_memory().unwrap();
        let mut value=OutcomeRecord::new("goal-b","one-shot");
        value.success=true; value.verified=true; value.quality_score=1.0;
        store.record(&value).unwrap();
        assert!(store.best_route("goal-b",2).unwrap().is_none());
    }
}
