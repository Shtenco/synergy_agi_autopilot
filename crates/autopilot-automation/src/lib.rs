use std::collections::BTreeMap;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Manual,
    Webhook { topic: String },
    Event { topic: String, #[serde(default)] equals: BTreeMap<String, Value> },
    Once { at: DateTime<Utc> },
    Interval { every_seconds: u64, #[serde(default)] start_at: Option<DateTime<Utc>> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationSpec {
    pub id: Uuid,
    pub name: String,
    pub goal: String,
    pub trigger: Trigger,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool { true }

impl AutomationSpec {
    pub fn new(name: impl Into<String>, goal: impl Into<String>, trigger: Trigger) -> Self {
        Self { id: Uuid::new_v4(), name: name.into(), goal: goal.into(), trigger, enabled: true }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutomationEvent {
    pub topic: String,
    #[serde(default)]
    pub payload: BTreeMap<String, Value>,
    pub occurred_at: DateTime<Utc>,
}

impl AutomationEvent {
    pub fn new(topic: impl Into<String>, payload: BTreeMap<String, Value>) -> Self {
        Self { topic: topic.into(), payload, occurred_at: Utc::now() }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AutomationRegistry {
    items: BTreeMap<Uuid, AutomationSpec>,
    last_fired: BTreeMap<Uuid, DateTime<Utc>>,
}

impl AutomationRegistry {
    pub fn upsert(&mut self, spec: AutomationSpec) { self.items.insert(spec.id, spec); }
    pub fn get(&self, id: Uuid) -> Option<&AutomationSpec> { self.items.get(&id) }

    pub fn matching_event(&self, event: &AutomationEvent) -> Vec<&AutomationSpec> {
        self.items.values().filter(|spec| {
            if !spec.enabled { return false; }
            match &spec.trigger {
                Trigger::Webhook { topic } => topic == &event.topic,
                Trigger::Event { topic, equals } => {
                    topic == &event.topic && equals.iter().all(|(key, expected)| event.payload.get(key) == Some(expected))
                }
                Trigger::Manual | Trigger::Once { .. } | Trigger::Interval { .. } => false,
            }
        }).collect()
    }

    pub fn due(&self, now: DateTime<Utc>) -> Vec<&AutomationSpec> {
        self.items.values().filter(|spec| {
            if !spec.enabled { return false; }
            match &spec.trigger {
                Trigger::Once { at } => {
                    at <= &now && self.last_fired.get(&spec.id).is_none()
                }
                Trigger::Interval { every_seconds, start_at } => {
                    if *every_seconds == 0 { return false; }
                    if start_at.as_ref().is_some_and(|start| start > &now) { return false; }
                    match self.last_fired.get(&spec.id) {
                        None => true,
                        Some(last) => last.clone() + Duration::seconds(*every_seconds as i64) <= now,
                    }
                }
                Trigger::Manual | Trigger::Webhook { .. } | Trigger::Event { .. } => false,
            }
        }).collect()
    }

    pub fn mark_fired(&mut self, id: Uuid, at: DateTime<Utc>) {
        if self.items.contains_key(&id) { self.last_fired.insert(id, at); }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthState { Closed, Open, HalfOpen }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitBreaker {
    pub failure_threshold: u32,
    pub recovery_seconds: u64,
    pub consecutive_failures: u32,
    pub state: HealthState,
    pub opened_at: Option<DateTime<Utc>>,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, recovery_seconds: u64) -> Self {
        Self {
            failure_threshold: failure_threshold.max(1),
            recovery_seconds,
            consecutive_failures: 0,
            state: HealthState::Closed,
            opened_at: None,
        }
    }

    pub fn allow(&mut self, now: DateTime<Utc>) -> bool {
        match self.state {
            HealthState::Closed => true,
            HealthState::HalfOpen => true,
            HealthState::Open => {
                let ready = self.opened_at.as_ref().is_some_and(|opened| {
                    opened.clone() + Duration::seconds(self.recovery_seconds as i64) <= now
                });
                if ready {
                    self.state = HealthState::HalfOpen;
                    true
                } else {
                    false
                }
            }
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.state = HealthState::Closed;
        self.opened_at = None;
    }

    pub fn record_failure(&mut self, now: DateTime<Utc>) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures >= self.failure_threshold {
            self.state = HealthState::Open;
            self.opened_at = Some(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_trigger_matches_topic_and_fields() {
        let mut registry = AutomationRegistry::default();
        let mut equals = BTreeMap::new();
        equals.insert("status".into(), Value::String("paid".into()));
        let spec = AutomationSpec::new(
            "invoice paid",
            "prepare receipt",
            Trigger::Event { topic: "invoice.updated".into(), equals },
        );
        registry.upsert(spec);
        let mut payload = BTreeMap::new();
        payload.insert("status".into(), Value::String("paid".into()));
        let event = AutomationEvent::new("invoice.updated", payload);
        assert_eq!(registry.matching_event(&event).len(), 1);
    }

    #[test]
    fn interval_is_due_then_waits_until_next_period() {
        let now = Utc::now();
        let mut registry = AutomationRegistry::default();
        let spec = AutomationSpec::new(
            "hourly",
            "run report",
            Trigger::Interval { every_seconds: 3600, start_at: None },
        );
        let id = spec.id;
        registry.upsert(spec);
        assert_eq!(registry.due(now).len(), 1);
        registry.mark_fired(id, now);
        assert!(registry.due(now + Duration::seconds(3599)).is_empty());
        assert_eq!(registry.due(now + Duration::seconds(3600)).len(), 1);
    }

    #[test]
    fn circuit_breaker_opens_and_recovers_half_open() {
        let now = Utc::now();
        let mut breaker = CircuitBreaker::new(2, 30);
        breaker.record_failure(now);
        assert_eq!(breaker.state, HealthState::Closed);
        breaker.record_failure(now);
        assert_eq!(breaker.state, HealthState::Open);
        assert!(!breaker.allow(now + Duration::seconds(29)));
        assert!(breaker.allow(now + Duration::seconds(30)));
        assert_eq!(breaker.state, HealthState::HalfOpen);
        breaker.record_success();
        assert_eq!(breaker.state, HealthState::Closed);
    }
}
