use std::sync::Arc;

use autopilot_audit::InMemoryAuditStore;
use autopilot_domain::{AgentRole, GoalState, RiskClass};
use autopilot_engine::{PluginGoalEngine, PluginGoalStep};
use autopilot_plugin_runtime::{PluginExecutor, PluginRuntimeConfig};
use autopilot_plugins::{parse_manifest_json, PluginRegistry};
use autopilot_policy::PolicyEngine;
use autopilot_runtime::{CommandResult, FakeRuntime};
use serde_json::json;

fn registry() -> PluginRegistry {
    let mut registry = PluginRegistry::default();
    registry
        .register(
            parse_manifest_json(include_str!("../../../plugins/local-computer/plugin.json"))
                .expect("local commander manifest"),
        )
        .expect("register local commander");
    registry
}

#[tokio::test]
async fn director_completes_a_safe_plugin_goal_with_structured_evidence() {
    let stdout = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"os=windows\"}]}}\n"
    );
    let runtime = Arc::new(FakeRuntime::new(CommandResult {
        exit_code: 0,
        stdout: stdout.into(),
        stderr: String::new(),
        duration_ms: 1,
    }));
    let executor = Arc::new(PluginExecutor::new(
        registry(),
        PolicyEngine::default(),
        runtime,
        PluginRuntimeConfig::default(),
    ));
    let audit = Arc::new(InMemoryAuditStore::default());
    let engine = PluginGoalEngine::new(
        audit,
        executor,
        vec![PluginGoalStep::new(
            AgentRole::Tester,
            "computer.local.commander",
            "computer.system_info",
            json!({}),
            RiskClass::Low,
        )],
        0,
    );

    let summary = engine.run("inspect the local node").await.expect("plugin goal");
    assert_eq!(summary.final_state, GoalState::Completed);
    assert!(!summary.blocked_by_policy);
    assert_eq!(summary.evidence.len(), 1);
    assert!(summary.evidence[0].passed);
}
