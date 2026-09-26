use std::collections::{BTreeMap, BTreeSet};

use autopilot_domain::{Capability, RiskClass};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PLUGIN_SCHEMA_V1: &str = "synergy.plugin.v1";

pub fn parse_manifest_json(input: &str) -> Result<PluginManifest, PluginError> {
    let manifest: PluginManifest =
        serde_json::from_str(input).map_err(|error| PluginError::InvalidManifest(error.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Mcp,
    RemoteMcp,
    Http,
    OpenApi,
    Cli,
    Filesystem,
    Browser,
    Desktop,
    Mobile,
    Database,
    Webhook,
    Queue,
    Iot,
    Model,
    Memory,
    Business,
    Worker,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginEndpoint {
    pub transport: String,
    pub address: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAdapter {
    OpenApi {
        base_url: String,
        operation_id: String,
        document: serde_json::Value,
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginCapability {
    pub name: String,
    pub risk: RiskClass,
    pub requires_approval: bool,
    #[serde(default)]
    pub mapped_core_capabilities: BTreeSet<Capability>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub schema: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: PluginKind,
    pub provider: String,
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    #[serde(default)]
    pub endpoints: Vec<PluginEndpoint>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub requires: BTreeSet<String>,
    #[serde(default)]
    pub enabled_by_default: bool,
    #[serde(default)]
    pub adapter: Option<PluginAdapter>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), PluginError> {
        if self.schema != PLUGIN_SCHEMA_V1 {
            return Err(PluginError::UnsupportedSchema(self.schema.clone()));
        }
        if self.id.trim().is_empty() {
            return Err(PluginError::InvalidManifest("plugin id is empty".into()));
        }
        if self.name.trim().is_empty() {
            return Err(PluginError::InvalidManifest("plugin name is empty".into()));
        }
        if self.version.trim().is_empty() {
            return Err(PluginError::InvalidManifest("plugin version is empty".into()));
        }
        if let Some(adapter) = &self.adapter {
            if self.enabled_by_default {
                return Err(PluginError::InvalidManifest(
                    "plugins with generated adapter specs must be disabled by default".into()
                ));
            }
            match adapter {
                PluginAdapter::OpenApi { base_url, operation_id, document } => {
                    if self.kind != PluginKind::OpenApi {
                        return Err(PluginError::InvalidManifest("OpenAPI adapter requires kind=open_api".into()));
                    }
                    let lower = base_url.to_ascii_lowercase();
                    if !(lower.starts_with("https://")
                        || lower.starts_with("http://127.0.0.1")
                        || lower.starts_with("http://localhost"))
                    {
                        return Err(PluginError::InvalidManifest(
                            "OpenAPI adapter base_url must use HTTPS except loopback".into()
                        ));
                    }
                    if operation_id.trim().is_empty() {
                        return Err(PluginError::InvalidManifest("OpenAPI adapter operation_id is empty".into()));
                    }
                    let found = document.get("paths").and_then(serde_json::Value::as_object).is_some_and(|paths| {
                        paths.values().filter_map(serde_json::Value::as_object).any(|methods| {
                            methods.values().filter_map(serde_json::Value::as_object).any(|operation| {
                                operation.get("operationId").and_then(serde_json::Value::as_str)
                                    == Some(operation_id.as_str())
                            })
                        })
                    });
                    if !found {
                        return Err(PluginError::InvalidManifest(
                            format!("OpenAPI adapter operationId not found: {operation_id}")
                        ));
                    }
                }
                PluginAdapter::McpStdio { command, args, tool } => {
                    if self.kind != PluginKind::Mcp {
                        return Err(PluginError::InvalidManifest("MCP adapter requires kind=mcp".into()));
                    }
                    if invalid_program(command) || args.iter().any(|arg| contains_shell_meta(arg)) {
                        return Err(PluginError::InvalidManifest("unsafe MCP command or arguments".into()));
                    }
                    if tool.trim().is_empty() {
                        return Err(PluginError::InvalidManifest("MCP adapter tool is empty".into()));
                    }
                }
                PluginAdapter::Cli { program, args } => {
                    if self.kind != PluginKind::Cli {
                        return Err(PluginError::InvalidManifest("CLI adapter requires kind=cli".into()));
                    }
                    if invalid_program(program) || args.iter().any(|arg| contains_shell_meta(arg)) {
                        return Err(PluginError::InvalidManifest("unsafe CLI program or arguments".into()));
                    }
                }
            }
        }

        let mut seen = BTreeSet::new();
        for capability in &self.capabilities {
            if capability.name.trim().is_empty() {
                return Err(PluginError::InvalidManifest("capability name is empty".into()));
            }
            if !seen.insert(capability.name.clone()) {
                return Err(PluginError::DuplicateCapability(capability.name.clone()));
            }
        }
        Ok(())
    }

    pub fn capability(&self, name: &str) -> Option<&PluginCapability> {
        self.capabilities.iter().find(|c| c.name == name)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PluginRegistry {
    manifests: BTreeMap<String, PluginManifest>,
}

impl PluginRegistry {
    pub fn register(&mut self, manifest: PluginManifest) -> Result<(), PluginError> {
        manifest.validate()?;
        if self.manifests.contains_key(&manifest.id) {
            return Err(PluginError::DuplicatePlugin(manifest.id));
        }
        self.manifests.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    pub fn upsert(&mut self, manifest: PluginManifest) -> Result<(), PluginError> {
        manifest.validate()?;
        self.manifests.insert(manifest.id.clone(), manifest);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&PluginManifest> {
        self.manifests.get(id)
    }

    pub fn list(&self) -> impl Iterator<Item = &PluginManifest> {
        self.manifests.values()
    }

    pub fn providers_for_capability(&self, capability: &str) -> Vec<&PluginManifest> {
        self.manifests
            .values()
            .filter(|manifest| manifest.capability(capability).is_some())
            .collect()
    }

    pub fn validate_dependencies(&self) -> Result<(), PluginError> {
        for manifest in self.manifests.values() {
            for dependency in &manifest.requires {
                if !self.manifests.contains_key(dependency) {
                    return Err(PluginError::MissingDependency {
                        plugin: manifest.id.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn contains_shell_meta(value: &str) -> bool {
    [';', '|', '&', '>', '<', '`', '\n', '\r'].iter().any(|c| value.contains(*c))
}

fn invalid_program(value: &str) -> bool {
    value.trim().is_empty() || contains_shell_meta(value)
}


#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginError {
    #[error("unsupported plugin schema: {0}")]
    UnsupportedSchema(String),
    #[error("invalid plugin manifest: {0}")]
    InvalidManifest(String),
    #[error("duplicate plugin id: {0}")]
    DuplicatePlugin(String),
    #[error("duplicate capability: {0}")]
    DuplicateCapability(String),
    #[error("plugin {plugin} requires missing dependency {dependency}")]
    MissingDependency { plugin: String, dependency: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &str, capability: &str) -> PluginManifest {
        PluginManifest {
            schema: PLUGIN_SCHEMA_V1.into(),
            id: id.into(),
            name: id.into(),
            version: "1.0.0".into(),
            kind: PluginKind::Mcp,
            provider: "synergy".into(),
            capabilities: vec![PluginCapability {
                name: capability.into(),
                risk: RiskClass::Low,
                requires_approval: false,
                mapped_core_capabilities: BTreeSet::new(),
            }],
            endpoints: vec![],
            tags: BTreeSet::new(),
            requires: BTreeSet::new(),
            enabled_by_default: false,
            adapter: None,
        }
    }

    #[test]
    fn parses_repository_manifests() {
        for raw in [
            include_str!("../../../plugins/local-computer/plugin.json"),
            include_str!("../../../plugins/remote-computer/plugin.json"),
            include_str!("../../../plugins/business-runtime/plugin.json"),
            include_str!("../../../plugins/nexus/plugin.json"),
            include_str!("../../../plugins/agent-fabric/plugin.json"),
            include_str!("../../../plugins/auto-company/plugin.json"),
            include_str!("../../../plugins/browser/plugin.json"),
            include_str!("../../../plugins/connector-gateway/plugin.json"),
            include_str!("../../../plugins/email/plugin.json"),
            include_str!("../../../plugins/calendar/plugin.json"),
            include_str!("../../../plugins/crm/plugin.json"),
        ] {
            parse_manifest_json(raw).unwrap();
        }
    }

    #[test]
    fn registers_and_discovers_capability_provider() {
        let mut registry = PluginRegistry::default();
        registry.register(manifest("local-computer", "computer.read_file")).unwrap();
        let providers = registry.providers_for_capability("computer.read_file");
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "local-computer");
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut registry = PluginRegistry::default();
        registry.register(manifest("same", "a")).unwrap();
        assert_eq!(
            registry.register(manifest("same", "b")),
            Err(PluginError::DuplicatePlugin("same".into()))
        );
    }

    #[test]
    fn generated_adapter_must_be_disabled_by_default() {
        let mut item = manifest("generated", "custom.read");
        item.kind = PluginKind::Cli;
        item.enabled_by_default = true;
        item.adapter = Some(PluginAdapter::Cli {
            program: "safe-tool".into(),
            args: vec!["--version".into()],
        });
        assert!(matches!(item.validate(), Err(PluginError::InvalidManifest(_))));
    }

    #[test]
    fn generated_cli_rejects_shell_metacharacters() {
        let mut item = manifest("generated", "custom.read");
        item.kind = PluginKind::Cli;
        item.adapter = Some(PluginAdapter::Cli {
            program: "tool;evil".into(),
            args: vec![],
        });
        assert!(matches!(item.validate(), Err(PluginError::InvalidManifest(_))));
    }

    #[test]
    fn generated_openapi_requires_https_or_loopback() {
        let mut item = manifest("generated", "custom.read");
        item.kind = PluginKind::OpenApi;
        item.adapter = Some(PluginAdapter::OpenApi {
            base_url: "http://example.com".into(),
            operation_id: "status".into(),
            document: serde_json::json!({
                "paths": {"/status": {"get": {"operationId": "status"}}}
            }),
        });
        assert!(matches!(item.validate(), Err(PluginError::InvalidManifest(_))));
    }

    #[test]
    fn dependency_validation_fails_closed() {
        let mut registry = PluginRegistry::default();
        let mut item = manifest("remote-computer", "computer.remote.run");
        item.requires.insert("transport.shttps".into());
        registry.register(item).unwrap();
        assert_eq!(
            registry.validate_dependencies(),
            Err(PluginError::MissingDependency {
                plugin: "remote-computer".into(),
                dependency: "transport.shttps".into(),
            })
        );
    }
}
