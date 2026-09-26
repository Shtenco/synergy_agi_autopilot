use std::collections::{BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use autopilot_capability_graph::CapabilityGraph;
use autopilot_domain::{Capability, RiskClass};
use autopilot_plugins::{PluginAdapter, PluginCapability, PluginEndpoint, PluginKind, PluginManifest, PluginRegistry, PLUGIN_SCHEMA_V1};
use autopilot_runtime::{CommandSpec, ExecutionProfile, RuntimeAdapter};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticGap {
    pub goal: String,
    pub suggested_capability: String,
    pub terms: BTreeSet<String>,
    pub reason: String,
}

pub struct SemanticGapDetector;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryStrategy {
    OpenApiDocument,
    McpConfiguration,
    CliEntrypoint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfBuildPlan {
    pub gap: SemanticGap,
    pub repository_candidates: Vec<RepositoryDiscoveryCandidate>,
    pub strategies: Vec<DiscoveryStrategy>,
    pub qualification_gates: Vec<String>,
    pub auto_execution_allowed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscoveryCandidate {
    pub repository: String,
    pub score: i64,
    pub transports: BTreeSet<String>,
}

impl SemanticGapDetector {
    fn gap(goal: &str, reason: &str) -> SemanticGap {
        let terms = tokenize(goal);
        SemanticGap {
            goal: goal.to_string(),
            suggested_capability: infer_capability(&terms),
            terms,
            reason: reason.into(),
        }
    }

    pub fn detect(graph: &CapabilityGraph, goal: &str) -> Option<SemanticGap> {
        if !graph.route_goal(goal, 1).is_empty() {
            return None;
        }
        Some(Self::gap(goal, "no executable provider matched the semantic goal"))
    }

    pub fn detect_typed(
        graph: &CapabilityGraph,
        registry: &PluginRegistry,
        goal: &str,
    ) -> Option<SemanticGap> {
        let routes = graph.route_goal(goal, 20);
        let typed = routes.iter().any(|route| {
            registry.get(&route.provider)
                .and_then(|manifest| manifest.input_contract(&route.capability))
                .is_some()
        });
        if typed {
            None
        } else {
            Some(Self::gap(goal, "no typed executable provider matched the semantic goal"))
        }
    }

    pub fn self_build_plan(graph: &CapabilityGraph, goal: &str) -> Option<SelfBuildPlan> {
        let gap = Self::detect(graph, goal)?;
        Some(Self::plan_for_gap(graph, gap))
    }

    pub fn self_build_plan_typed(
        graph: &CapabilityGraph,
        registry: &PluginRegistry,
        goal: &str,
    ) -> Option<SelfBuildPlan> {
        let gap = Self::detect_typed(graph, registry, goal)?;
        Some(Self::plan_for_gap(graph, gap))
    }

    pub fn typed_gaps_for_goal(
        graph: &CapabilityGraph,
        registry: &PluginRegistry,
        goal: &str,
    ) -> Vec<SemanticGap> {
        split_objectives(goal).into_iter()
            .filter_map(|objective| Self::detect_typed(graph, registry, &objective))
            .collect()
    }

    fn plan_for_gap(graph: &CapabilityGraph, gap: SemanticGap) -> SelfBuildPlan {
        let query = autopilot_capability_graph::CapabilityQuery {
            capability: Some(gap.suggested_capability.clone()),
            keywords: gap.terms.iter().cloned().collect(),
            require_transport: None,
            include_archived: false,
        };
        let repository_candidates = graph.search(&query).into_iter().take(10).map(|candidate| {
            RepositoryDiscoveryCandidate {
                repository: candidate.repository,
                score: candidate.score,
                transports: candidate.transports,
            }
        }).collect();

        SelfBuildPlan {
            gap,
            repository_candidates,
            strategies: vec![
                DiscoveryStrategy::OpenApiDocument,
                DiscoveryStrategy::McpConfiguration,
                DiscoveryStrategy::CliEntrypoint,
            ],
            qualification_gates: vec![
                "static source discovery without code execution".into(),
                "synergy.plugin.v1 manifest validation".into(),
                "high-risk approval requirement".into(),
                "sandbox probe in read-only isolation".into(),
                "runtime executable allowlist for MCP/CLI".into(),
                "promotion from quarantine only after all checks pass".into(),
            ],
            auto_execution_allowed: false,
        }
    }
}

fn split_objectives(goal: &str) -> Vec<String> {
    let normalized = goal
        .replace("→", "\n")
        .replace("->", "\n")
        .replace(" затем ", "\n")
        .replace(" потом ", "\n")
        .replace(" and then ", "\n")
        .replace(';', "\n");
    normalized.lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn tokenize(goal: &str) -> BTreeSet<String> {
    goal.to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|part| part.len() >= 2)
        .map(str::to_string)
        .collect()
}

fn infer_capability(terms: &BTreeSet<String>) -> String {
    let joined = terms.iter().cloned().collect::<Vec<_>>().join(" ");
    let has = |needles: &[&str]| needles.iter().any(|needle| joined.contains(needle));
    let write = has(&["send","отправ","create","созда","update","обнов","write","запиш","publish","опублик"]);
    let update = has(&["update","обнов"]);
    let create = has(&["create","созда"]);
    let send = has(&["send","отправ"]);

    if has(&["email","почт"]) {
        return if send || write { "email.send" } else { "email.read" }.into();
    }
    if has(&["calendar","календар"]) {
        if update { return "calendar.event.update".into(); }
        if create || write { return "calendar.event.create".into(); }
        return "calendar.read".into();
    }
    if has(&["crm"]) {
        if update { return "crm.record.update".into(); }
        if create || write { return "crm.record.create".into(); }
        return "crm.read".into();
    }
    if has(&["browser","сайт","web"]) {
        if has(&["click","клик","наж"]) { return "browser.click".into(); }
        if has(&["fill","заполн"]) { return "browser.fill".into(); }
        if has(&["navigate","открой","перейд","open"]) { return "browser.navigate".into(); }
        if has(&["screenshot","snapshot","скрин","снимок"]) { return "browser.snapshot".into(); }
        return "browser.snapshot".into();
    }
    if has(&["file","файл"]) {
        return if write { "computer.write_file" } else { "computer.read_file" }.into();
    }
    if has(&["database","баз"]) { return "database.query".into(); }
    if has(&["memory","памят"]) { return if write { "memory.upsert" } else { "memory.search" }.into(); }

    let normalized = terms.iter().next().cloned().unwrap_or_else(|| "generic".into()).replace('-', "_");
    format!("custom.{normalized}.read")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag="type", rename_all="snake_case")]
pub enum AdapterSource {
    OpenApi {
        base_url: String,
        operation_id: String,
        document: Value,
    },
    McpStdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        tool: String,
    },
    Cli {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum CandidateState {
    Quarantined,
    Qualified,
    Rejected,
    Promoted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationReport {
    pub checked_at: DateTime<Utc>,
    pub checks: Vec<QualificationCheck>,
}

impl QualificationReport {
    pub fn passed(&self) -> bool {
        !self.checks.is_empty() && self.checks.iter().all(|check| check.passed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCandidate {
    pub candidate_id: Uuid,
    pub requested_capability: String,
    pub source: AdapterSource,
    pub manifest: PluginManifest,
    pub state: CandidateState,
    pub created_at: DateTime<Utc>,
    pub qualification: Option<QualificationReport>,
}

#[derive(Debug, Error)]
pub enum BuilderError {
    #[error("invalid candidate: {0}")]
    Invalid(String),
    #[error("candidate is not qualified")]
    NotQualified,
    #[error("candidate has already been promoted")]
    AlreadyPromoted,
    #[error("I/O error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Json(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildRequest {
    pub plugin_id: String,
    pub display_name: String,
    pub capability: String,
    pub source: AdapterSource,
    pub risk: RiskClass,
    pub requires_approval: bool,
    #[serde(default)]
    pub input_contract: Option<Value>,
    pub tags: BTreeSet<String>,
}

pub struct PluginBuilder;

impl PluginBuilder {
    pub fn build(request: BuildRequest) -> Result<PluginCandidate, BuilderError> {
        validate_source(&request.source)?;
        let (kind, endpoint, provider) = match &request.source {
            AdapterSource::OpenApi { base_url, .. } => (
                PluginKind::OpenApi,
                PluginEndpoint { transport: "openapi_gateway".into(), address: base_url.clone() },
                "connector.gateway".to_string(),
            ),
            AdapterSource::McpStdio { command, .. } => (
                PluginKind::Mcp,
                PluginEndpoint { transport: "mcp_stdio".into(), address: command.clone() },
                "generated.mcp".to_string(),
            ),
            AdapterSource::Cli { program, .. } => (
                PluginKind::Cli,
                PluginEndpoint { transport: "cli".into(), address: program.clone() },
                "generated.cli".to_string(),
            ),
        };
        let manifest = PluginManifest {
            schema: PLUGIN_SCHEMA_V1.into(),
            id: request.plugin_id,
            name: request.display_name,
            version: "0.1.0".into(),
            kind,
            provider,
            capabilities: vec![PluginCapability {
                name: request.capability.clone(),
                risk: request.risk,
                requires_approval: request.requires_approval || request.risk >= RiskClass::High,
                mapped_core_capabilities: mapped_capabilities(&request.source, request.risk),
            }],
            endpoints: vec![endpoint],
            tags: request.tags,
            requires: dependencies(&request.source),
            enabled_by_default: false,
            adapter: Some(match &request.source {
                AdapterSource::OpenApi { base_url, operation_id, document } => PluginAdapter::OpenApi {
                    base_url: base_url.clone(),
                    operation_id: operation_id.clone(),
                    document: document.clone(),
                },
                AdapterSource::McpStdio { command, args, tool } => PluginAdapter::McpStdio {
                    command: command.clone(),
                    args: args.clone(),
                    tool: tool.clone(),
                },
                AdapterSource::Cli { program, args } => PluginAdapter::Cli {
                    program: program.clone(),
                    args: args.clone(),
                },
            }),
            input_contracts: {
                let mut contracts = std::collections::BTreeMap::new();
                contracts.insert(
                    request.capability.clone(),
                    request.input_contract.clone()
                        .unwrap_or_else(|| derive_input_contract(&request.source)),
                );
                contracts
            },
        };
        manifest.validate().map_err(|error| BuilderError::Invalid(error.to_string()))?;
        Ok(PluginCandidate {
            candidate_id: Uuid::new_v4(),
            requested_capability: request.capability,
            source: request.source,
            manifest,
            state: CandidateState::Quarantined,
            created_at: Utc::now(),
            qualification: None,
        })
    }
}

fn derive_input_contract(source: &AdapterSource) -> Value {
    match source {
        AdapterSource::OpenApi { operation_id, document, .. } => {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::<Value>::new();
            let mut body_schema = None;

            if let Some(paths) = document.get("paths").and_then(Value::as_object) {
                'outer: for item in paths.values().filter_map(Value::as_object) {
                    for operation in item.values().filter_map(Value::as_object) {
                        if operation.get("operationId").and_then(Value::as_str) != Some(operation_id) {
                            continue;
                        }
                        for parameters in [
                            item.get("parameters").and_then(Value::as_array),
                            operation.get("parameters").and_then(Value::as_array),
                        ].into_iter().flatten() {
                            for parameter in parameters {
                                let Some(name) = parameter.get("name").and_then(Value::as_str) else { continue };
                                let location = parameter.get("in").and_then(Value::as_str).unwrap_or("query");
                                let raw_schema = parameter.get("schema").cloned().unwrap_or_else(|| serde_json::json!({"type":"string"}));
                                let mut schema = resolve_document_schema_refs(document, &raw_schema, 0);
                                if let Some(object) = schema.as_object_mut() {
                                    object.insert("x-location".into(), Value::String(location.into()));
                                }
                                properties.insert(name.to_string(), schema);
                                if parameter.get("required").and_then(Value::as_bool) == Some(true)
                                    && !required.iter().any(|value| value.as_str() == Some(name))
                                {
                                    required.push(Value::String(name.to_string()));
                                }
                            }
                        }
                        body_schema = operation.get("requestBody")
                            .and_then(|body| body.get("content"))
                            .and_then(Value::as_object)
                            .and_then(|content| {
                                content.get("application/json")
                                    .or_else(|| content.iter()
                                        .find(|(media, _)| media.contains("json"))
                                        .map(|(_, value)| value))
                            })
                            .and_then(|media| media.get("schema"))
                            .map(|schema| resolve_document_schema_refs(document, schema, 0));
                        if operation.pointer("/requestBody/required").and_then(Value::as_bool) == Some(true) {
                            required.push(Value::String("body".into()));
                        }
                        break 'outer;
                    }
                }
            }

            if let Some(schema) = body_schema {
                properties.insert("body".into(), schema);
            }
            serde_json::json!({
                "type":"object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            })
        }
        AdapterSource::McpStdio { .. } => serde_json::json!({
            "type":"object",
            "properties":{},
            "additionalProperties": true,
            "x-contract-source":"mcp-runtime-discovery"
        }),
        AdapterSource::Cli { .. } => serde_json::json!({
            "type":"object",
            "properties":{},
            "additionalProperties": true,
            "x-contract-source":"cli-explicit-mapping"
        }),
    }
}

fn resolve_document_schema_refs(document: &Value, schema: &Value, depth: usize) -> Value {
    if depth >= 12 {
        return schema.clone();
    }
    match schema {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                if let Some(pointer) = reference.strip_prefix('#') {
                    if let Some(target) = document.pointer(pointer) {
                        let mut resolved = resolve_document_schema_refs(document, target, depth + 1);
                        if let Some(object) = resolved.as_object_mut() {
                            for (key, value) in map {
                                if key != "$ref" {
                                    object.insert(
                                        key.clone(),
                                        resolve_document_schema_refs(document, value, depth + 1),
                                    );
                                }
                            }
                        }
                        return resolved;
                    }
                }
            }
            Value::Object(map.iter().map(|(key, value)| {
                (key.clone(), resolve_document_schema_refs(document, value, depth + 1))
            }).collect())
        }
        Value::Array(items) => Value::Array(items.iter()
            .map(|item| resolve_document_schema_refs(document, item, depth + 1))
            .collect()),
        other => other.clone(),
    }
}

fn dependencies(source: &AdapterSource) -> BTreeSet<String> {
    match source {
        AdapterSource::OpenApi { .. } => ["connector.gateway".to_string()].into_iter().collect(),
        AdapterSource::McpStdio { .. } | AdapterSource::Cli { .. } => BTreeSet::new(),
    }
}

fn mapped_capabilities(source: &AdapterSource, risk: RiskClass) -> BTreeSet<Capability> {
    let mut caps = BTreeSet::new();
    if matches!(source, AdapterSource::OpenApi { .. }) {
        caps.insert(Capability::NetworkAccess);
    }
    if risk >= RiskClass::High {
        caps.insert(Capability::AccessSecrets);
    }
    caps
}

fn validate_source(source: &AdapterSource) -> Result<(), BuilderError> {
    match source {
        AdapterSource::OpenApi { base_url, operation_id, document } => {
            let lower = base_url.to_ascii_lowercase();
            if !(lower.starts_with("https://") || lower.starts_with("http://127.0.0.1") || lower.starts_with("http://localhost")) {
                return Err(BuilderError::Invalid("OpenAPI base URL must use HTTPS except loopback".into()));
            }
            if operation_id.trim().is_empty() {
                return Err(BuilderError::Invalid("OpenAPI operationId is empty".into()));
            }
            let found = document.get("paths").and_then(Value::as_object).is_some_and(|paths| {
                paths.values().filter_map(Value::as_object).any(|methods| {
                    methods.values().filter_map(Value::as_object).any(|op| {
                        op.get("operationId").and_then(Value::as_str) == Some(operation_id)
                    })
                })
            });
            if !found {
                return Err(BuilderError::Invalid(format!("operationId not found: {operation_id}")));
            }
        }
        AdapterSource::McpStdio { command, tool, args } => {
            validate_program(command)?;
            if args.iter().any(|arg| contains_shell_meta(arg)) {
                return Err(BuilderError::Invalid("MCP args contain shell metacharacters".into()));
            }
            if tool.trim().is_empty() {
                return Err(BuilderError::Invalid("MCP tool name is empty".into()));
            }
        }
        AdapterSource::Cli { program, args } => {
            validate_program(program)?;
            if args.iter().any(|arg| contains_shell_meta(arg)) {
                return Err(BuilderError::Invalid("CLI adapter arguments contain shell metacharacters".into()));
            }
        }
    }
    Ok(())
}

fn validate_program(program: &str) -> Result<(), BuilderError> {
    if program.trim().is_empty() || contains_shell_meta(program) {
        return Err(BuilderError::Invalid("program is empty or contains shell metacharacters".into()));
    }
    Ok(())
}

fn contains_shell_meta(value: &str) -> bool {
    [';', '|', '&', '>', '<', '`', '\n', '\r'].iter().any(|c| value.contains(*c))
}


#[async_trait]
pub trait CandidateQualifier: Send + Sync {
    async fn qualify(&self, candidate: &PluginCandidate) -> QualificationReport;
}

pub struct SandboxQualifier {
    runtime: Arc<dyn RuntimeAdapter>,
    allowed_programs: BTreeSet<String>,
}

impl SandboxQualifier {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, allowed_programs: BTreeSet<String>) -> Self {
        Self { runtime, allowed_programs }
    }

    fn static_checks(&self, candidate: &PluginCandidate) -> Vec<QualificationCheck> {
        vec![
            QualificationCheck {
                name: "manifest".into(),
                passed: candidate.manifest.validate().is_ok(),
                detail: "synergy.plugin.v1 manifest validates".into(),
            },
            QualificationCheck {
                name: "disabled_by_default".into(),
                passed: !candidate.manifest.enabled_by_default,
                detail: "generated candidates remain disabled until promotion".into(),
            },
            QualificationCheck {
                name: "approval_for_high_risk".into(),
                passed: candidate.manifest.capabilities.iter().all(|cap| {
                    cap.risk < RiskClass::High || cap.requires_approval
                }),
                detail: "high-risk generated capabilities require approval".into(),
            },
            QualificationCheck {
                name: "source_contract".into(),
                passed: validate_source(&candidate.source).is_ok(),
                detail: "adapter source passes transport-specific validation".into(),
            },
        ]
    }

    async fn probe(&self, candidate: &PluginCandidate) -> QualificationCheck {
        match &candidate.source {
            AdapterSource::OpenApi { .. } => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: true,
                detail: "OpenAPI uses trusted Connector Gateway; mutating external requests are never auto-probed".into(),
            },
            AdapterSource::McpStdio { command, args, tool } => {
                self.probe_mcp(command, args, tool).await
            }
            AdapterSource::Cli { program, args } => {
                self.probe_program(program, args, "CLI").await
            }
        }
    }

    async fn probe_mcp(&self, command: &str, args: &[String], tool: &str) -> QualificationCheck {
        if !self.allowed_programs.contains(command) {
            return QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: format!("program is not in sandbox allowlist: {command}"),
            };
        }
        let initialize = serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"synergy-plugin-qualifier","version":"0.1.0"}}
        });
        let initialized = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        let list = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let stdin = format!("{}\n{}\n{}\n", initialize, initialized, list);
        let spec = CommandSpec::new(command.to_string())
            .args(args.to_vec())
            .profile(ExecutionProfile::ReadOnlyWorkspace)
            .stdin_data(stdin);
        match self.runtime.execute(spec).await {
            Ok(result) if result.exit_code == 0 => {
                let mut found = false;
                for line in result.stdout.lines() {
                    let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
                    if value.get("id") != Some(&Value::from(2)) { continue; }
                    found = value.pointer("/result/tools").and_then(Value::as_array)
                        .is_some_and(|tools| tools.iter().any(|item| item.get("name").and_then(Value::as_str) == Some(tool)));
                }
                QualificationCheck {
                    name: "sandbox_probe".into(),
                    passed: found,
                    detail: if found {
                        format!("MCP tool exists in sandbox tools/list: {tool}")
                    } else {
                        format!("MCP tools/list did not expose requested tool: {tool}")
                    },
                }
            }
            Ok(result) => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: format!("MCP probe exit={} stderr={}", result.exit_code, result.stderr),
            },
            Err(error) => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: error.to_string(),
            },
        }
    }

    async fn probe_program(&self, program: &str, args: &[String], label: &str) -> QualificationCheck {
        if !self.allowed_programs.contains(program) {
            return QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: format!("program is not in sandbox allowlist: {program}"),
            };
        }
        let spec = CommandSpec::new(program.to_string())
            .args(args.to_vec())
            .profile(ExecutionProfile::ReadOnlyWorkspace);
        match self.runtime.execute(spec).await {
            Ok(result) if result.exit_code == 0 => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: true,
                detail: format!("{label} probe completed in read-only sandbox profile"),
            },
            Ok(result) => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: format!("probe exit={} stderr={}", result.exit_code, result.stderr),
            },
            Err(error) => QualificationCheck {
                name: "sandbox_probe".into(),
                passed: false,
                detail: error.to_string(),
            },
        }
    }
}

