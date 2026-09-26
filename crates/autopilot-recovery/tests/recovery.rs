use autopilot_domain::{MissionRecord};
use autopilot_recovery::{MissionCheckpoint, RecoveryDecision, RecoveryManager, RecoveryPhase};
use autopilot_store::SqliteStore;

#[test]
fn planned_checkpoint_restarts_fleet_after_process_restart() {
    let store = SqliteStore::open_in_memory().unwrap();
    let mission = MissionRecord::queued("goal", "project");
    store.insert_mission(&mission).unwrap();
    let manager = RecoveryManager::new(store);
    let checkpoint = MissionCheckpoint::planned();
    manager.save(mission.id, &checkpoint).unwrap();
    let loaded = manager.load(mission.id).unwrap().unwrap();
    assert_eq!(loaded.phase, RecoveryPhase::Planned);
    assert_eq!(manager.decide(&loaded), RecoveryDecision::RestartFleet);
}

#[test]
fn missing_worktree_fails_closed_instead_of_replaying_publish() {
    let store = SqliteStore::open_in_memory().unwrap();
    let manager = RecoveryManager::new(store);
    let checkpoint = MissionCheckpoint {
        phase: RecoveryPhase::Published,
        worktree: Some("Z:/definitely/missing/open-autopilot-worktree".into()),
        correlation_id: None,
        repairs: 0,
        ci_repairs: 0,
        pr_url: Some("https://github.com/o/r/pull/1".into()),
        metadata: serde_json::json!({}),
    };
    assert!(matches!(manager.decide(&checkpoint), RecoveryDecision::FailMissingWorktree(_)));
}
