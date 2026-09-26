use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use autopilot_automation::CircuitBreaker;
use autopilot_control_plane::{ApprovalStatus, ControlPlaneStore};
use autopilot_plugins::{PluginManifest, PluginRegistry};
use autopilot_policy::{PolicyDecision, PolicyEngine};
use autopilot_runtime::{CommandSpec, ExecutionProfile, RuntimeAdapter};
use autopilot_secrets::{EnvSecretVault, SecretRef, SecretVault};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

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
    pub bearer_token_env: BTreeMap<String, String>,
    pub control_plane_db: Option<PathBuf>,
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
    #[error("HTTP sidecar error: {0}")]
    Http(String),
    #[error("plugin circuit breaker is open: {0}")]
    CircuitOpen(String),
    #[error("persistent approval verification failed: {0}")]
    PersistentApproval(String),
    #[error("budget verification failed: {0}")]
    Budget(String),
}

pub struct PluginExecutor {
    registry: PluginRegistry,
    policy: PolicyEngine,
    runtime: Arc<dyn RuntimeAdapter>,
    config: PluginRuntimeConfig,
    health: Mutex<BTreeMap<String, CircuitBreaker>>,
}

impl PluginExecutor {
    pub fn new(
        registry: PluginRegistry,
        policy: PolicyEngine,
        runtime: Arc<dyn RuntimeAdapter>,
        config: PluginRuntimeConfig,
    ) -> Self {
        Self { registry, policy, runtime, config, health: Mutex::new(BTreeMap::new()) }
    }

    pub fn registry(&self) -> &PluginRegistry { &self.registry }