#[async_trait]
impl CandidateQualifier for SandboxQualifier {
    async fn qualify(&self, candidate: &PluginCandidate) -> QualificationReport {
        let mut checks = self.static_checks(candidate);
        checks.push(self.probe(candidate).await);
        QualificationReport { checked_at: Utc::now(), checks }
    }
}

#[derive(Clone, Debug)]
pub struct CandidateStore {
    root: PathBuf,
}

impl CandidateStore {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { root: root.into() } }

    fn candidate_dir(&self, id: Uuid) -> PathBuf { self.root.join(id.to_string()) }

    pub fn save(&self, candidate: &PluginCandidate) -> Result<(), BuilderError> {
        let dir = self.candidate_dir(candidate.candidate_id);
        fs::create_dir_all(&dir).map_err(|error| BuilderError::Io(error.to_string()))?;
        let raw = serde_json::to_string_pretty(candidate)
            .map_err(|error| BuilderError::Json(error.to_string()))?;
        fs::write(dir.join("candidate.json"), raw)
            .map_err(|error| BuilderError::Io(error.to_string()))
    }

    pub fn load(&self, id: Uuid) -> Result<PluginCandidate, BuilderError> {
        let raw = fs::read_to_string(self.candidate_dir(id).join("candidate.json"))
            .map_err(|error| BuilderError::Io(error.to_string()))?;
        serde_json::from_str(&raw).map_err(|error| BuilderError::Json(error.to_string()))
    }

    pub async fn qualify<Q: CandidateQualifier>(
        &self,
        id: Uuid,
        qualifier: &Q,
    ) -> Result<PluginCandidate, BuilderError> {
        let mut candidate = self.load(id)?;
        let report = qualifier.qualify(&candidate).await;
        candidate.state = if report.passed() {
            CandidateState::Qualified
        } else {
            CandidateState::Rejected
        };
        candidate.qualification = Some(report);
        self.save(&candidate)?;
        Ok(candidate)
    }

    pub fn promote(
        &self,
        id: Uuid,
        generated_plugins_root: impl AsRef<Path>,
    ) -> Result<PathBuf, BuilderError> {
        let mut candidate = self.load(id)?;
        match candidate.state {
            CandidateState::Qualified => {}
            CandidateState::Promoted => return Err(BuilderError::AlreadyPromoted),
            CandidateState::Quarantined | CandidateState::Rejected => {
                return Err(BuilderError::NotQualified)
            }
        }
        let target = generated_plugins_root.as_ref()
            .join(&candidate.manifest.id)
            .join("plugin.json");
        let parent = target.parent()
            .ok_or_else(|| BuilderError::Io("invalid target path".into()))?;
        fs::create_dir_all(parent).map_err(|error| BuilderError::Io(error.to_string()))?;
        let raw = serde_json::to_string_pretty(&candidate.manifest)
            .map_err(|error| BuilderError::Json(error.to_string()))?;
        fs::write(&target, raw).map_err(|error| BuilderError::Io(error.to_string()))?;
        candidate.state = CandidateState::Promoted;
        self.save(&candidate)?;
        Ok(target)
    }
}

