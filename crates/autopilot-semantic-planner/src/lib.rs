use std::collections::{BTreeMap, BTreeSet};

use autopilot_capability_graph::CapabilityGraph;
use autopilot_domain::RiskClass;
use autopilot_plugins::PluginRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use thiserror::Error;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MappingContext {
    pub goal: String,
    #[serde(default)]
    pub hints: BTreeMap<String, Value>,
    #[serde(default)]
    pub memory: Value,
    #[serde(default)]
    pub prior_outputs: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingEvidence {
    pub field: String,
    pub source: String,
    pub confidence: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MappingResult {
    pub arguments: Value,
    pub unresolved_required: BTreeSet<String>,
    pub validation_errors: Vec<String>,
    pub evidence: Vec<MappingEvidence>,
}

impl MappingResult {
    pub fn valid(&self) -> bool {
        self.unresolved_required.is_empty() && self.validation_errors.is_empty()
    }
}

#[derive(Debug, Error)]
pub enum SemanticError {
    #[error("input contract must be a JSON object")]
    ContractNotObject,
    #[error("workflow has no executable steps")]
    EmptyWorkflow,
}

pub struct JsonContractValidator;

impl JsonContractValidator {
    pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
        let mut errors = Vec::new();
        validate_at(schema, value, "$", &mut errors);
        errors
    }
}

fn validate_at(schema: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(object) = schema.as_object() else { return };
    if let Some(constant) = object.get("const") {
        if value != constant {
            errors.push(format!("{path}: value does not match const"));
            return;
        }
    }
    if let Some(options) = object.get("enum").and_then(Value::as_array) {
        if !options.iter().any(|candidate| candidate == value) {
            errors.push(format!("{path}: value is not in enum"));
        }
    }
    if let Some(kind) = object.get("type").and_then(Value::as_str) {
        let matches = match kind {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        if !matches {
            errors.push(format!("{path}: expected {kind}"));
            return;
        }
    }
    if let (Some(properties), Some(instance)) = (
        object.get("properties").and_then(Value::as_object),
        value.as_object(),
    ) {
        let required = object.get("required").and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<BTreeSet<_>>())
            .unwrap_or_default();
        for field in required {
            if !instance.contains_key(field) {
                errors.push(format!("{path}.{field}: required field is missing"));
            }
        }
        for (field, child) in instance {
            if let Some(child_schema) = properties.get(field) {
                validate_at(child_schema, child, &format!("{path}.{field}"), errors);
            } else if object.get("additionalProperties").and_then(Value::as_bool) == Some(false) {
                errors.push(format!("{path}.{field}: additional property is not allowed"));
            }
        }
    }
    if let (Some(items_schema), Some(items)) = (object.get("items"), value.as_array()) {
        for (index, item) in items.iter().enumerate() {
            validate_at(items_schema, item, &format!("{path}[{index}]"), errors);
        }
    }
    if let Some(text) = value.as_str() {
        if let Some(min) = object.get("minLength").and_then(Value::as_u64) {
            if text.chars().count() < min as usize {
                errors.push(format!("{path}: string shorter than minLength={min}"));
            }
        }
        if let Some(max) = object.get("maxLength").and_then(Value::as_u64) {
            if text.chars().count() > max as usize {
                errors.push(format!("{path}: string longer than maxLength={max}"));
            }
        }
        if let Some(format) = object.get("format").and_then(Value::as_str) {
            match format {
                "email" if !looks_like_email(text) => errors.push(format!("{path}: invalid email")),
                "uri" | "url" if !looks_like_url(text) => errors.push(format!("{path}: invalid URL")),
                _ => {}
            }
        }
    }
}

pub struct SemanticArgumentMapper;

impl SemanticArgumentMapper {
    pub fn map(schema: &Value, context: &MappingContext) -> Result<MappingResult, SemanticError> {
        let schema_object = schema.as_object().ok_or(SemanticError::ContractNotObject)?;
        let properties = schema_object.get("properties").and_then(Value::as_object)
            .cloned().unwrap_or_default();
        let required = schema_object.get("required").and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).map(str::to_string).collect::<BTreeSet<_>>())
            .unwrap_or_default();

        let mut arguments = Map::new();
        let mut evidence = Vec::new();

        for (field, field_schema) in &properties {
            if let Some(value) = map_field(field, field_schema, context, &mut evidence) {
                arguments.insert(field.clone(), value);
            }
        }

        let unresolved_required = required.into_iter()
            .filter(|field| !arguments.contains_key(field))
            .collect::<BTreeSet<_>>();
        let value = Value::Object(arguments);
        let validation_errors = JsonContractValidator::validate(schema, &value);

        Ok(MappingResult {
            arguments: value,
            unresolved_required,
            validation_errors,
            evidence,
        })
    }
}

