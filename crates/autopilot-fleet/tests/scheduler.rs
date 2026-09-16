use std::collections::BTreeSet;
use autopilot_domain::AgentRole;
use autopilot_fleet::{FleetError, FleetScheduler, FleetTask};

fn task(id: &str, deps: &[&str], writes: bool, workspace: &str) -> FleetTask {
    FleetTask { id: id.into(), role: AgentRole::Coder, dependencies: deps.iter().map(|s| (*s).into()).collect(), writes, workspace_id: workspace.into() }
}

#[test]
fn rejects_unknown_dependencies() {
    let error = FleetScheduler::new(vec![task("b", &["missing"], false, "w")]).unwrap_err();
    assert!(matches!(error, FleetError::UnknownDependency { .. }));
}

#[test]
fn rejects_cycles() {
    let error = FleetScheduler::new(vec![task("a", &["b"], false, "w"), task("b", &["a"], false, "w")]).unwrap_err();
    assert_eq!(error, FleetError::Cycle);
}

#[test]
fn returns_only_dependency_ready_tasks_in_stable_order() {
    let scheduler = FleetScheduler::new(vec![task("b", &["a"], false, "w2"), task("a", &[], false, "w1"), task("c", &[], false, "w3")]).unwrap();
    let ready = scheduler.ready(&BTreeSet::new(), &[]);
    assert_eq!(ready.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["a", "c"]);
}

#[test]
fn blocks_parallel_writers_to_same_workspace() {
    let scheduler = FleetScheduler::new(vec![task("a", &[], true, "repo"), task("b", &[], true, "repo"), task("c", &[], false, "repo")]).unwrap();
    let running = vec![task("external", &[], true, "repo")];
    let ready = scheduler.ready(&BTreeSet::new(), &running);
    assert_eq!(ready.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["c"]);
}
