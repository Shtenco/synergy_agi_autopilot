use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use autopilot_plugins::{PluginManifest, PluginRegistry};
use autopilot_policy::{PolicyDecision, PolicyEngine};
use autopilot_runtime::{CommandSpec, ExecutionProfile, RuntimeAdapter};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginCall {
    pub plugin_id: String,
    pub capability: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub approved: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginResult {
    pub plugin_id: String,
    pub capability: String,
    pub transport: String,
    pub output: Value,
}

#[derive(Clone, Debug, Default)]
pub struct PluginRuntimeConfig {
    pub local_workspace: Option<PathBuf>,
    pub env_allowlist: BTreeMap<String, Vec<String>>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Error)]
pub enum PluginRuntimeError {
    #[error("unknown plugin: {0}")]
    UnknownPlugin(String),
    #[error("plugin {plugin} does not expose capability {capability}")]
    UnknownCapability { plugin: String, capability: String },
    #[error("plugin capability denied by policy: {0}")]
    Denied(String),
    #[error("plugin capability requires explicit approval: {0}")]
    ApprovalRequired(String),
    #[error("plugin has no supported executable endpoint: {0}")]
    NoEndpoint(String),
    #[error("transport failed: {0}")]
    Transport(String),
    #[error("invalid MCP response: {0}")]
    Mcp(String),
}

pub struct PluginExecutor {
    registry: PluginRegistry,
    policy: PolicyEngine,
    runtime: Arc<dyn RuntimeAdapter>,
    config: PluginRuntimeConfig,
}

impl PluginExecutor {
    pub fn new(
        registry: PluginRegistry,
        policy: PolicyEngine,
        runtime: Arc<dyn RuntimeAdapter>,
        config: PluginRuntimeConfig,
    ) -> Self {
        Self { registry, policy, runtime, config }
    }

    pub fn registry(&self) -> &PluginRegistry { &self.registry }

