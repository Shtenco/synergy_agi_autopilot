use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self { Self(Uuid::new_v4()) }
        }

        impl Default for $name {
            fn default() -> Self { Self::new() }
        }
    };
}

uuid_id!(GoalId);
uuid_id!(TaskId);
uuid_id!(RunId);
uuid_id!(CorrelationId);
uuid_id!(MissionId);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AgentRole {
    Director,
    Architect,
    Coder,
    Debugger,
    Tester,
    Reviewer,
    Security,
    Release,
    Custom(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GoalState {
    Created,
    Planning,
    PolicyCheck,
    Scheduling,
    Executing,
    Reviewing,
    Verifying,
    Repairing,
    Publishing,
    Completed,
    Failed,
    Cancelled,
}

impl GoalState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: &Self) -> bool {
        if self.is_terminal() { return false; }
        if matches!(next, Self::Failed | Self::Cancelled) && self != Self::Created { return true; }
        matches!(
            (self, *next),
            (Self::Created, Self::Planning)
                | (Self::Planning, Self::PolicyCheck)
                | (Self::PolicyCheck, Self::Scheduling)
                | (Self::Scheduling, Self::Executing)
                | (Self::Executing, Self::Reviewing)
                | (Self::Reviewing, Self::Verifying)
                | (Self::Verifying, Self::Repairing)
                | (Self::Verifying, Self::Publishing)
                | (Self::Repairing, Self::Executing)
                | (Self::Publishing, Self::Completed)
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MissionState {
    Queued,
    Leased,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
}

impl MissionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        if self.is_terminal() { return false; }
        matches!(
            (self, next),
            (Self::Queued, Self::Leased)
                | (Self::Queued, Self::Cancelled)
                | (Self::Leased, Self::Running)
                | (Self::Leased, Self::Queued)
                | (Self::Leased, Self::Failed)
                | (Self::Leased, Self::Cancelled)
                | (Self::Running, Self::Waiting)
                | (Self::Running, Self::Completed)
                | (Self::Running, Self::Failed)
                | (Self::Running, Self::Cancelled)
                | (Self::Running, Self::Queued)
                | (Self::Waiting, Self::Running)
                | (Self::Waiting, Self::Queued)
                | (Self::Waiting, Self::Failed)
                | (Self::Waiting, Self::Cancelled)
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRecord {
    pub id: MissionId,
    pub objective: String,
    pub project: String,
    pub state: MissionState,
    pub phase: String,
    pub attempt: u32,
    pub lease_owner: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MissionRecord {
    pub fn queued(objective: impl Into<String>, project: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: MissionId::new(), objective: objective.into(), project: project.into(),
            state: MissionState::Queued, phase: "Queued".into(), attempt: 0,
            lease_owner: None, lease_until: None, created_at: now, updated_at: now,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub mission_id: MissionId,
    pub phase: String,
    pub payload_json: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub project: String,
    pub category: String,
    pub key: String,
    pub value_json: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    ReadWorkspace,
    WriteWorkspace,
    ExecuteProcess,
    NetworkAccess,
    GitCommit,
    GitPush,
    CreatePullRequest,
    ModifyCi,
    InstallDependencies,
    AccessSecrets,
    DockerExecution,
    WslExecution,
    DeleteFiles,
    ForcePush,
    ReleasePublish,
    ComputerUse,
    Custom(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RiskClass { Low, Medium, High, Critical }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub summary: String,
    pub passed: bool,
}

impl Evidence {
    pub fn passed(kind: impl Into<String>, summary: impl Into<String>) -> Self {
        Self { kind: kind.into(), summary: summary.into(), passed: true }
    }

    pub fn failed(kind: impl Into<String>, summary: impl Into<String>) -> Self {
        Self { kind: kind.into(), summary: summary.into(), passed: false }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub id: Uuid,
    pub correlation_id: CorrelationId,
    pub timestamp: DateTime<Utc>,
    pub actor: String,
    pub event_type: String,
    pub risk: RiskClass,
    pub status: String,
}

impl AuditEvent {
    pub fn new(
        correlation_id: CorrelationId,
        actor: impl Into<String>,
        event_type: impl Into<String>,
        risk: RiskClass,
        status: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(), correlation_id, timestamp: Utc::now(), actor: actor.into(),
            event_type: event_type.into(), risk, status: status.into(),
        }
    }
}
