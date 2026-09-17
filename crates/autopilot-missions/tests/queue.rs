use autopilot_domain::MissionState;
use autopilot_missions::MissionQueue;
use autopilot_store::SqliteStore;
use chrono::Duration;

#[test]
fn mission_queue_leases_and_completes_exclusively() {
    let store = SqliteStore::open_in_memory().unwrap();
    let queue = MissionQueue::new(store, Duration::minutes(5));
    let mission = queue.enqueue("fix bug", "C:/repo").unwrap();
    let lease = queue.lease_next("worker-a").unwrap().unwrap();
    assert_eq!(lease.mission.id, mission.id);
    assert_eq!(lease.mission.state, MissionState::Leased);
    assert!(queue.lease_next("worker-b").unwrap().is_none());
    queue.start(mission.id, "worker-a", "Planning").unwrap();
    queue.checkpoint(mission.id, "worker-a", "WorktreeReady", "{}").unwrap();
    let completed = queue.complete(mission.id, "worker-a").unwrap();
    assert_eq!(completed.state, MissionState::Completed);
}

#[test]
fn expired_lease_is_recovered_without_losing_checkpoint() {
    let store = SqliteStore::open_in_memory().unwrap();
    let queue = MissionQueue::new(store.clone(), Duration::seconds(-1));
    let mission = queue.enqueue("resume me", "C:/repo").unwrap();
    queue.lease_next("dead-worker").unwrap().unwrap();
    queue.start(mission.id, "dead-worker", "Executing").unwrap();
    queue.checkpoint(mission.id, "dead-worker", "Executing", "{\"step\":3}").unwrap();
    let recovered = queue.recover_expired().unwrap();
    assert_eq!(recovered, vec![mission.id]);
    assert_eq!(store.get_mission(mission.id).unwrap().unwrap().state, MissionState::Queued);
    assert_eq!(store.load_checkpoint(mission.id).unwrap().unwrap().payload_json, "{\"step\":3}");
}
