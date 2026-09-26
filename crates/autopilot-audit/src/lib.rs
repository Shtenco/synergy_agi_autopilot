use std::sync::Arc;
use async_trait::async_trait;
use autopilot_domain::{AuditEvent, CorrelationId};
use tokio::sync::RwLock;

pub type AuditResult<T> = Result<T, String>;

#[async_trait]
pub trait AuditStore: Send + Sync {
    async fn append(&self, event: AuditEvent) -> AuditResult<()>;
    async fn list_by_correlation(&self, correlation: CorrelationId) -> AuditResult<Vec<AuditEvent>>;
}

#[derive(Clone, Default)]
pub struct InMemoryAuditStore { events: Arc<RwLock<Vec<AuditEvent>>> }

#[async_trait]
impl AuditStore for InMemoryAuditStore {
    async fn append(&self, event: AuditEvent) -> AuditResult<()> {
        self.events.write().await.push(event);
        Ok(())
    }

    async fn list_by_correlation(&self, correlation: CorrelationId) -> AuditResult<Vec<AuditEvent>> {
        Ok(self.events.read().await.iter().filter(|event| event.correlation_id == correlation).cloned().collect())
    }
}
