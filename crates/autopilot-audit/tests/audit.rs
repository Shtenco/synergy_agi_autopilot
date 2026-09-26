use autopilot_audit::{AuditStore, InMemoryAuditStore};
use autopilot_domain::{AuditEvent, CorrelationId, RiskClass};

#[tokio::test]
async fn preserves_append_order() {
    let store = InMemoryAuditStore::default();
    let correlation = CorrelationId::new();
    store.append(AuditEvent::new(correlation, "director", "planning", RiskClass::Low, "ok")).await.unwrap();
    store.append(AuditEvent::new(correlation, "tester", "verification", RiskClass::Low, "ok")).await.unwrap();
    let events = store.list_by_correlation(correlation).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "planning");
    assert_eq!(events[1].event_type, "verification");
}

#[tokio::test]
async fn filters_by_correlation() {
    let store = InMemoryAuditStore::default();
    let a = CorrelationId::new();
    let b = CorrelationId::new();
    store.append(AuditEvent::new(a, "director", "a", RiskClass::Low, "ok")).await.unwrap();
    store.append(AuditEvent::new(b, "director", "b", RiskClass::Low, "ok")).await.unwrap();
    assert_eq!(store.list_by_correlation(a).await.unwrap().len(), 1);
}