fn map_field(
    field: &str,
    schema: &Value,
    context: &MappingContext,
    evidence: &mut Vec<MappingEvidence>,
) -> Option<Value> {
    if let Some(value) = context.hints.get(field) {
        evidence.push(MappingEvidence { field: field.into(), source: "hint".into(), confidence: 100 });
        return Some(value.clone());
    }
    if let Some(value) = find_key_recursive(&context.memory, field) {
        evidence.push(MappingEvidence { field: field.into(), source: "memory".into(), confidence: 92 });
        return Some(value);
    }
    for (index, output) in context.prior_outputs.iter().enumerate().rev() {
        if let Some(value) = find_key_recursive(output, field) {
            evidence.push(MappingEvidence { field: field.into(), source: format!("steps.{index}"), confidence: 95 });
            return Some(value);
        }
    }
    if let Some(value) = schema.get("const") {
        evidence.push(MappingEvidence { field: field.into(), source: "schema.const".into(), confidence: 100 });
        return Some(value.clone());
    }
    if let Some(value) = schema.get("default") {
        evidence.push(MappingEvidence { field: field.into(), source: "schema.default".into(), confidence: 100 });
        return Some(value.clone());
    }

    let kind = schema.get("type").and_then(Value::as_str).unwrap_or("string");
    let normalized = normalize_field(field);
    let goal = context.goal.trim();

    if kind == "object" {
        if let Ok(result) = Self::map(schema, context) {
            if !result.arguments.as_object().is_some_and(|v| v.is_empty()) {
                evidence.extend(result.evidence.into_iter().map(|mut item| {
                    item.field = format!("{field}.{}", item.field);
                    item
                }));
                return Some(result.arguments);
            }
        }
        return None;
    }

    if kind == "array" {
        if let Some(value) = extract_array_from_goal(&normalized, goal) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.array".into(), confidence: 70 });
            return Some(value);
        }
        return None;
    }

    if matches_alias(&normalized, &["email","to","recipient","адрес","почта","получатель"]) {
        if let Some(email) = extract_email(goal) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.email".into(), confidence: 98 });
            return Some(Value::String(email));
        }
    }
    if matches_alias(&normalized, &["url","uri","link","website","site","ссылка","сайт"]) {
        if let Some(url) = extract_url(goal) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.url".into(), confidence: 98 });
            return Some(Value::String(url));
        }
    }
    if matches_alias(&normalized, &["query","search","q","поиск","запрос"]) {
        evidence.push(MappingEvidence { field: field.into(), source: "goal.full".into(), confidence: 80 });
        return Some(Value::String(goal.to_string()));
    }
    if matches_alias(&normalized, &["subject","title","name","тема","название","имя"]) {
        if let Some(quoted) = extract_quoted(goal) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.quoted".into(), confidence: 88 });
            return Some(Value::String(quoted));
        }
    }
    if matches_alias(&normalized, &["body","message","text","content","текст","сообщение","контент"]) {
        if let Some(after) = extract_after_alias(goal, &["текст","сообщение","message","body","content"]) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.after_alias".into(), confidence: 82 });
            return Some(Value::String(after));
        }
    }
    if matches_alias(&normalized, &["id","lead_id","client_id","user_id","campaign_id","ид","номер"]) {
        if let Some(id) = extract_id(goal, field) {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.id".into(), confidence: 85 });
            return Some(Value::String(id));
        }
    }

    match kind {
        "integer" => extract_first_integer(goal).map(|number| {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.integer".into(), confidence: 60 });
            Value::Number(Number::from(number))
        }),
        "number" => extract_first_number(goal).and_then(Number::from_f64).map(|number| {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.number".into(), confidence: 60 });
            Value::Number(number)
        }),
        "boolean" => extract_boolean(goal).map(|value| {
            evidence.push(MappingEvidence { field: field.into(), source: "goal.boolean".into(), confidence: 65 });
            Value::Bool(value)
        }),
        "string" => {
            if let Some(values) = schema.get("enum").and_then(Value::as_array) {
                for value in values {
                    let Some(text) = value.as_str() else { continue };
                    if goal.to_ascii_lowercase().contains(&text.to_ascii_lowercase()) {
                        evidence.push(MappingEvidence { field: field.into(), source: "goal.enum".into(), confidence: 90 });
                        return Some(Value::String(text.into()));
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn normalize_field(value: &str) -> String {
    value.to_ascii_lowercase().replace('-', "_")
}

fn matches_alias(field: &str, aliases: &[&str]) -> bool {
    aliases.iter().any(|alias| field == *alias || field.ends_with(&format!("_{alias}")))
}

fn find_key_recursive(value: &Value, key: &str) -> Option<Value> {
    match value {
        Value::Object(map) => {
            if let Some(value) = map.get(key) { return Some(value.clone()); }
            map.values().find_map(|value| find_key_recursive(value, key))
        }
        Value::Array(items) => items.iter().find_map(|value| find_key_recursive(value, key)),
        _ => None,
    }
}

fn extract_email(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|c: char| ",.;:()[]{}<>"'".contains(c)))
        .find(|token| looks_like_email(token))
        .map(str::to_string)
}

fn looks_like_email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else { return false };
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|token| token.trim_matches(|c: char| ",.;:()[]{}<>"'".contains(c)))
        .find(|token| looks_like_url(token))
        .map(str::to_string)
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("https://") || value.starts_with("http://")
}

fn extract_quoted(text: &str) -> Option<String> {
    for (open, close) in [('"','"'), ('«','»'), ('“','”')] {
        let start = text.find(open)?;
        let rest = &text[start + open.len_utf8()..];
        if let Some(end) = rest.find(close) {
            let value = rest[..end].trim();
            if !value.is_empty() { return Some(value.to_string()); }
        }
    }
    None
}

fn extract_after_alias(text: &str, aliases: &[&str]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    for alias in aliases {
        if let Some(index) = lower.find(alias) {
            let rest = text[index + alias.len()..]
                .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '—' | '='))
                .trim();
            if !rest.is_empty() { return Some(rest.to_string()); }
        }
    }
    None
}

fn extract_id(text: &str, field: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    for alias in [field, "id", "ид", "номер"] {
        if let Some(index) = lower.find(&alias.to_ascii_lowercase()) {
            let rest = text[index + alias.len()..].trim_start_matches(|c: char| {
                c.is_whitespace() || matches!(c, ':' | '=' | '#' | '-')
            });
            let token = rest.split_whitespace().next()
                .map(|v| v.trim_matches(|c: char| ",.;()[]{}"'".contains(c)))
                .unwrap_or("");
            if !token.is_empty() { return Some(token.to_string()); }
        }
    }
    None
}

fn extract_first_integer(text: &str) -> Option<i64> {
    text.split(|c: char| !c.is_ascii_digit() && c != '-')
        .find_map(|part| if part.is_empty() || part == "-" { None } else { part.parse().ok() })
}

fn extract_first_number(text: &str) -> Option<f64> {
    text.replace(',', ".")
        .split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .find_map(|part| if part.is_empty() || matches!(part, "-" | ".") { None } else { part.parse().ok() })
}

fn extract_boolean(text: &str) -> Option<bool> {
    let lower = text.to_ascii_lowercase();
    if [" true"," yes"," да"," включ"].iter().any(|v| lower.contains(v)) { Some(true) }
    else if [" false"," no"," нет"," выключ"].iter().any(|v| lower.contains(v)) { Some(false) }
    else { None }
}

fn extract_array_from_goal(_field: &str, goal: &str) -> Option<Value> {
    let quoted = extract_quoted(goal)?;
    let values = quoted.split(',').map(str::trim).filter(|v| !v.is_empty())
        .map(|v| Value::String(v.to_string())).collect::<Vec<_>>();
    if values.len() > 1 { Some(Value::Array(values)) } else { None }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub objective: String,
    pub plugin_id: String,
    pub capability: String,
    pub risk: RiskClass,
    pub input_contract: Value,
    pub mapped_arguments: Value,
    pub unresolved_required: BTreeSet<String>,
    pub validation_errors: Vec<String>,
    pub depends_on: Vec<String>,
    pub requires_approval: bool,
}

impl WorkflowStep {
    pub fn ready(&self) -> bool {
        self.unresolved_required.is_empty() && self.validation_errors.is_empty()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkflowPlan {
    pub goal: String,
    pub steps: Vec<WorkflowStep>,
    pub ready: bool,
}

pub struct AdaptiveWorkflowSynthesizer;

impl AdaptiveWorkflowSynthesizer {
    pub fn synthesize(
        goal: &str,
        graph: &CapabilityGraph,
        registry: &PluginRegistry,
        context: &MappingContext,
    ) -> Result<WorkflowPlan, SemanticError> {
        let clauses = split_goal(goal);
        let mut steps = Vec::new();

        for (index, clause) in clauses.iter().enumerate() {
            let Some(route) = graph.route_goal(clause, 1).into_iter().next() else { continue };
            let Some(manifest) = registry.get(&route.provider) else { continue };
            let Some(capability) = manifest.capability(&route.capability) else { continue };
            let contract = manifest.input_contract(&route.capability)
                .cloned().unwrap_or_else(|| serde_json::json!({
                    "type":"object","properties":{},"additionalProperties":true
                }));
            let mut local_context = context.clone();
            local_context.goal = clause.clone();
            let mapped = SemanticArgumentMapper::map(&contract, &local_context)?;
            steps.push(WorkflowStep {
                id: format!("step-{}", index + 1),
                objective: clause.clone(),
                plugin_id: manifest.id.clone(),
                capability: route.capability,
                risk: capability.risk,
                input_contract: contract,
                mapped_arguments: mapped.arguments,
                unresolved_required: mapped.unresolved_required,
                validation_errors: mapped.validation_errors,
                depends_on: if index == 0 { Vec::new() } else { vec![format!("step-{index}")] },
                requires_approval: capability.requires_approval,
            });
        }

        if steps.is_empty() { return Err(SemanticError::EmptyWorkflow); }
        let ready = steps.iter().all(WorkflowStep::ready);
        Ok(WorkflowPlan { goal: goal.into(), steps, ready })
    }
}

fn split_goal(goal: &str) -> Vec<String> {
    let normalized = goal
        .replace("→", "
")
        .replace("->", "
")
        .replace(" затем ", "
")
        .replace(" потом ", "
")
        .replace(" and then ", "
")
        .replace(';', "
");
    normalized.lines().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use autopilot_plugins::{PluginCapability, PluginKind, PluginManifest, PLUGIN_SCHEMA_V1};

    #[test]
    fn maps_email_and_message_from_goal() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{
                "to":{"type":"string","format":"email"},
                "body":{"type":"string"}
            },
            "required":["to","body"],
            "additionalProperties":false
        });
        let context = MappingContext {
            goal: "отправь на client@example.com сообщение: Коммерческое предложение готово".into(),
            ..Default::default()
        };
        let mapped = SemanticArgumentMapper::map(&schema, &context).unwrap();
        assert_eq!(mapped.arguments["to"], "client@example.com");
        assert!(mapped.arguments["body"].as_str().unwrap().contains("Коммерческое"));
        assert!(mapped.valid());
    }

    #[test]
    fn missing_required_argument_fails_dry_run() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{"id":{"type":"string"}},
            "required":["id"],
            "additionalProperties":false
        });
        let mapped = SemanticArgumentMapper::map(
            &schema,
            &MappingContext { goal:"найди клиента Иванова".into(), ..Default::default() },
        ).unwrap();
        assert!(mapped.unresolved_required.contains("id"));
        assert!(!mapped.valid());
    }

    #[test]
    fn prior_output_has_priority_for_exact_field() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{"client_id":{"type":"string"}},
            "required":["client_id"]
        });
        let mapped = SemanticArgumentMapper::map(
            &schema,
            &MappingContext {
                goal:"подготовь предложение".into(),
                prior_outputs: vec![serde_json::json!({"client_id":"abc-42"})],
                ..Default::default()
            },
        ).unwrap();
        assert_eq!(mapped.arguments["client_id"], "abc-42");
        assert!(mapped.valid());
    }

    #[test]
    fn validates_types_and_additional_properties() {
        let schema = serde_json::json!({
            "type":"object",
            "properties":{"limit":{"type":"integer"}},
            "required":["limit"],
            "additionalProperties":false
        });
        let errors = JsonContractValidator::validate(&schema, &serde_json::json!({"limit":"10","extra":1}));
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn synthesizes_linear_workflow_from_goal_clauses() {
        let mut registry = PluginRegistry::default();
        for (id, capability) in [("sales","business.sales.qualify"),("email","email.send")] {
            let manifest = PluginManifest {
                schema: PLUGIN_SCHEMA_V1.into(),
                id: id.into(),
                name: id.into(),
                version:"1.0.0".into(),
                kind:PluginKind::Business,
                provider:"test".into(),
                capabilities:vec![PluginCapability {
                    name:capability.into(),
                    risk:RiskClass::Low,
                    requires_approval:false,
                    mapped_core_capabilities:BTreeSet::new(),
                }],
                endpoints:vec![],
                tags:[if id=="sales" {"sales"} else {"email"}.to_string()].into_iter().collect(),
                requires:BTreeSet::new(),
                enabled_by_default:false,
                adapter:None,
                input_contracts:BTreeMap::from([(
                    capability.into(),
                    serde_json::json!({"type":"object","properties":{},"additionalProperties":true})
                )]),
            };
            registry.register(manifest).unwrap();
        }
        let mut graph=CapabilityGraph::default();
        graph.attach_plugins(&registry);
        let plan=AdaptiveWorkflowSynthesizer::synthesize(
            "найди клиента → отправь email",
            &graph,
            &registry,
            &MappingContext::default(),
        ).unwrap();
        assert_eq!(plan.steps.len(),2);
        assert_eq!(plan.steps[1].depends_on,vec!["step-1"]);
    }
}
