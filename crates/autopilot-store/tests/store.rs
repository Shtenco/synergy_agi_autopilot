use autopilot_audit::AuditStore;
use autopilot_domain::{AuditEvent, CheckpointRecord, CorrelationId, MemoryRecord, MissionRecord, RiskClass};
use autopilot_store::SqliteStore;
use chrono::{Duration, Utc};

#[tokio::test]
async fn persists_audit_mission_checkpoint_and_memory() {
    let store = SqliteStore::open_in_memory().unwrap();
    let correlation = CorrelationId::new();
    store.append(AuditEvent::new(correlation, "Director", "Planning", RiskClass::Low, "entered")).await.unwrap();
    assert_eq!(store.list_by_correlation(correlation).await.unwrap().len(), 1);

    let mission = MissionRecord::queued("fix checkout", "C:/repo");
    store.insert_mission(&mission).unwrap();
    assert_eq!(store.get_mission(mission.id).unwrap().unwrap().objective, "fix checkout");

    let checkpoint = CheckpointRecord { mission_id: mission.id, phase: "WorktreeReady".into(), payload_json: "{\"worktree\":\"x\"}".into(), updated_at: Utc::now() };
    store.save_checkpoint(&checkpoint).unwrap();
    assert_eq!(store.load_checkpoint(mission.id).unwrap().unwrap().phase, "WorktreeReady");

    let memory = MemoryRecord { project: "C:/repo".into(), category: "convention".into(), key: "tests".into(), value_json: "{\"command\":\"cargo test\"}".into(), created_at: Utc::now(), updated_at: Utc::now(), expires_at: None };
    store.put_memory(&memory).unwrap();
    assert_eq!(store.search_memory("C:/repo", None).unwrap().len(), 1);
}

#[test]
fn expired_memory_is_not_returned() {
    let store = SqliteStore::open_in_memory().unwrap();
    let now = Utc::now();
    store.put_memory(&MemoryRecord { project: "p".into(), category: "temp".into(), key: "old".into(), value_json: "{}".into(), created_at: now, updated_at: now, expires_at: Some(now - Duration::seconds(1)) }).unwrap();
    assert!(store.search_memory("p", None).unwrap().is_empty());
}