    fn authorize(&self, manifest: &PluginManifest, call: &PluginCall) -> Result<(), PluginRuntimeError> {
        let capability = manifest.capability(&call.capability).ok_or_else(|| {
            PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(),
                capability: call.capability.clone(),
            }
        })?;
        if capability.requires_approval {
            if !call.approved {
                return Err(PluginRuntimeError::ApprovalRequired(call.capability.clone()));
            }
            if let Some(db) = &self.config.control_plane_db {
                let approval_id = call.input.get("_approval_id").and_then(Value::as_str)
                    .ok_or_else(|| PluginRuntimeError::PersistentApproval(
                        format!("{} requires _approval_id when persistent control plane is enabled", call.capability)
                    ))?;
                let approval_id = Uuid::parse_str(approval_id)
                    .map_err(|error| PluginRuntimeError::PersistentApproval(error.to_string()))?;
                let store = ControlPlaneStore::open(db)
                    .map_err(|error| PluginRuntimeError::PersistentApproval(error.to_string()))?;
                let request = store.get_approval(approval_id)
                    .map_err(|error| PluginRuntimeError::PersistentApproval(error.to_string()))?
                    .ok_or_else(|| PluginRuntimeError::PersistentApproval("approval not found".into()))?;
                if request.status != ApprovalStatus::Approved {
                    return Err(PluginRuntimeError::PersistentApproval(
                        format!("approval {} is not approved", approval_id)
                    ));
                }
            }
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

        {
            let mut health = self.health.lock()
                .map_err(|_| PluginRuntimeError::Transport("plugin health lock poisoned".into()))?;
            let breaker = health.entry(call.plugin_id.clone())
                .or_insert_with(|| CircuitBreaker::new(3, 30));
            if !breaker.allow(Utc::now()) {
                return Err(PluginRuntimeError::CircuitOpen(call.plugin_id.clone()));
            }
        }

        if let Some(db) = &self.config.control_plane_db {
            if let Some(budget) = call.input.get("_budget").and_then(Value::as_object) {
                let budget_id = budget.get("id").and_then(Value::as_str)
                    .ok_or_else(|| PluginRuntimeError::Budget("_budget.id is required".into()))?;
                let amount = budget.get("amount_microusd").and_then(Value::as_i64)
                    .ok_or_else(|| PluginRuntimeError::Budget("_budget.amount_microusd is required".into()))?;
                let store = ControlPlaneStore::open(db)
                    .map_err(|error| PluginRuntimeError::Budget(error.to_string()))?;
                store.reserve_budget(budget_id, Utc::now().date_naive(), amount)
                    .map_err(|error| PluginRuntimeError::Budget(error.to_string()))?;
            }
        }

        let plugin_id = call.plugin_id.clone();
        let result = match call.plugin_id.as_str() {
            "computer.local.commander" => self.call_local_commander(manifest, call).await,
            "computer.remote.shttps" => self.call_remote_mcp(manifest, call).await,
            "workers.agent.fabric" => self.call_agent_fabric(manifest, call).await,
            "intelligence.nexus" => self.call_nexus(manifest, call).await,
            "business.capability.runtime" => self.call_business_runtime(manifest, call).await,
            "business.auto-company" => self.call_auto_company(manifest, call).await,
            "browser.local.playwright" => self.call_browser(manifest, call).await,
            "connector.gateway" | "capability.email" | "capability.calendar" | "capability.crm" => {
                self.call_connector_gateway(manifest, call).await
            }
            _ => Err(PluginRuntimeError::NoEndpoint(manifest.id.clone())),
        };

        if let Ok(mut health) = self.health.lock() {
            let breaker = health.entry(plugin_id).or_insert_with(|| CircuitBreaker::new(3, 30));
            match &result {
                Ok(_) => breaker.record_success(),
                Err(_) => breaker.record_failure(Utc::now()),
            }
        }
        result
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

    async fn call_agent_fabric(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        if call.capability != "worker.schedule" && call.capability != "worker.lease" {
            return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            });
        }
        let agents = call.input.get("agents").and_then(Value::as_array).cloned().unwrap_or_default();
        let task = call.input.get("task").cloned().unwrap_or_else(|| call.input.clone());
        let mut lines = vec![json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string()];
        let mut request_id = 2u64;
        for agent in agents {
            lines.push(json!({"jsonrpc":"2.0","id":request_id,"method":"agent/register","params":agent}).to_string());
            request_id += 1;
        }
        lines.push(json!({"jsonrpc":"2.0","id":request_id,"method":"task/lease","params":task}).to_string());
        let mut spec = CommandSpec::new("agent-fabric-stdio").profile(ExecutionProfile::Host);
        spec = self.apply_env_allowlist(&manifest.id, spec);
        spec = spec.stdin_data(format!("{}\n", lines.join("\n")));
        let result = self.runtime.execute(spec).await
            .map_err(|error| PluginRuntimeError::Transport(error.to_string()))?;
        if result.exit_code != 0 {
            return Err(PluginRuntimeError::Transport(format!("exit={} stderr={}", result.exit_code, result.stderr)));
        }
        let mut selected = None;
        for line in result.stdout.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("id") == Some(&Value::from(request_id)) { selected = Some(value); }
        }
        let selected = selected.ok_or_else(|| PluginRuntimeError::Transport("missing task/lease response".into()))?;
        if let Some(error) = selected.get("error") { return Err(PluginRuntimeError::Transport(error.to_string())); }
        Ok(PluginResult {
            plugin_id: manifest.id.clone(),
            capability: call.capability,
            transport: "synergy.worker.v1+stdio".into(),
            output: selected.get("result").cloned().unwrap_or(Value::Null),
        })
    }

    async fn call_nexus(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let (method, path) = match call.capability.as_str() {
            "memory.search" => ("POST", "/v1/memory/search"),
            "memory.upsert" => ("POST", "/v1/memory/upsert"),
            "intelligence.infer" | "intelligence.reason" => ("POST", "/v1/infer"),
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let output = self.http_json(&manifest.id, "http://127.0.0.1:8130", method, path, Some(call.input.clone())).await?;
        Ok(PluginResult { plugin_id: manifest.id.clone(), capability: call.capability, transport: "http-loopback".into(), output })
    }

    async fn call_business_runtime(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let inner = match call.capability.as_str() {
            "business.ceo.plan" => "ceo.plan",
            "business.cfo.analyze" => "cfo.analyze_budget",
            "business.smm.plan" => "smm.content_plan",
            "business.sales.plan" => "sales.qualify",
            "business.verify.evidence" => "verifier.check_evidence",
            "business.external.publish" => call.input.get("capability").and_then(Value::as_str)
                .ok_or_else(|| PluginRuntimeError::Http("external publish requires input.capability".into()))?,
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let params = call.input.get("params").cloned().unwrap_or_else(|| call.input.clone());
        let created = self.http_json(
            &manifest.id, "http://127.0.0.1:8120", "POST", "/v1/tasks",
            Some(json!({"capability":inner,"params":params}))
        ).await?;
        let task_id = created.get("id").and_then(Value::as_str)
            .ok_or_else(|| PluginRuntimeError::Http("business runtime returned no task id".into()))?
            .to_string();
        let mut state = created;
        if state.get("status").and_then(Value::as_str) == Some("needs_approval") && call.approved {
            state = self.http_json(
                &manifest.id, "http://127.0.0.1:8120", "POST",
                &format!("/v1/tasks/{task_id}/approve"),
                Some(json!({"actor":"synergy-ai-autopilot"}))
            ).await?;
        }
        if matches!(state.get("status").and_then(Value::as_str), Some("ready") | Some("approved")) {
            state = self.http_json(
                &manifest.id, "http://127.0.0.1:8120", "POST",
                &format!("/v1/tasks/{task_id}/execute"), Some(json!({}))
            ).await?;
        }
        let status = state.get("status").and_then(Value::as_str).unwrap_or("");
        if status != "succeeded" {
            return Err(PluginRuntimeError::Http(format!(
                "business capability {} did not succeed: status={} state={}",
                call.capability, status, state
            )));
        }
        if call.capability == "business.verify.evidence" {
            let result_json = state.get("result_json").and_then(Value::as_str)
                .ok_or_else(|| PluginRuntimeError::Http("verifier returned no result_json".into()))?;
            let verified: Value = serde_json::from_str(result_json)
                .map_err(|error| PluginRuntimeError::Http(format!("invalid verifier result_json: {error}")))?;
            if verified.get("verified").and_then(Value::as_bool) != Some(true) {
                return Err(PluginRuntimeError::Http(format!("evidence verifier failed: {verified}")));
            }
        }
        Ok(PluginResult {
            plugin_id: manifest.id.clone(), capability: call.capability,
            transport: "http-loopback+synergy.capability.v1".into(), output: state,
        })
    }

    async fn call_browser(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        if call.capability == "browser.run_plan" {
            let actions = call.input.get("actions").cloned()
                .ok_or_else(|| PluginRuntimeError::Transport("browser.run_plan requires input.actions".into()))?;
            let has_write_action = actions.as_array().map(|items| {
                items.iter().any(|item| matches!(
                    item.get("action").and_then(Value::as_str),
                    Some("click") | Some("fill")
                ))
            }).unwrap_or(false);
            if has_write_action && !call.approved {
                return Err(PluginRuntimeError::ApprovalRequired("browser.run_plan write action".into()));
            }
            let request = json!({
                "actions": actions,
                "profile_dir": call.input.get("profile_dir").cloned().unwrap_or(Value::Null),
                "headless": call.input.get("headless").and_then(Value::as_bool).unwrap_or(true),
                "timeout_ms": call.input.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30000)
            });
            let request_json = request.to_string();
            let spec = self.apply_env_allowlist(
                &manifest.id,
                CommandSpec::new("node")
                    .args(vec!["tools/browser/bridge.mjs".to_string(), request_json])
                    .profile(ExecutionProfile::Host),
            );
            let result = self.runtime.execute(spec).await
                .map_err(|error| PluginRuntimeError::Transport(error.to_string()))?;
            if result.exit_code != 0 {
                return Err(PluginRuntimeError::Transport(format!("browser exit={} stderr={}", result.exit_code, result.stderr)));
            }
            let output = serde_json::from_str::<Value>(&result.stdout)
                .map_err(|error| PluginRuntimeError::Transport(format!("invalid browser JSON: {error}")))?;
            return Ok(PluginResult {
                plugin_id: manifest.id.clone(), capability: call.capability,
                transport: "node+playwright".into(), output,
            });
        }

        let action = match call.capability.as_str() {
            "browser.navigate" => json!({"action":"navigate","url":call.input.get("url").cloned().unwrap_or(Value::Null)}),
            "browser.snapshot" => json!({"action":"snapshot"}),
            "browser.click" => json!({"action":"click","selector":call.input.get("selector").cloned().unwrap_or(Value::Null)}),
            "browser.fill" => json!({"action":"fill","selector":call.input.get("selector").cloned().unwrap_or(Value::Null),"value":call.input.get("value").cloned().unwrap_or(Value::Null)}),
            "browser.wait" => json!({"action":"wait_for","selector":call.input.get("selector").cloned().unwrap_or(Value::Null),"timeout_ms":call.input.get("timeout_ms").cloned().unwrap_or(Value::Null)}),
            "browser.screenshot" => json!({"action":"screenshot","output":call.input.get("output").cloned().unwrap_or(Value::Null)}),
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let request = json!({
            "actions":[action],
            "profile_dir":call.input.get("profile_dir").cloned().unwrap_or(Value::Null),
            "headless":call.input.get("headless").and_then(Value::as_bool).unwrap_or(true),
            "timeout_ms":call.input.get("timeout_ms").and_then(Value::as_u64).unwrap_or(30000)
        });
        let request_json = request.to_string();
        let spec = self.apply_env_allowlist(
            &manifest.id,
            CommandSpec::new("node")
                .args(vec!["tools/browser/bridge.mjs".to_string(), request_json])
                .profile(ExecutionProfile::Host),
        );
        let result = self.runtime.execute(spec).await
            .map_err(|error| PluginRuntimeError::Transport(error.to_string()))?;
        if result.exit_code != 0 {
            return Err(PluginRuntimeError::Transport(format!("browser exit={} stderr={}", result.exit_code, result.stderr)));
        }
        let output = serde_json::from_str::<Value>(&result.stdout)
            .map_err(|error| PluginRuntimeError::Transport(format!("invalid browser JSON: {error}")))?;
        Ok(PluginResult {
            plugin_id: manifest.id.clone(), capability: call.capability,
            transport: "node+playwright".into(), output,
        })
    }

    async fn call_connector_gateway(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let path = match (manifest.id.as_str(), call.capability.as_str()) {
            ("connector.gateway", "http.request") => "/v1/http",
            ("connector.gateway", "openapi.call") => "/v1/openapi",
            ("connector.gateway", "database.query") => "/v1/db/query",
            ("connector.gateway", "queue.publish") => "/v1/queue/publish",
            ("connector.gateway", "queue.consume") => "/v1/queue/consume",
            ("capability.email", "email.send" | "email.read") => "/v1/openapi",
            ("capability.calendar", "calendar.read" | "calendar.event.create" | "calendar.event.update") => "/v1/openapi",
            ("capability.crm", "crm.read" | "crm.record.create" | "crm.record.update") => "/v1/openapi",
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(),
                capability: call.capability,
            }),
        };
        let output = self.http_json(
            &manifest.id,
            "http://127.0.0.1:8150",
            "POST",
            path,
            Some(call.input.clone()),
        ).await?;
        Ok(PluginResult {
            plugin_id: manifest.id.clone(),
            capability: call.capability,
            transport: "connector-gateway".into(),
            output,
        })
    }

    async fn call_auto_company(
        &self,
        manifest: &PluginManifest,
        call: PluginCall,
    ) -> Result<PluginResult, PluginRuntimeError> {
        let (method, path, body) = match call.capability.as_str() {
            "company.status" => ("GET".to_string(), "/v1/status".to_string(), None),
            "company.opportunity.ingest" => ("POST".to_string(), "/v1/leads".to_string(), Some(call.input.clone())),
            "company.pause" => ("POST".to_string(), "/v1/pause".to_string(), Some(json!({"actor":"synergy-ai-autopilot","reason":"plugin call"}))),
            "company.action.approve" => {
                let id = call.input.get("action_id").and_then(Value::as_str).ok_or_else(|| PluginRuntimeError::Http("action_id required".into()))?;
                ("POST".to_string(), format!("/v1/actions/{id}/approve"), Some(json!({"actor":"synergy-ai-autopilot","reason":"approved via Autopilot"})))
            }
            "company.action.dispatch" => {
                let id = call.input.get("action_id").and_then(Value::as_str).ok_or_else(|| PluginRuntimeError::Http("action_id required".into()))?;
                ("POST".to_string(), format!("/v1/actions/{id}/dispatch"), Some(json!({})))
            }
            _ => return Err(PluginRuntimeError::UnknownCapability {
                plugin: manifest.id.clone(), capability: call.capability,
            }),
        };
        let output = self.http_json(&manifest.id, "http://127.0.0.1:8788", &method, &path, body).await?;
        Ok(PluginResult { plugin_id: manifest.id.clone(), capability: call.capability, transport: "http-loopback".into(), output })
    }

    async fn http_json(
        &self,
        plugin_id: &str,
        base: &str,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, PluginRuntimeError> {
        let plugin_id = plugin_id.to_string();
        let base = base.to_string();
        let method = method.to_string();
        let path = path.to_string();
        let body = body.unwrap_or(Value::Null);
        let bearer = match self.config.bearer_token_env.get(&plugin_id) {
            Some(env_key) => {
                let reference = SecretRef::new(env_key.clone())
                    .map_err(|error| PluginRuntimeError::Http(error.to_string()))?;
                let vault = EnvSecretVault::new([env_key.clone()]);
                Some(vault.resolve(&reference)
                    .map_err(|error| PluginRuntimeError::Http(error.to_string()))?
                    .expose().to_string())
            }
            None => None,
        };
        tokio::task::spawn_blocking(move || {
            loopback_http_json(&base, &method, &path, &body, bearer.as_deref())
        }).await.map_err(|error| PluginRuntimeError::Http(error.to_string()))?
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

fn loopback_http_json(
    base: &str,
    method: &str,
    path: &str,
    body: &Value,
    bearer_token: Option<&str>,
) -> Result<Value, PluginRuntimeError> {
    let authority = base.strip_prefix("http://")
        .ok_or_else(|| PluginRuntimeError::Http("only http:// loopback sidecars are supported".into()))?;
    let (host, port) = authority.rsplit_once(':')
        .ok_or_else(|| PluginRuntimeError::Http("sidecar URL must include port".into()))?;
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(PluginRuntimeError::Http("sidecar endpoint must be loopback".into()));
    }
    let port: u16 = port.parse().map_err(|_| PluginRuntimeError::Http("invalid port".into()))?;
    let payload = if method == "GET" { String::new() } else { serde_json::to_string(body).map_err(|e| PluginRuntimeError::Http(e.to_string()))? };
    let mut headers = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: application/json\r\nConnection: close\r\n"
    );
    if !payload.is_empty() {
        headers.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", payload.as_bytes().len()));
    }
    if let Some(token) = bearer_token {
        headers.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    headers.push_str("\r\n");

    let mut stream = TcpStream::connect((host, port))
        .map_err(|e| PluginRuntimeError::Http(format!("connect {host}:{port}: {e}")))?;
    stream.write_all(headers.as_bytes()).map_err(|e| PluginRuntimeError::Http(e.to_string()))?;
    if !payload.is_empty() { stream.write_all(payload.as_bytes()).map_err(|e| PluginRuntimeError::Http(e.to_string()))?; }
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|e| PluginRuntimeError::Http(e.to_string()))?;
    let (head, body) = raw.split_once("\r\n\r\n")
        .ok_or_else(|| PluginRuntimeError::Http("invalid HTTP response".into()))?;
    let status = head.lines().next().and_then(|line| line.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    let parsed = if body.trim().is_empty() { json!({}) } else {
        serde_json::from_str(body).map_err(|e| PluginRuntimeError::Http(format!("invalid JSON response: {e}; body={body}")))?
    };
    if !(200..300).contains(&status) {
        return Err(PluginRuntimeError::Http(format!("status={status} body={parsed}")));
    }
    Ok(parsed)
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
    async fn agent_fabric_returns_a_deterministic_lease_result() {
        let mut full_registry = registry();
        full_registry.register(
            parse_manifest_json(include_str!("../../../plugins/agent-fabric/plugin.json")).unwrap()
        ).unwrap();
        let stdout = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"schema\":\"synergy.worker.v1\"}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"registered\":true}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{\"task_id\":\"t1\",\"agent_id\":\"local\",\"epoch\":1,\"expires_epoch\":11,\"lease_root\":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1]}}\n"
        );
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: stdout.into(), stderr: String::new(), duration_ms: 1,
        }));
        let executor = PluginExecutor::new(
            full_registry, PolicyEngine::default(), runtime, PluginRuntimeConfig::default(),
        );
        let result = executor.execute(PluginCall {
            plugin_id: "workers.agent.fabric".into(),
            capability: "worker.schedule".into(),
            input: json!({
                "agents":[{"id":"local","role":"coder","cpu_millis":1000,"memory_mib":1024,"tools":["cargo"],"remote":false}],
                "task":{"id":"t1","goal":"compile","required_role":"coder","required_tools":["cargo"],"cpu_millis":500,"memory_mib":512,"allow_remote":false,"epoch":1}
            }),
            approved: false,
        }).await.unwrap();
        assert_eq!(result.transport, "synergy.worker.v1+stdio");
        assert_eq!(result.output["agent_id"], "local");
        assert_eq!(result.output["task_id"], "t1");
    }

    #[tokio::test]
    async fn browser_run_plan_write_actions_require_approval() {
        let mut full_registry = registry();
        full_registry.register(
            parse_manifest_json(include_str!("../../../plugins/browser/plugin.json")).unwrap()
        ).unwrap();
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0,
        }));
        let executor = PluginExecutor::new(
            full_registry, PolicyEngine::default(), runtime, PluginRuntimeConfig::default(),
        );
        let error = executor.execute(PluginCall {
            plugin_id: "browser.local.playwright".into(),
            capability: "browser.run_plan".into(),
            input: json!({"actions":[{"action":"click","selector":"#buy"}]}),
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
