use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use autopilot_audit::InMemoryAuditStore;
use autopilot_domain::{AgentRole, RiskClass};
use autopilot_engine::{ParallelPluginWorkflowEngine, PluginGoalStep};
use autopilot_plugin_runtime::{PluginExecutor, PluginRuntimeConfig};
use autopilot_plugins::{parse_manifest_json, PluginRegistry};
use autopilot_policy::PolicyEngine;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};

#[derive(Clone)]
struct SlowJsonRuntime;

#[async_trait]
impl RuntimeAdapter for SlowJsonRuntime {
    async fn probe(&self) -> bool { true }

    async fn execute(&self, _spec: CommandSpec) -> Result<CommandResult, RuntimeError> {
        tokio::time::sleep(Duration::from_millis(120)).await;
        Ok(CommandResult {
            exit_code: 0,
            stdout: "{\"ok\":true}".into(),
            stderr: String::new(),
            duration_ms: 120,
        })
    }

    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}

fn manifest(id: &str, capability: &str, program: &str) -> String {
    format!(r#"{{
      "schema":"synergy.plugin.v1",
      "id":"{id}",
      "name":"{id}",
      "version":"1.0.0",
      "kind":"cli",
      "provider":"m11-test",
      "capabilities":[{{
        "name":"{capability}",
        "risk":"Low",
        "requires_approval":false,
        "mapped_core_capabilities":[],
        "parallel_safe":true
      }}],
      "endpoints":[{{"transport":"cli","address":"{program}"}}],
      "tags":[],
      "requires":[],
      "enabled_by_default":false,
      "adapter":{{"type":"cli","program":"{program}","args":[]}},
      "input_contracts":{{
        "{capability}":{{"type":"object","properties":{{}},"additionalProperties":false}}
      }},
      "output_contracts":{{
        "{capability}":{{
          "type":"object",
          "properties":{{"ok":{{"type":"boolean","const":true}}}},
          "required":["ok"],
          "additionalProperties":false
        }}
      }}
    }}"#)
}

#[tokio::test]
async fn independent_parallel_safe_steps_execute_concurrently() {
    let mut registry=PluginRegistry::default();
    registry.register(parse_manifest_json(&manifest("p1","read.one","slow-one")).unwrap()).unwrap();
    registry.register(parse_manifest_json(&manifest("p2","read.two","slow-two")).unwrap()).unwrap();

    let mut config=PluginRuntimeConfig::default();
    config.generated_program_allowlist.extend(["slow-one".into(),"slow-two".into()]);
    let executor=Arc::new(PluginExecutor::new(
        registry,PolicyEngine::default(),Arc::new(SlowJsonRuntime),config
    ));
    let engine=ParallelPluginWorkflowEngine::new(
        executor,Arc::new(InMemoryAuditStore::default()),None
    );
    let steps=vec![
        PluginGoalStep {
            id:Some("a".into()),role:AgentRole::Custom("Reader".into()),
            plugin_id:"p1".into(),capability:"read.one".into(),
            input:serde_json::json!({}),approved:false,risk:RiskClass::Low,
            employee_id:Some("research-a".into()),department_id:Some("research".into()),
        },
        PluginGoalStep {
            id:Some("b".into()),role:AgentRole::Custom("Reader".into()),
            plugin_id:"p2".into(),capability:"read.two".into(),
            input:serde_json::json!({}),approved:false,risk:RiskClass::Low,
            employee_id:Some("research-b".into()),department_id:Some("research".into()),
        }
    ];

    let started=Instant::now();
    let summary=engine.run("goal:test",steps,vec![vec!["a".into(),"b".into()]])
        .await.unwrap();
    let elapsed=started.elapsed();

    assert!(summary.success);
    assert_eq!(summary.outcomes.len(),2);
    assert!(elapsed < Duration::from_millis(220), "elapsed={elapsed:?}");
}
