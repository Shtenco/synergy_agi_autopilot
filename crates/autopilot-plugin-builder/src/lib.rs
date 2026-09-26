use std::collections::{BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use autopilot_capability_graph::CapabilityGraph;
use autopilot_domain::{Capability, RiskClass};
use autopilot_plugins::{PluginAdapter, PluginCapability, PluginEndpoint, PluginKind, PluginManifest, PLUGIN_SCHEMA_V1};
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
    pub fn detect(graph: &CapabilityGraph, goal: &str) -> Option<SemanticGap> {
        if !graph.route_goal(goal, 1).is_empty() {
            return None;
        }
        let terms = tokenize(goal);
        Some(SemanticGap {
            goal: goal.to_string(),
            suggested_capability: infer_capability(&terms),
            terms,
            reason: "no executable provider matched the semantic goal".into(),
        })
    }

    pub fn self_build_plan(graph: &CapabilityGraph, goal: &str) -> Option<SelfBuildPlan> {
        let gap = Self::detect(graph, goal)?;
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

        Some(SelfBuildPlan {
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
        })
    }
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
    for (needle, capability) in [
        ("email", "email.read"),
        ("почт", "email.read"),
        ("calendar", "calendar.read"),
        ("календар", "calendar.read"),
        ("crm", "crm.read"),
        ("database", "database.query"),
        ("баз", "database.query"),
        ("browser", "browser.snapshot"),
        ("сайт", "browser.snapshot"),
        ("file", "computer.read_file"),
        ("файл", "computer.read_file"),
        ("memory", "memory.search"),
        ("памят", "memory.search"),
    ] {
        if joined.contains(needle) {
            return capability.into();
        }
    }
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
            tags: BTreeSet::new(),
        }).unwrap();
        let report = qualifier.qualify(&candidate).await;
        assert!(!report.passed());
    }
}
