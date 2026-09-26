use std::collections::{BTreeMap, BTreeSet};

use autopilot_plugins::PluginRegistry;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDescriptor {
    pub name: String,
    pub full_name: String,
    pub default_branch: String,
    pub visibility: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    #[serde(default)]
    pub transports: BTreeSet<String>,
    #[serde(default)]
    pub plugin_ids: BTreeSet<String>,
    #[serde(default)]
    pub evidence: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryCatalog {
    pub schema: String,
    pub owner: String,
    pub repository_count: usize,
    #[serde(default)]
    pub repositories: Vec<RepositoryDescriptor>,
}

impl RepositoryCatalog {
    pub fn into_graph(self) -> Result<CapabilityGraph, CapabilityGraphError> {
        let mut graph = CapabilityGraph::default();
        for repository in self.repositories {
            graph.add_repository(repository)?;
        }
        Ok(graph)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityNode {
    pub name: String,
    pub providers: BTreeSet<String>,
    pub repositories: BTreeSet<String>,
    pub transports: BTreeSet<String>,
    pub tags: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGraph {
    pub repositories: BTreeMap<String, RepositoryDescriptor>,
    pub capabilities: BTreeMap<String, CapabilityNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityQuery {
    pub capability: Option<String>,
    pub keywords: Vec<String>,
    pub require_transport: Option<String>,
    pub include_archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCandidate {
    pub repository: String,
    pub score: i64,
    pub matched_capabilities: BTreeSet<String>,
    pub matched_tags: BTreeSet<String>,
    pub transports: BTreeSet<String>,
}

#[derive(Debug, Error)]
pub enum CapabilityGraphError {
    #[error("duplicate repository: {0}")]
    DuplicateRepository(String),
}

impl CapabilityGraph {
    pub fn add_repository(&mut self, repository: RepositoryDescriptor) -> Result<(), CapabilityGraphError> {
        if self.repositories.contains_key(&repository.full_name) {
            return Err(CapabilityGraphError::DuplicateRepository(repository.full_name));
        }
        let repo_name = repository.full_name.clone();
        for capability in &repository.capabilities {
            let node = self.capabilities.entry(capability.clone()).or_insert_with(|| CapabilityNode {
                name: capability.clone(),
                providers: BTreeSet::new(),
                repositories: BTreeSet::new(),
                transports: BTreeSet::new(),
                tags: BTreeSet::new(),
            });
            node.repositories.insert(repo_name.clone());
            node.transports.extend(repository.transports.iter().cloned());
            node.tags.extend(repository.tags.iter().cloned());
        }
        self.repositories.insert(repo_name, repository);
        Ok(())
    }

    pub fn attach_plugins(&mut self, registry: &PluginRegistry) {
        for plugin in registry.list() {
            let provider = plugin.provider.clone();
            for capability in &plugin.capabilities {
                let node = self.capabilities.entry(capability.name.clone()).or_insert_with(|| CapabilityNode {
                    name: capability.name.clone(),
                    providers: BTreeSet::new(),
                    repositories: BTreeSet::new(),
                    transports: BTreeSet::new(),
                    tags: BTreeSet::new(),
                });
                node.providers.insert(plugin.id.clone());
                node.tags.extend(plugin.tags.iter().cloned());
                node.transports.extend(plugin.endpoints.iter().map(|endpoint| endpoint.transport.clone()));
                if let Some(repo) = provider.split('#').next().filter(|value| value.contains('/')) {
                    node.repositories.insert(repo.to_string());
                }
            }
        }
    }

    pub fn search(&self, query: &CapabilityQuery) -> Vec<ProviderCandidate> {
        let keywords = query.keywords.iter().map(|v| v.to_ascii_lowercase()).collect::<Vec<_>>();
        let mut out = Vec::new();
        for repository in self.repositories.values() {
            if repository.archived && !query.include_archived { continue; }
            if let Some(required) = &query.require_transport {
                if !repository.transports.contains(required) { continue; }
            }

            let mut score = 0i64;
            let mut matched_capabilities = BTreeSet::new();
            let mut matched_tags = BTreeSet::new();

            if let Some(capability) = &query.capability {
                if repository.capabilities.contains(capability) {
                    score += 100;
                    matched_capabilities.insert(capability.clone());
                } else {
                    let prefix = capability.split('.').next().unwrap_or(capability);
                    for existing in &repository.capabilities {
                        if existing.starts_with(prefix) {
                            score += 20;
                            matched_capabilities.insert(existing.clone());
                        }
                    }
                }
            }

            let searchable = format!(
                "{} {} {} {}",
                repository.name,
                repository.tags.iter().cloned().collect::<Vec<_>>().join(" "),
                repository.capabilities.iter().cloned().collect::<Vec<_>>().join(" "),
                repository.evidence.iter().cloned().collect::<Vec<_>>().join(" ")
            ).to_ascii_lowercase();

            for keyword in &keywords {
                if searchable.contains(keyword) {
                    score += 10;
                    if repository.tags.iter().any(|tag| tag.eq_ignore_ascii_case(keyword)) {
                        matched_tags.insert(keyword.clone());
                        score += 5;
                    }
                }
            }

            if !repository.plugin_ids.is_empty() { score += 15; }
            if repository.transports.contains("mcp") || repository.transports.contains("mcp_stdio") { score += 5; }
            if score > 0 {
                out.push(ProviderCandidate {
                    repository: repository.full_name.clone(),
                    score,
                    matched_capabilities,
                    matched_tags,
                    transports: repository.transports.clone(),
                });
            }
        }
        out.sort_by(|a,b| b.score.cmp(&a.score).then_with(|| a.repository.cmp(&b.repository)));
        out
    }

    pub fn resolve_capability(&self, capability: &str) -> Vec<&CapabilityNode> {
        self.capabilities.values().filter(|node| {
            node.name == capability || node.name.starts_with(&format!("{}.", capability.trim_end_matches('.')))
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(name: &str, caps: &[&str], tags: &[&str]) -> RepositoryDescriptor {
        RepositoryDescriptor {
            name: name.into(),
            full_name: format!("Shtenco/{name}"),
            default_branch: "main".into(),
            visibility: "private".into(),
            archived: false,
            tags: tags.iter().map(|v|v.to_string()).collect(),
            capabilities: caps.iter().map(|v|v.to_string()).collect(),
            transports: ["mcp".to_string()].into_iter().collect(),
            plugin_ids: BTreeSet::new(),
            evidence: BTreeSet::new(),
        }
    }

    #[test]
    fn selects_exact_capability_before_keyword_only_match() {
        let mut graph=CapabilityGraph::default();
        graph.add_repository(repo("sales", &["business.sales.qualify"], &["sales"])).unwrap();
        graph.add_repository(repo("content", &["business.smm.plan"], &["sales","content"])).unwrap();
        let hits=graph.search(&CapabilityQuery{
            capability:Some("business.sales.qualify".into()),
            keywords:vec!["sales".into()],
            require_transport:None,
            include_archived:false,
        });
        assert_eq!(hits[0].repository,"Shtenco/sales");
        assert!(hits[0].score>hits[1].score);
    }
}
