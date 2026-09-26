use std::collections::{BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use autopilot_capability_graph::CapabilityGraph;
use autopilot_domain::{Capability, RiskClass};
use autopilot_plugins::{PluginCapability, PluginEndpoint, PluginKind, PluginManifest, PLUGIN_SCHEMA_V1};
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

#[derive(Clone, Debug)]
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