    fn authorize(&self, manifest: &PluginManifest, call: &PluginCall) -> Result<(), PluginRuntimeError> {
        let capability = manifest.capability(&call.capability).ok_or_else(|| {
            PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(),
                capability: call.capability.clone(),
            }
        })?;
        if capability.requires_approval && !call.approved {
            return Err(PluginRuntimeError::ApprovalRequired(call.capability.clone()));
        }
        for mapped in &capability.mapped_core_capabilities {
            match self.policy.evaluate(mapped.clone(), capability.risk) {
                PolicyDecision::Deny => return Err(PluginRuntimeError::Denied(call.capability.clone())),
                PolicyDecision::RequireApproval if !call.approved => {
                    return Err(PluginRuntimeError::ApprovalRequired(call.capability.clone()))
                }
                PolicyDecision::Allow | PolicyDecision::RequireApproval => {}
            }
        }
        Ok(())
    }

    pub async fn execute(&self, call: PluginCall) -> Result<PluginResult, PluginRuntimeError> {
        let manifest = self.registry.get(&call.plugin_id)
            .ok_or_else(|| PluginRuntimeError::UnknownPlugin(call.plugin_id.clone()))?;
        self.authorize(manifest, &call)?;

        match call.plugin_id.as_str() {
            "computer.local.commander" => self.call_local_commander(manifest, call).await,
            "computer.remote.shttps" => self.call_remote_mcp(manifest, call).await,
            _ => Err(PluginRuntimeError::NoEndpoint(manifest.id.clone())),
        }
    }

    async fn call_local_commander(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let tool = match call.capability.as_str() {
            "computer.read_file" => "read_file",
            "computer.write_file" => "write_file",
            "computer.run_command" => "run_command",
            "computer.system_info" => "system_info",
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let mut spec = CommandSpec::new("synergy-node")
            .args(["mcp", "--stdio"])
            .profile(ExecutionProfile::WorkspaceWrite);
        if let Some(workspace) = &self.config.local_workspace {
            spec = spec.env("SYNERGY_WORKSPACE", workspace.to_string_lossy().into_owned());
        }
        spec = self.apply_env_allowlist(&manifest.id, spec);
        let output = self.mcp_stdio_call(spec, tool, call.input).await?;
        Ok(PluginResult {
            plugin_id: manifest.id.clone(),
            capability: call.capability,
            transport: "mcp_stdio".into(),
            output,
        })
    }

    async fn call_remote_mcp(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let tool = match call.capability.as_str() {
            "computer.remote.list_dir" => "synergy_list_dir",
            "computer.remote.read_file" => "synergy_read_file",
            "computer.remote.run" => "synergy_run",
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let spec = self.apply_env_allowlist(
            &manifest.id,
            CommandSpec::new("synergy-mcp-gateway").profile(ExecutionProfile::Host),
        );
        let output = self.mcp_stdio_call(spec, tool, call.input).await?;
        Ok(PluginResult {
            plugin_id: manifest.id.clone(),
            capability: call.capability,
            transport: "mcp_stdio+shttps_stcp".into(),
            output,
        })
    }

    fn apply_env_allowlist(&self, plugin_id: &str, mut spec: CommandSpec) -> CommandSpec {
        if let Some(keys) = self.config.env_allowlist.get(plugin_id) {
            for key in keys { spec = spec.allow_env(key.clone()); }
        }
        if let Some(timeout_ms) = self.config.timeout_ms { spec = spec.timeout_ms(timeout_ms); }
        spec
    }

    async fn mcp_stdio_call(
        &self,
        mut spec: CommandSpec,
        tool: &str,
        input: Value,
    ) -> Result<Value, PluginRuntimeError> {
        let initialize = json!({
            "jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18",
                "capabilities":{},
                "clientInfo":{"name":"synergy-ai-autopilot","version":"1.0.0"}
            }
        });
        let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        let call = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":input}
        });
        spec = spec.stdin_data(format!(
            "{}\n{}\n{}\n",
            initialize, initialized, call
        ));
        let result = self.runtime.execute(spec).await
            .map_err(|error| PluginRuntimeError::Transport(error.to_string()))?;
        if result.exit_code != 0 {
            return Err(PluginRuntimeError::Transport(format!(
                "exit={} stderr={}", result.exit_code, result.stderr
            )));
        }

        let mut response = None;
        for line in result.stdout.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("id") == Some(&Value::from(2)) { response = Some(value); }
        }
        let response = response.ok_or_else(|| PluginRuntimeError::Mcp("missing tools/call response".into()))?;
        if let Some(error) = response.get("error") {
            return Err(PluginRuntimeError::Mcp(error.to_string()));
        }
        let result = response.get("result").cloned()
            .ok_or_else(|| PluginRuntimeError::Mcp("missing result".into()))?;
        if result.get("isError").and_then(Value::as_bool).unwrap_or(false) {
            return Err(PluginRuntimeError::Mcp(result.to_string()));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autopilot_plugins::parse_manifest_json;
    use autopilot_runtime::{CommandResult, FakeRuntime};

    fn registry() -> PluginRegistry {
        let mut registry = PluginRegistry::default();
        for raw in [
            include_str!("../../../plugins/local-computer/plugin.json"),
            include_str!("../../../plugins/remote-computer/plugin.json"),
        ] {
            registry.register(parse_manifest_json(raw).unwrap()).unwrap();
        }
        registry
    }

    #[tokio::test]
    async fn high_risk_plugin_call_requires_approval() {
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0,
        }));
        let executor = PluginExecutor::new(
            registry(), PolicyEngine::default(), runtime, PluginRuntimeConfig::default(),
        );
        let error = executor.execute(PluginCall {
            plugin_id: "computer.local.commander".into(),
            capability: "computer.run_command".into(),
            input: json!({"program":"cargo","args":["test"]}),
            approved: false,
        }).await.unwrap_err();
        assert!(matches!(error, PluginRuntimeError::ApprovalRequired(_)));
    }

    #[tokio::test]
    async fn local_mcp_parses_tools_call_result() {
        let stdout = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n"
        );
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: stdout.into(), stderr: String::new(), duration_ms: 1,
        }));
        let executor = PluginExecutor::new(
            registry(), PolicyEngine::default(), runtime, PluginRuntimeConfig::default(),
        );
        let result = executor.execute(PluginCall {
            plugin_id: "computer.local.commander".into(),
            capability: "computer.system_info".into(),
            input: json!({}),
            approved: false,
        }).await.unwrap();
        assert_eq!(result.transport, "mcp_stdio");
        assert_eq!(result.output["content"][0]["text"], "ok");
    }
}