#[async_trait]
pub trait BuildDiscovery: Send + Sync {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError>;
}

#[derive(Clone, Debug)]
pub struct FilesystemOpenApiDiscovery {
    root: PathBuf,
    base_url_override: Option<String>,
    max_files: usize,
}

impl FilesystemOpenApiDiscovery {
    pub fn new(root: impl Into<PathBuf>, base_url_override: Option<String>) -> Self {
        Self { root: root.into(), base_url_override, max_files: 5000 }
    }

    pub fn max_files(mut self, value: usize) -> Self {
        self.max_files = value.max(1);
        self
    }

    fn collect_files(&self) -> Vec<PathBuf> {
        fn walk(root: &Path, out: &mut Vec<PathBuf>, left: &mut usize) {
            if *left == 0 { return; }
            let Ok(entries) = fs::read_dir(root) else { return };
            for entry in entries.flatten() {
                if *left == 0 { break; }
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    if matches!(name, ".git" | "node_modules" | "target" | ".venv" | "venv" | "dist" | "build") {
                        continue;
                    }
                    walk(&path, out, left);
                } else if path.is_file() {
                    *left -= 1;
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        let mut left = self.max_files;
        walk(&self.root, &mut out, &mut left);
        out
    }

    fn operation_score(operation_id: &str, path: &str, capability: &str, method: &str) -> i64 {
        let desired = capability.to_ascii_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|v| !v.is_empty())
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let searchable = format!("{operation_id} {path}").to_ascii_lowercase();
        let mut score = desired.iter().filter(|term| searchable.contains(term.as_str())).count() as i64 * 20;
        if method.eq_ignore_ascii_case("get") { score += 20; }
        if operation_id.to_ascii_lowercase().contains("list") || operation_id.to_ascii_lowercase().contains("get") {
            score += 5;
        }
        score
    }

    fn requests_from_document(
        &self,
        file: &Path,
        document: Value,
        capability: &str,
    ) -> Vec<(i64, BuildRequest)> {
        let base_url = self.base_url_override.clone().or_else(|| {
            document.get("servers").and_then(Value::as_array)
                .and_then(|servers| servers.first())
                .and_then(Value::as_object)
                .and_then(|server| server.get("url"))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        let Some(base_url) = base_url else { return Vec::new() };
        let Some(paths) = document.get("paths").and_then(Value::as_object) else { return Vec::new() };
        let mut out = Vec::new();
        for (path_name, item) in paths {
            let Some(methods) = item.as_object() else { continue };
            for (method, operation) in methods {
                if !matches!(method.as_str(), "get" | "post" | "put" | "patch" | "delete") { continue; }
                let Some(operation_id) = operation.get("operationId").and_then(Value::as_str) else { continue };
                let has_required_parameter = operation.get("parameters")
                    .and_then(Value::as_array)
                    .is_some_and(|parameters| parameters.iter().any(|parameter| {
                        parameter.get("required").and_then(Value::as_bool) == Some(true)
                    }));
                let autonomously_invocable = !path_name.contains('{') && !has_required_parameter;
                let mut score = Self::operation_score(operation_id, path_name, capability, method);
                if autonomously_invocable { score += 40; } else { score -= 80; }
                let risk = if method == "get" { RiskClass::Medium } else { RiskClass::High };
                let id_suffix = operation_id.to_ascii_lowercase()
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                    .collect::<String>();
                let request = BuildRequest {
                    plugin_id: format!("generated.openapi.{id_suffix}"),
                    display_name: format!("Generated OpenAPI {operation_id}"),
                    capability: capability.to_string(),
                    source: AdapterSource::OpenApi {
                        base_url: base_url.clone(),
                        operation_id: operation_id.to_string(),
                        document: document.clone(),
                    },
                    risk,
                    requires_approval: risk >= RiskClass::High,
                    input_contract: None,
                    tags: ["generated".to_string(), "openapi".to_string(), format!("source:{}", file.display())]
                        .into_iter().collect(),
                };
                out.push((score, request));
            }
        }
        out
    }
}

#[async_trait]
impl BuildDiscovery for FilesystemOpenApiDiscovery {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError> {
        let mut ranked = Vec::new();
        for file in self.collect_files() {
            let name = file.file_name().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
            if !matches!(name.as_str(), "openapi.json" | "swagger.json" | "api.json") { continue; }
            let raw = match fs::read_to_string(&file) { Ok(value) => value, Err(_) => continue };
            if raw.len() > 2_000_000 { continue; }
            let Ok(document) = serde_json::from_str::<Value>(&raw) else { continue };
            if document.get("openapi").is_none() && document.get("swagger").is_none() { continue; }
            ranked.extend(self.requests_from_document(&file, document, &plan.gap.suggested_capability));
        }
        ranked.sort_by(|a,b| b.0.cmp(&a.0).then_with(|| a.1.plugin_id.cmp(&b.1.plugin_id)));
        Ok(ranked.into_iter().map(|(_,request)| request).collect())
    }
}

pub struct BuildDiscoveryChain {
    providers: Vec<Arc<dyn BuildDiscovery>>,
}

impl BuildDiscoveryChain {
    pub fn new(providers: Vec<Arc<dyn BuildDiscovery>>) -> Self {
        Self { providers }
    }

    pub fn push(&mut self, provider: Arc<dyn BuildDiscovery>) {
        self.providers.push(provider);
    }
}

#[async_trait]
impl BuildDiscovery for BuildDiscoveryChain {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError> {
        let mut all = Vec::new();
        for provider in &self.providers {
            all.extend(provider.discover(plan).await?);
        }
        Ok(all)
    }
}

pub struct FilesystemMcpDiscovery {
    root: PathBuf,
    runtime: Arc<dyn RuntimeAdapter>,
    allowed_programs: BTreeSet<String>,
    max_files: usize,
}

impl FilesystemMcpDiscovery {
    pub fn new(
        root: impl Into<PathBuf>,
        runtime: Arc<dyn RuntimeAdapter>,
        allowed_programs: BTreeSet<String>,
    ) -> Self {
        Self { root: root.into(), runtime, allowed_programs, max_files: 5000 }
    }

    fn collect_configs(&self) -> Vec<PathBuf> {
        fn walk(root: &Path, out: &mut Vec<PathBuf>, left: &mut usize) {
            if *left == 0 { return; }
            let Ok(entries) = fs::read_dir(root) else { return };
            for entry in entries.flatten() {
                if *left == 0 { break; }
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    if matches!(name, ".git" | "node_modules" | "target" | ".venv" | "venv" | "dist" | "build") {
                        continue;
                    }
                    walk(&path, out, left);
                } else if path.is_file() {
                    *left -= 1;
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
                    if matches!(name.as_str(), "mcp.json" | "mcp.config.json" | "mcp-server.json") {
                        out.push(path);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut left = self.max_files;
        walk(&self.root, &mut out, &mut left);
        out
    }

    async fn list_tools(
        &self,
        command: &str,
        args: &[String],
    ) -> Result<Vec<(String, String, Value)>, BuilderError> {
        if !self.allowed_programs.contains(command) {
            return Ok(Vec::new());
        }
        if invalid_program(command) || args.iter().any(|arg| contains_shell_meta(arg)) {
            return Ok(Vec::new());
        }
        let initialize = serde_json::json!({
            "jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"synergy-m8-discovery","version":"0.1.0"}}
        });
        let initialized = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        let list = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let spec = CommandSpec::new(command.to_string())
            .args(args.to_vec())
            .profile(ExecutionProfile::ReadOnlyWorkspace)
            .stdin_data(format!("{}\n{}\n{}\n", initialize, initialized, list));
        let result = match self.runtime.execute(spec).await {
            Ok(result) if result.exit_code == 0 => result,
            _ => return Ok(Vec::new()),
        };
        for line in result.stdout.lines() {
            let Ok(value) = serde_json::from_str::<Value>(line) else { continue };
            if value.get("id") != Some(&Value::from(2)) { continue; }
            let Some(items) = value.pointer("/result/tools").and_then(Value::as_array) else { continue };
            return Ok(items.iter().filter_map(|item| {
                let name = item.get("name").and_then(Value::as_str)?.to_string();
                let description = item.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                let input_schema = item.get("inputSchema").cloned()
                    .unwrap_or_else(|| serde_json::json!({
                        "type":"object",
                        "properties":{},
                        "additionalProperties":true
                    }));
                Some((name, description, input_schema))
            }).collect());
        }
        Ok(Vec::new())
    }

    fn semantic_score(plan: &SelfBuildPlan, name: &str, description: &str) -> i64 {
        let searchable = format!("{name} {description}").to_ascii_lowercase();
        let mut score = 0i64;
        for term in &plan.gap.terms {
            if searchable.contains(&term.to_ascii_lowercase()) { score += 15; }
        }
        for term in plan.gap.suggested_capability.split('.') {
            if searchable.contains(&term.to_ascii_lowercase()) { score += 25; }
        }
        score
    }

    fn tool_risk(name: &str) -> RiskClass {
        let lower = name.to_ascii_lowercase();
        if ["write","create","update","delete","remove","send","publish","execute","run","kill","install"]
            .iter().any(|needle| lower.contains(needle))
        {
            RiskClass::High
        } else {
            RiskClass::Medium
        }
    }
}

#[async_trait]
impl BuildDiscovery for FilesystemMcpDiscovery {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError> {
        let mut ranked = Vec::<(i64, BuildRequest)>::new();
        for config in self.collect_configs() {
            let Ok(raw) = fs::read_to_string(&config) else { continue };
            if raw.len() > 1_000_000 { continue; }
            let Ok(document) = serde_json::from_str::<Value>(&raw) else { continue };
            let Some(servers) = document.get("mcpServers").and_then(Value::as_object) else { continue };
            for (server_name, server) in servers {
                let Some(command) = server.get("command").and_then(Value::as_str) else { continue };
                let args = server.get("args").and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
                    .unwrap_or_default();
                let tools = self.list_tools(command, &args).await?;
                for (tool, description, input_schema) in tools {
                    let score = Self::semantic_score(plan, &tool, &description);
                    if score <= 0 { continue; }
                    let risk = Self::tool_risk(&tool);
                    let id_suffix = format!("{server_name}-{tool}").to_ascii_lowercase()
                        .chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                        .collect::<String>();
                    ranked.push((score, BuildRequest {
                        plugin_id: format!("generated.mcp.{id_suffix}"),
                        display_name: format!("Generated MCP {server_name}/{tool}"),
                        capability: plan.gap.suggested_capability.clone(),
                        source: AdapterSource::McpStdio {
                            command: command.to_string(),
                            args: args.clone(),
                            tool,
                        },
                        risk,
                        requires_approval: risk >= RiskClass::High,
                        input_contract: Some(input_schema),
                        tags: ["generated".to_string(), "mcp".to_string(), format!("server:{server_name}")]
                            .into_iter().collect(),
                    }));
                }
            }
        }
        ranked.sort_by(|a,b| b.0.cmp(&a.0).then_with(|| a.1.plugin_id.cmp(&b.1.plugin_id)));
        Ok(ranked.into_iter().map(|(_,request)| request).collect())
    }
}

#[derive(Clone, Debug)]
pub struct FilesystemCliDescriptorDiscovery {
    root: PathBuf,
    max_files: usize,
}

impl FilesystemCliDescriptorDiscovery {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), max_files: 5000 }
    }

    fn descriptors(&self) -> Vec<PathBuf> {
        fn walk(root: &Path, out: &mut Vec<PathBuf>, left: &mut usize) {
            if *left == 0 { return; }
            let Ok(entries) = fs::read_dir(root) else { return };
            for entry in entries.flatten() {
                if *left == 0 { break; }
                let path = entry.path();
                if path.is_dir() {
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    if matches!(name, ".git" | "node_modules" | "target" | ".venv" | "venv" | "dist" | "build") {
                        continue;
                    }
                    walk(&path, out, left);
                } else if path.is_file() {
                    *left -= 1;
                    let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
                    if matches!(name, "synergy.cli.json" | "cli-capabilities.json") {
                        out.push(path);
                    }
                }
            }
        }
        let mut out = Vec::new();
        let mut left = self.max_files;
        walk(&self.root, &mut out, &mut left);
        out
    }
}

#[async_trait]
impl BuildDiscovery for FilesystemCliDescriptorDiscovery {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError> {
        let mut out = Vec::new();
        for descriptor in self.descriptors() {
            let Ok(raw) = fs::read_to_string(&descriptor) else { continue };
            if raw.len() > 1_000_000 { continue; }
            let Ok(document) = serde_json::from_str::<Value>(&raw) else { continue };
            if document.get("schema").and_then(Value::as_str) != Some("synergy.cli-capabilities.v1") {
                continue;
            }
            let Some(commands) = document.get("commands").and_then(Value::as_array) else { continue };
            for command in commands {
                let Some(program) = command.get("program").and_then(Value::as_str) else { continue };
                let Some(capability) = command.get("capability").and_then(Value::as_str) else { continue };
                if capability != plan.gap.suggested_capability {
                    continue;
                }
                let args = command.get("args").and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
                    .unwrap_or_default();
                let input_contract = command.get("input_schema").cloned()
                    .unwrap_or_else(|| serde_json::json!({
                        "type":"object","properties":{},"additionalProperties":true
                    }));
                let risk = match command.get("risk").and_then(Value::as_str).unwrap_or("Medium") {
                    "Low" => RiskClass::Low,
                    "Medium" => RiskClass::Medium,
                    "High" => RiskClass::High,
                    "Critical" => RiskClass::Critical,
                    _ => RiskClass::High,
                };
                let requires_approval = command.get("requires_approval").and_then(Value::as_bool)
                    .unwrap_or(risk >= RiskClass::High);
                let tags = command.get("tags").and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_else(BTreeSet::new);
                let suffix = capability.to_ascii_lowercase()
                    .chars().map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
                    .collect::<String>();
                out.push(BuildRequest {
                    plugin_id: format!("generated.cli.{suffix}"),
                    display_name: format!("Generated CLI {capability}"),
                    capability: capability.to_string(),
                    source: AdapterSource::Cli { program: program.to_string(), args },
                    risk,
                    requires_approval,
                    input_contract: Some(input_contract),
                    tags,
                });
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Default)]
pub struct StaticBuildDiscovery {
    pub requests: Vec<BuildRequest>,
}

#[async_trait]
impl BuildDiscovery for StaticBuildDiscovery {
    async fn discover(&self, plan: &SelfBuildPlan) -> Result<Vec<BuildRequest>, BuilderError> {
        Ok(self.requests.iter()
            .filter(|request| request.capability == plan.gap.suggested_capability)
            .cloned()
            .collect())
    }
}

#[derive(Clone, Debug)]
pub struct SelfBuildPolicy {
    pub auto_promote: bool,
    pub max_auto_promote_risk: RiskClass,
}

impl Default for SelfBuildPolicy {
    fn default() -> Self {
        Self { auto_promote: true, max_auto_promote_risk: RiskClass::Medium }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SelfBuildOutcome {
    ExistingProvider,
    DiscoveryEmpty { plan: SelfBuildPlan },
    CandidateRejected { candidate: PluginCandidate },
    QualifiedAwaitingPromotion { candidate: PluginCandidate },
    Promoted { candidate: PluginCandidate, manifest_path: PathBuf },
}

pub struct SelfBuildCoordinator<Q: CandidateQualifier, D: BuildDiscovery> {
    store: CandidateStore,
    qualifier: Q,
    discovery: D,
    policy: SelfBuildPolicy,
    generated_plugins_root: PathBuf,
}

impl<Q: CandidateQualifier, D: BuildDiscovery> SelfBuildCoordinator<Q, D> {
    pub fn new(
        store: CandidateStore,
        qualifier: Q,
        discovery: D,
        policy: SelfBuildPolicy,
        generated_plugins_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            store,
            qualifier,
            discovery,
            policy,
            generated_plugins_root: generated_plugins_root.into(),
        }
    }

    pub async fn ensure_for_goal(
        &self,
        graph: &CapabilityGraph,
        goal: &str,
    ) -> Result<SelfBuildOutcome, BuilderError> {
        let Some(plan) = SemanticGapDetector::self_build_plan(graph, goal) else {
            return Ok(SelfBuildOutcome::ExistingProvider);
        };
        self.ensure_plan(plan).await
    }

    pub async fn ensure_typed_for_goal(
        &self,
        graph: &CapabilityGraph,
        registry: &PluginRegistry,
        goal: &str,
    ) -> Result<SelfBuildOutcome, BuilderError> {
        let Some(plan) = SemanticGapDetector::self_build_plan_typed(graph, registry, goal) else {
            return Ok(SelfBuildOutcome::ExistingProvider);
        };
        self.ensure_plan(plan).await
    }

    async fn ensure_plan(&self, plan: SelfBuildPlan) -> Result<SelfBuildOutcome, BuilderError> {
        let requests = self.discovery.discover(&plan).await?;
        if requests.is_empty() {
            return Ok(SelfBuildOutcome::DiscoveryEmpty { plan });
        }

        let mut last_rejected = None;
        let mut awaiting_promotion = None;
        for request in requests {
            let candidate = match PluginBuilder::build(request) {
                Ok(candidate) => candidate,
                Err(BuilderError::Invalid(_)) => continue,
                Err(error) => return Err(error),
            };
            let id = candidate.candidate_id;
            self.store.save(&candidate)?;
            let candidate = self.store.qualify(id, &self.qualifier).await?;
            if candidate.state != CandidateState::Qualified {
                last_rejected = Some(candidate);
                continue;
            }

            let capability = candidate.manifest.capabilities.first()
                .ok_or_else(|| BuilderError::Invalid("generated candidate exposes no capability".into()))?;
            let auto_promotable = self.policy.auto_promote
                && capability.risk <= self.policy.max_auto_promote_risk
                && !capability.requires_approval;

            if !auto_promotable {
                if awaiting_promotion.is_none() {
                    awaiting_promotion = Some(candidate);
                }
                continue;
            }

            let manifest_path = self.store.promote(id, &self.generated_plugins_root)?;
            let candidate = self.store.load(id)?;
            return Ok(SelfBuildOutcome::Promoted { candidate, manifest_path });
        }

        if let Some(candidate) = awaiting_promotion {
            Ok(SelfBuildOutcome::QualifiedAwaitingPromotion { candidate })
        } else if let Some(candidate) = last_rejected {
            Ok(SelfBuildOutcome::CandidateRejected { candidate })
        } else {
            Ok(SelfBuildOutcome::DiscoveryEmpty { plan })
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use autopilot_runtime::{CommandResult, FakeRuntime};

    #[test]
    fn detects_semantic_gap_when_graph_has_no_provider() {
        let graph = CapabilityGraph::default();
        let gap = SemanticGapDetector::detect(&graph, "прочитай неизвестную CRM").unwrap();
        assert!(!gap.suggested_capability.is_empty());
    }

    #[test]
    fn infers_action_specific_capabilities() {
        let email_terms = tokenize("отправь email клиенту");
        assert_eq!(infer_capability(&email_terms), "email.send");
        let crm_terms = tokenize("создай запись crm");
        assert_eq!(infer_capability(&crm_terms), "crm.record.create");
        let browser_terms = tokenize("открой сайт");
        assert_eq!(infer_capability(&browser_terms), "browser.navigate");
    }

    #[test]
    fn openapi_contract_keeps_path_and_required_body() {
        let source = AdapterSource::OpenApi {
            base_url: "https://api.example.com".into(),
            operation_id: "updateLead".into(),
            document: serde_json::json!({
                "openapi":"3.0.0",
                "paths":{
                    "/leads/{id}":{
                        "parameters":[
                            {"name":"id","in":"path","required":true,"schema":{"type":"string"}}
                        ],
                        "post":{
                            "operationId":"updateLead",
                            "requestBody":{
                                "required":true,
                                "content":{
                                    "application/json":{
                                        "schema":{
                                            "type":"object",
                                            "properties":{"name":{"type":"string"}},
                                            "required":["name"]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }),
        };
        let contract = derive_input_contract(&source);
        assert_eq!(contract["properties"]["id"]["x-location"], "path");
        assert!(contract["required"].as_array().unwrap().iter().any(|v| v.as_str() == Some("id")));
        assert!(contract["required"].as_array().unwrap().iter().any(|v| v.as_str() == Some("body")));
        assert_eq!(contract["properties"]["body"]["required"][0].as_str(), Some("name"));
    }

    #[test]
    fn typed_gap_ignores_untyped_provider() {
        let candidate = PluginBuilder::build(BuildRequest {
            plugin_id: "generated.crm.read".into(),
            display_name: "CRM reader".into(),
            capability: "crm.read".into(),
            source: AdapterSource::OpenApi {
                base_url: "https://api.example.com".into(),
                operation_id: "listLeads".into(),
                document: serde_json::json!({
                    "paths": {"/leads": {"get": {"operationId": "listLeads"}}}
                }),
            },
            risk: RiskClass::Medium,
            requires_approval: false,
            input_contract: None,
            tags: ["crm".to_string()].into_iter().collect(),
        }).unwrap();

        let mut untyped = candidate.manifest.clone();
        untyped.input_contracts.clear();
        let mut registry = PluginRegistry::default();
        registry.register(untyped).unwrap();
        let mut graph = CapabilityGraph::default();
        graph.attach_plugins(&registry);
        assert!(SemanticGapDetector::detect_typed(&graph, &registry, "прочитай CRM").is_some());

        registry.upsert(candidate.manifest).unwrap();
        graph.attach_plugins(&registry);
        assert!(SemanticGapDetector::detect_typed(&graph, &registry, "прочитай CRM").is_none());
    }

    #[test]
    fn generates_openapi_candidate_disabled_by_default() {
        let source = AdapterSource::OpenApi {
            base_url: "https://api.example.com".into(),
            operation_id: "getLead".into(),
            document: serde_json::json!({
                "paths": {"/leads/{id}": {"get": {"operationId": "getLead"}}}
            }),
        };
        let candidate = PluginBuilder::build(BuildRequest {
            plugin_id: "generated.example.crm".into(),
            display_name: "Generated Example CRM".into(),
            capability: "crm.read".into(),
            source,
            risk: RiskClass::Medium,
            requires_approval: false,
            input_contract: None,
            tags: ["crm".to_string()].into_iter().collect(),
        }).unwrap();
        assert!(!candidate.manifest.enabled_by_default);
        assert!(candidate.manifest.requires.contains("connector.gateway"));
    }

    #[test]
    fn shell_metacharacters_are_rejected() {
        let result = PluginBuilder::build(BuildRequest {
            plugin_id: "generated.bad.cli".into(),
            display_name: "Bad CLI".into(),
            capability: "custom.bad.read".into(),
            source: AdapterSource::Cli {
                program: "tool;rm".into(),
                args: vec![],
            },
            risk: RiskClass::Low,
            requires_approval: false,
            input_contract: None,
            tags: BTreeSet::new(),
        });
        assert!(matches!(result, Err(BuilderError::Invalid(_))));
    }

    #[test]
    fn quarantined_candidate_cannot_be_promoted() {
        let candidate = PluginBuilder::build(BuildRequest {
            plugin_id: "generated.safe.openapi".into(),
            display_name: "Safe OpenAPI".into(),
            capability: "custom.safe.read".into(),
            source: AdapterSource::OpenApi {
                base_url: "https://api.example.com".into(),
                operation_id: "status".into(),
                document: serde_json::json!({
                    "paths": {"/status": {"get": {"operationId": "status"}}}
                }),
            },
            risk: RiskClass::Low,
            requires_approval: false,
            input_contract: None,
            tags: BTreeSet::new(),
        }).unwrap();
        let root = std::env::temp_dir().join(format!("autopilot-candidate-test-{}", Uuid::new_v4()));
        let generated = root.join("generated");
        let store = CandidateStore::new(root.join("quarantine"));
        store.save(&candidate).unwrap();
        assert!(matches!(
            store.promote(candidate.candidate_id, &generated),
            Err(BuilderError::NotQualified)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn filesystem_cli_descriptor_builds_typed_candidate() {
        let root = std::env::temp_dir().join(format!("autopilot-cli-discovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("synergy.cli.json"),
            serde_json::json!({
                "schema":"synergy.cli-capabilities.v1",
                "commands":[{
                    "program":"lead-tool",
                    "capability":"crm.read",
                    "args":["--json"],
                    "risk":"Medium",
                    "input_schema":{
                        "type":"object",
                        "properties":{"query":{"type":"string"}},
                        "required":["query"],
                        "additionalProperties":false
                    }
                }]
            }).to_string(),
        ).unwrap();
        let discovery = FilesystemCliDescriptorDiscovery::new(&root);
        let plan = SelfBuildPlan {
            gap: SemanticGap {
                goal:"прочитай CRM".into(),
                suggested_capability:"crm.read".into(),
                terms:["crm".to_string(),"read".to_string()].into_iter().collect(),
                reason:"test".into(),
            },
            repository_candidates:Vec::new(),
            strategies:vec![DiscoveryStrategy::CliEntrypoint],
            qualification_gates:Vec::new(),
            auto_execution_allowed:false,
        };
        let requests = discovery.discover(&plan).await.unwrap();
        assert_eq!(requests.len(),1);
        assert_eq!(requests[0].input_contract.as_ref().unwrap()["required"][0],"query");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn filesystem_openapi_discovery_prefers_parameterless_get() {
        let root = std::env::temp_dir().join(format!("autopilot-openapi-discovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("openapi.json"),
            serde_json::json!({
                "openapi":"3.0.0",
                "servers":[{"url":"https://api.example.com"}],
                "paths":{
                    "/leads":{
                        "get":{"operationId":"listLeads"},
                        "post":{"operationId":"createLead"}
                    },
                    "/leads/{id}":{
                        "get":{
                            "operationId":"getLead",
                            "parameters":[{"name":"id","in":"path","required":true}]
                        }
                    }
                }
            }).to_string(),
        ).unwrap();
        let discovery = FilesystemOpenApiDiscovery::new(&root, None);
        let plan = SemanticGapDetector::self_build_plan(
            &CapabilityGraph::default(),
            "прочитай CRM",
        ).unwrap();
        let requests = discovery.discover(&plan).await.unwrap();
        assert!(!requests.is_empty());
        assert!(matches!(
            requests[0].source,
            AdapterSource::OpenApi { ref operation_id, .. } if operation_id == "listLeads"
        ));
        assert_eq!(requests[0].risk, RiskClass::Medium);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn filesystem_mcp_discovery_uses_tools_list_and_allowlist() {
        let root = std::env::temp_dir().join(format!("autopilot-mcp-discovery-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("mcp.json"),
            serde_json::json!({
                "mcpServers":{
                    "crm":{
                        "command":"crm-mcp",
                        "args":["--stdio"]
                    }
                }
            }).to_string(),
        ).unwrap();
        let stdout = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"crm_read\",\"description\":\"Read CRM leads\"}]}}\n"
        );
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
            duration_ms: 1,
        }));
        let discovery = FilesystemMcpDiscovery::new(
            &root,
            runtime,
            ["crm-mcp".to_string()].into_iter().collect(),
        );
        let plan = SemanticGapDetector::self_build_plan(
            &CapabilityGraph::default(),
            "прочитай CRM",
        ).unwrap();
        let requests = discovery.discover(&plan).await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(matches!(
            requests[0].source,
            AdapterSource::McpStdio { ref tool, .. } if tool == "crm_read"
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn self_build_coordinator_auto_promotes_safe_openapi_candidate() {
        let root = std::env::temp_dir().join(format!("autopilot-self-build-{}", Uuid::new_v4()));
        let request = BuildRequest {
            plugin_id: "generated.crm.read".into(),
            display_name: "Generated CRM Reader".into(),
            capability: "crm.read".into(),
            source: AdapterSource::OpenApi {
                base_url: "https://api.example.com".into(),
                operation_id: "listLeads".into(),
                document: serde_json::json!({
                    "paths": {"/leads": {"get": {"operationId": "listLeads"}}}
                }),
            },
            risk: RiskClass::Medium,
            requires_approval: false,
            input_contract: None,
            tags: ["crm".to_string()].into_iter().collect(),
        };
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 1,
        }));
        let coordinator = SelfBuildCoordinator::new(
            CandidateStore::new(root.join("quarantine")),
            SandboxQualifier::new(runtime, BTreeSet::new()),
            StaticBuildDiscovery { requests: vec![request] },
            SelfBuildPolicy::default(),
            root.join("generated"),
        );
        let outcome = coordinator.ensure_for_goal(
            &CapabilityGraph::default(),
            "прочитай CRM",
        ).await.unwrap();
        match outcome {
            SelfBuildOutcome::Promoted { manifest_path, candidate } => {
                assert!(manifest_path.is_file());
                assert_eq!(candidate.state, CandidateState::Promoted);
            }
            other => panic!("unexpected self-build outcome: {other:?}"),
        }
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn high_risk_self_build_requires_explicit_promotion() {
        let root = std::env::temp_dir().join(format!("autopilot-self-build-high-{}", Uuid::new_v4()));
        let request = BuildRequest {
            plugin_id: "generated.crm.write".into(),
            display_name: "Generated CRM Writer".into(),
            capability: "crm.read".into(),
            source: AdapterSource::OpenApi {
                base_url: "https://api.example.com".into(),
                operation_id: "updateLead".into(),
                document: serde_json::json!({
                    "paths": {"/leads": {"post": {"operationId": "updateLead"}}}
                }),
            },
            risk: RiskClass::High,
            requires_approval: true,
            input_contract: None,
            tags: ["crm".to_string()].into_iter().collect(),
        };
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 1,
        }));
        let coordinator = SelfBuildCoordinator::new(
            CandidateStore::new(root.join("quarantine")),
            SandboxQualifier::new(runtime, BTreeSet::new()),
            StaticBuildDiscovery { requests: vec![request] },
            SelfBuildPolicy::default(),
            root.join("generated"),
        );
        let outcome = coordinator.ensure_for_goal(
            &CapabilityGraph::default(),
            "прочитай CRM",
        ).await.unwrap();
        assert!(matches!(outcome, SelfBuildOutcome::QualifiedAwaitingPromotion { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unallowlisted_cli_candidate_is_rejected_by_sandbox() {
        let runtime = Arc::new(FakeRuntime::new(CommandResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 1,
        }));
        let qualifier = SandboxQualifier::new(runtime, BTreeSet::new());
        let candidate = PluginBuilder::build(BuildRequest {
            plugin_id: "generated.cli.test".into(),
            display_name: "CLI test".into(),
            capability: "custom.test.read".into(),
            source: AdapterSource::Cli {
                program: "unknown-tool".into(),
                args: vec!["--version".into()],
            },
            risk: RiskClass::Low,
            requires_approval: false,
            input_contract: None,
            tags: BTreeSet::new(),
        }).unwrap();
        let report = qualifier.qualify(&candidate).await;
        assert!(!report.passed());
    }
}
