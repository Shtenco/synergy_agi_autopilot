use std::collections::BTreeMap;
use std::path::Path;

use autopilot_automation::{AutomationEvent, AutomationSpec};
use autopilot_control_plane::{ControlPlaneError, ControlPlaneStore, EventLease, RetryPolicy};
use autopilot_domain::MissionId;
use autopilot_missions::{MissionError, MissionQueue};
use autopilot_store::SqliteStore;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;

#[derive(Clone)]
pub struct AutomationRuntime {
    control: ControlPlaneStore,
    missions: MissionQueue,
    project_prefix: String,
}

#[derive(Clone, Debug)]
pub struct AutomationDispatch {
    pub automation_id: String,
    pub mission_id: MissionId,
    pub goal: String,
}

#[derive(Debug)]
pub enum AutomationRuntimeError {
    Control(ControlPlaneError),
    Mission(MissionError),
    Store(autopilot_store::StoreError),
    InvalidEventPayload,
}

impl std::fmt::Display for AutomationRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Control(e) => write!(f, "{e}"),
            Self::Mission(e) => write!(f, "{e}"),
            Self::Store(e) => write!(f, "{e}"),
            Self::InvalidEventPayload => write!(f, "event payload must be a JSON object"),
        }
    }
}
impl std::error::Error for AutomationRuntimeError {}
impl From<ControlPlaneError> for AutomationRuntimeError {
    fn from(value: ControlPlaneError) -> Self { Self::Control(value) }
}
impl From<MissionError> for AutomationRuntimeError {
    fn from(value: MissionError) -> Self { Self::Mission(value) }
}
impl From<autopilot_store::StoreError> for AutomationRuntimeError {
    fn from(value: autopilot_store::StoreError) -> Self { Self::Store(value) }
}

impl AutomationRuntime {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AutomationRuntimeError> {
        let path = path.as_ref();
        let control = ControlPlaneStore::open(path)?;
        let store = SqliteStore::open(path)?;
        let missions = MissionQueue::new(store, Duration::seconds(60));
        Ok(Self { control, missions, project_prefix: "automation".into() })
    }

    pub fn from_stores(control: ControlPlaneStore, store: SqliteStore) -> Self {
        Self {
            control,
            missions: MissionQueue::new(store, Duration::seconds(60)),
            project_prefix: "automation".into(),
        }
    }

    pub fn control(&self) -> &ControlPlaneStore { &self.control }
    pub fn missions(&self) -> &MissionQueue { &self.missions }

    fn dispatch(&self, spec: &AutomationSpec) -> Result<AutomationDispatch, AutomationRuntimeError> {
        let project = format!("{}:{}", self.project_prefix, spec.id);
        let mission = self.missions.enqueue(spec.goal.clone(), project)?;
        Ok(AutomationDispatch {
            automation_id: spec.id.to_string(),
            mission_id: mission.id,
            goal: spec.goal.clone(),
        })
    }

    pub fn schedule_due(&self, now: DateTime<Utc>) -> Result<Vec<AutomationDispatch>, AutomationRuntimeError> {
        let specs = self.control.due_automations(now)?;
        let mut dispatched = Vec::new();
        for spec in specs {
            let item = self.dispatch(&spec)?;
            self.control.mark_automation_fired(spec.id, now)?;
            dispatched.push(item);
        }
        Ok(dispatched)
    }

    pub fn emit_event(&self, event: AutomationEvent) -> Result<(String, Vec<AutomationDispatch>), AutomationRuntimeError> {
        let event_id = self.control.enqueue_event(&event)?;
        let specs = self.control.matching_event_automations(&event)?;
        let mut dispatched = Vec::new();
        for spec in specs {
            dispatched.push(self.dispatch(&spec)?);
        }
        self.control.complete_event(event_id)?;
        Ok((event_id.to_string(), dispatched))
    }

    pub fn process_one_queued_event(
        &self,
        owner: &str,
        now: DateTime<Utc>,
        retry: RetryPolicy,
    ) -> Result<Option<Vec<AutomationDispatch>>, AutomationRuntimeError> {
        self.control.recover_expired_event_leases(now)?;
        let Some(lease) = self.control.lease_event(owner, now, 30)? else { return Ok(None); };
        match self.dispatch_lease(&lease) {
            Ok(items) => {
                self.control.complete_event(lease.id)?;
                Ok(Some(items))
            }
            Err(error) => {
                self.control.retry_event(&lease, retry, now, &error.to_string())?;
                Err(error)
            }
        }
    }

    fn dispatch_lease(&self, lease: &EventLease) -> Result<Vec<AutomationDispatch>, AutomationRuntimeError> {
        let object = lease.payload.as_object().ok_or(AutomationRuntimeError::InvalidEventPayload)?;
        let payload: BTreeMap<String, Value> = object.iter().map(|(k,v)|(k.clone(),v.clone())).collect();
        let event = AutomationEvent {
            topic: lease.topic.clone(),
            payload,
            occurred_at: lease.occurred_at,
        };
        let specs = self.control.matching_event_automations(&event)?;
        specs.iter().map(|spec| self.dispatch(spec)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autopilot_automation::Trigger;

    #[test]
    fn due_automation_becomes_persistent_mission() {
        let control=ControlPlaneStore::open_in_memory().unwrap();
        let store=SqliteStore::open_in_memory().unwrap();
        let runtime=AutomationRuntime::from_stores(control,store);
        let spec=AutomationSpec::new(
            "hourly report",
            "prepare business report",
            Trigger::Interval{every_seconds:3600,start_at:None},
        );
        runtime.control().put_automation(&spec).unwrap();
        let items=runtime.schedule_due(Utc::now()).unwrap();
        assert_eq!(items.len(),1);
        assert_eq!(runtime.missions().list().unwrap().len(),1);
        assert_eq!(runtime.missions().list().unwrap()[0].objective,"prepare business report");
    }

    #[test]
    fn matching_event_becomes_mission() {
        let control=ControlPlaneStore::open_in_memory().unwrap();
        let store=SqliteStore::open_in_memory().unwrap();
        let runtime=AutomationRuntime::from_stores(control,store);
        let mut equals=BTreeMap::new();
        equals.insert("status".into(),Value::String("paid".into()));
        let spec=AutomationSpec::new(
            "paid invoice",
            "send receipt",
            Trigger::Event{topic:"invoice.updated".into(),equals},
        );
        runtime.control().put_automation(&spec).unwrap();
        let mut payload=BTreeMap::new();
        payload.insert("status".into(),Value::String("paid".into()));
        let (_,items)=runtime.emit_event(AutomationEvent::new("invoice.updated",payload)).unwrap();
        assert_eq!(items.len(),1);
        assert_eq!(items[0].goal,"send receipt");
    }
}
