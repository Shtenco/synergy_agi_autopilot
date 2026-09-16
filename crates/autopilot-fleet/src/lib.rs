use std::collections::{btree_map::Entry, BTreeMap, BTreeSet, VecDeque};
use autopilot_domain::AgentRole;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FleetTask {
    pub id: String,
    pub role: AgentRole,
    pub dependencies: Vec<String>,
    pub writes: bool,
    pub workspace_id: String,
}

#[derive(Clone, Debug)]
pub struct FleetScheduler {
    tasks: BTreeMap<String, FleetTask>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum FleetError {
    #[error("duplicate task id: {0}")]
    DuplicateTask(String),
    #[error("task {task} depends on unknown task {dependency}")]
    UnknownDependency { task: String, dependency: String },
    #[error("task dependency graph contains a cycle")]
    Cycle,
}

impl FleetScheduler {
    pub fn new(tasks: Vec<FleetTask>) -> Result<Self, FleetError> {
        let mut map = BTreeMap::new();
        for task in tasks {
            let id = task.id.clone();
            match map.entry(id.clone()) {
                Entry::Vacant(slot) => {
                    slot.insert(task);
                }
                Entry::Occupied(_) => return Err(FleetError::DuplicateTask(id)),
            }
        }
        for task in map.values() {
            for dependency in &task.dependencies {
                if !map.contains_key(dependency) {
                    return Err(FleetError::UnknownDependency {
                        task: task.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        if Self::has_cycle(&map) {
            return Err(FleetError::Cycle);
        }
        Ok(Self { tasks: map })
    }

    fn has_cycle(tasks: &BTreeMap<String, FleetTask>) -> bool {
        let mut indegree: BTreeMap<String, usize> = tasks.keys().map(|id| (id.clone(), 0)).collect();
        let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for task in tasks.values() {
            for dependency in &task.dependencies {
                *indegree.get_mut(&task.id).expect("task is indexed") += 1;
                dependents.entry(dependency.clone()).or_default().push(task.id.clone());
            }
        }
        let mut queue: VecDeque<String> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(id.clone()))
            .collect();
        let mut visited = 0usize;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            if let Some(children) = dependents.get(&id) {
                for child in children {
                    let degree = indegree.get_mut(child).expect("dependent is indexed");
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(child.clone());
                    }
                }
            }
        }
        visited != tasks.len()
    }

    pub fn ready(&self, completed: &BTreeSet<String>, running: &[FleetTask]) -> Vec<FleetTask> {
        let running_ids: BTreeSet<&str> = running.iter().map(|task| task.id.as_str()).collect();
        let mut active_writers: BTreeSet<String> = running
            .iter()
            .filter(|task| task.writes)
            .map(|task| task.workspace_id.clone())
            .collect();
        let mut active_readers: BTreeSet<String> = running
            .iter()
            .filter(|task| !task.writes)
            .map(|task| task.workspace_id.clone())
            .collect();

        let mut ready = Vec::new();
        for task in self.tasks.values() {
            if completed.contains(&task.id) || running_ids.contains(task.id.as_str()) {
                continue;
            }
            if !task.dependencies.iter().all(|dependency| completed.contains(dependency)) {
                continue;
            }

            if task.writes {
                if active_writers.contains(&task.workspace_id) || active_readers.contains(&task.workspace_id) {
                    continue;
                }
                active_writers.insert(task.workspace_id.clone());
            } else {
                if active_writers.contains(&task.workspace_id) {
                    continue;
                }
                active_readers.insert(task.workspace_id.clone());
            }

            ready.push(task.clone());
        }
        ready
    }
}
