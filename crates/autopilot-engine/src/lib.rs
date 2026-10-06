use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use autopilot_audit::AuditStore;
use autopilot_capability_graph::CapabilityGraph;
use autopilot_core::{
    AgentExecutor, AgentOutcome, Director, DirectorConfig, DirectorError, GoalRequest, GoalRunSummary,
    PlannedTask, Planner, VerificationOutcome, Verifier,
};
use autopilot_domain::{AgentRole, AuditEvent, Capability, CorrelationId, Evidence, GoalState, RiskClass};
use autopilot_fleet::{FleetScheduler, FleetTask};
use autopilot_harness::{AgentHarness, BackendKind, BackendRegistry, RunSpec, Termination};
use autopilot_policy::{PolicyDecision, PolicyEngine};
use autopilot_plugin_runtime::{PluginCall, PluginExecutor};
use autopilot_outcomes::{OutcomeRecord, OutcomeStore};
use autopilot_router::{ModelCandidate, ModelRouter, RoutingProfile};
use autopilot_runtime::{CommandSpec, ExecutionProfile, RuntimeAdapter};
use autopilot_semantic_planner::{JsonContractValidator, MappingContext, SemanticArgumentMapper};
use autopilot_transform::VerifiedTransform;
use autopilot_verification::{VerificationCheck, VerificationPipeline};
use autopilot_workspace::WorkspaceManager;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginGoalStep {
    #[serde(default)]
    pub id: Option<String>,
    pub role: AgentRole,
    pub plugin_id: String,
    pub capability: String,
    pub input: Value,
    pub approved: bool,
    pub risk: RiskClass,
}

impl PluginGoalStep {
    pub fn new(
        role: AgentRole,
        plugin_id: impl Into<String>,
        capability: impl Into<String>,
        input: Value,
        risk: RiskClass,
    ) -> Self {
        Self {
            id: None,
            role,
            plugin_id: plugin_id.into(),
            capability: capability.into(),
            input,
            approved: false,
            risk,
        }
    }

    pub fn approved(mut self, approved: bool) -> Self {
        self.approved = approved;
        self
    }
}

struct PluginPlan {
    steps: Vec<PluginGoalStep>,
}

#[async_trait]
impl Planner for PluginPlan {
    async fn plan(&self, _request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        self.steps.iter().enumerate().map(|(step_index, step)| {
            let envelope = json!({
                "step_index": step_index,
                "step_id": step.id,
                "plugin_id": step.plugin_id,
                "capability": step.capability,
                "input": step.input,
                "approved": step.approved,
            });
            Ok(PlannedTask {
                role: step.role.clone(),
                objective: serde_json::to_string(&envelope).map_err(|error| error.to_string())?,
                capability: Capability::Custom(format!("plugin:{}/{}", step.plugin_id, step.capability)),
                risk: step.risk,
            })
        }).collect()
    }
}

#[derive(Clone, Debug)]
struct PluginStepContext {
    id: Option<String>,
    value: Value,
}

struct PluginExecutorBridge {
    executor: Arc<PluginExecutor>,
    context: Arc<Mutex<Vec<PluginStepContext>>>,
}

fn resolve_plugin_refs(value: Value, context: &[PluginStepContext]) -> Result<Value, String> {
    match value {
        Value::Object(mut map) if map.len() == 1 && map.contains_key("$ref") => {
            let reference = map.remove("$ref").and_then(|v| v.as_str().map(str::to_string))
                .ok_or_else(|| "$ref must be a string".to_string())?;
            let mut parts = reference.split('.');
            if parts.next() != Some("steps") {
                return Err(format!("unsupported reference root: {reference}"));
            }
            let index: usize = parts.next().ok_or_else(|| format!("missing step index in {reference}"))?
                .parse().map_err(|_| format!("invalid step index in {reference}"))?;
            let mut current = context.get(index).map(|entry| entry.value.clone())
                .ok_or_else(|| format!("reference points to unavailable step {index}"))?;
            for segment in parts {
                current = match current {
                    Value::Object(ref object) => object.get(segment).cloned()
                        .ok_or_else(|| format!("missing object field {segment} in {reference}"))?,
                    Value::Array(ref array) => {
                        let array_index: usize = segment.parse()
                            .map_err(|_| format!("array segment {segment} is not an index in {reference}"))?;
                        array.get(array_index).cloned()
                            .ok_or_else(|| format!("array index {array_index} out of range in {reference}"))?
                    }
                    _ => return Err(format!("cannot descend through {segment} in {reference}")),
                };
            }
            Ok(current)
        }
        Value::Object(map) => {
            let mut resolved = serde_json::Map::new();
            for (key, item) in map {
                resolved.insert(key, resolve_plugin_refs(item, context)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => items.into_iter()
            .map(|item| resolve_plugin_refs(item, context))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        other => Ok(other),
    }
}

fn resolve_semantic_plugin_input(value: Value, context: &[PluginStepContext]) -> Result<Value, String> {
    let Value::Object(mut map) = value else { return Ok(value) };
    let Some(contract) = map.remove("$semantic_contract") else {
        return Ok(Value::Object(map));
    };
    let goal = map.remove("$semantic_goal").and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();
    let seed = map.remove("$semantic_hints")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    let bindings = map.remove("$typed_bindings")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let control = map.into_iter()
        .filter(|(key, _)| key.starts_with('_'))
        .collect::<serde_json::Map<_,_>>();

    let mut hints = seed.into_iter().collect::<std::collections::BTreeMap<_,_>>();
    for binding in bindings {
        let from_step = binding.get("from_step").and_then(Value::as_str)
            .ok_or_else(|| "typed binding missing from_step".to_string())?;
        let from_path = binding.get("from_path").and_then(Value::as_str)
            .ok_or_else(|| "typed binding missing from_path".to_string())?;
        let to_field = binding.get("to_field").and_then(Value::as_str)
            .ok_or_else(|| "typed binding missing to_field".to_string())?;
        let source = context.iter()
            .find(|entry| entry.id.as_deref() == Some(from_step))
            .and_then(|entry| entry.value.get("output"))
            .ok_or_else(|| format!("typed binding source output is unavailable: {from_step}"))?;
        let current = if let Some(transform_value) = binding.get("transform").filter(|value| !value.is_null()) {
            let transform: VerifiedTransform = serde_json::from_value(transform_value.clone())
                .map_err(|error| format!("invalid verified transform in runtime binding: {error}"))?;
            transform.apply(source.clone())
                .map_err(|error| format!("verified transform runtime failure from {from_step}: {error}"))?
        } else {
            let mut current = source;
            if !from_path.is_empty() {
                for segment in from_path.split('.') {
                    current = current.get(segment)
                        .ok_or_else(|| format!("typed binding path not found: {from_step}.{from_path}"))?;
                }
            }
            current.clone()
        };
        let pointer = format!(
            "/properties/{}",
            to_field.replace('~', "~0").replace('/', "~1")
        );
        let target_schema = contract.pointer(&pointer)
            .ok_or_else(|| format!("typed binding target is not declared: {to_field}"))?;
        let errors = JsonContractValidator::validate(target_schema, &current);
        if !errors.is_empty() {
            return Err(format!(
                "typed binding runtime validation failed for {from_step}.{from_path} -> {to_field}: {}",
                errors.join("; ")
            ));
        }
        hints.insert(to_field.to_string(), current);
    }

    let mapping = SemanticArgumentMapper::map(
        &contract,
        &MappingContext {
            goal,
            hints,
            memory: Value::Null,
            prior_outputs: context.iter().map(|entry| entry.value.clone()).collect(),
        },
    ).map_err(|error| error.to_string())?;
    if !mapping.valid() {
        return Err(format!(
            "semantic input dry-run failed: unresolved={:?} validation={:?}",
            mapping.unresolved_required,
            mapping.validation_errors
        ));
    }
    let mut arguments = mapping.arguments.as_object().cloned()
        .ok_or_else(|| "semantic mapper did not return an object".to_string())?;
    for (key, value) in control {
        arguments.insert(key, value);
    }
    Ok(Value::Object(arguments))
}

#[async_trait]
impl AgentExecutor for PluginExecutorBridge {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        let value: Value = serde_json::from_str(&task.objective).map_err(|error| error.to_string())?;
        let input = {
            let context = self.context.lock().map_err(|_| "plugin context lock poisoned".to_string())?;
            let resolved = resolve_plugin_refs(value.get("input").cloned().unwrap_or(Value::Null), &context)?;
            resolve_semantic_plugin_input(resolved, &context)?
        };
        let call = PluginCall {
            plugin_id: value.get("plugin_id").and_then(Value::as_str).ok_or("plugin_id missing")?.to_string(),
            capability: value.get("capability").and_then(Value::as_str).ok_or("capability missing")?.to_string(),
            input,
            approved: value.get("approved").and_then(Value::as_bool).unwrap_or(false),
        };
        let step_id = value.get("step_id").and_then(Value::as_str).map(str::to_string);
        let result = self.executor.execute(call).await.map_err(|error| error.to_string())?;
        let serialized = serde_json::to_value(&result).map_err(|error| error.to_string())?;
        self.context.lock().map_err(|_| "plugin context lock poisoned".to_string())?
            .push(PluginStepContext { id: step_id, value: serialized });
        Ok(AgentOutcome {
            objective: task.objective.clone(),
            success: true,
            summary: serde_json::to_string(&result).map_err(|error| error.to_string())?,
        })
    }
}

struct PluginEvidenceVerifier;

#[async_trait]
impl Verifier for PluginEvidenceVerifier {
    async fn verify(&self, _request: &GoalRequest, outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String> {
        let passed = !outcomes.is_empty() && outcomes.iter().all(|outcome| outcome.success);
        let evidence = outcomes.iter().enumerate().map(|(index, outcome)| {
            if outcome.success {
                Evidence::passed("plugin-execution", format!("step {} completed with structured plugin result", index + 1))
            } else {
                Evidence::failed("plugin-execution", format!("step {} failed", index + 1))
            }
        }).collect();
        Ok(VerificationOutcome { passed, evidence })
    }
}

pub struct PluginGoalEngine {
    director: Director,
}

impl PluginGoalEngine {
    pub fn new(
        audit: Arc<dyn AuditStore>,
        executor: Arc<PluginExecutor>,
        steps: Vec<PluginGoalStep>,
        max_repairs: usize,
    ) -> Self {
        let required_capabilities = steps.iter()
            .map(|step| (Capability::Custom(format!("plugin:{}/{}", step.plugin_id, step.capability)), step.risk))
            .collect::<Vec<_>>();
        let planner = Arc::new(PluginPlan { steps });
        let bridge = Arc::new(PluginExecutorBridge { executor, context: Arc::new(Mutex::new(Vec::new())) });
        let verifier = Arc::new(PluginEvidenceVerifier);
        let mut rules = Vec::new();
        for (capability, risk) in required_capabilities {
            rules.push(autopilot_policy::PolicyRule::for_capability_at_least_risk(
                capability, risk, PolicyDecision::Allow,
            ));
        }
        let director = Director::new(
            DirectorConfig { max_repairs },
            PolicyEngine::new(rules),
            audit,
            planner,
            bridge,
            verifier,
        );
        Self { director }
    }

    pub async fn run(&self, goal: impl Into<String>) -> Result<GoalRunSummary, DirectorError> {
        self.director.run_goal(GoalRequest::new(goal)).await
    }
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelStepOutcome {
    pub step_id: String,
    pub plugin_id: String,
    pub capability: String,
    pub success: bool,
    pub latency_ms: u64,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParallelWorkflowSummary {
    pub success: bool,
    pub layers_executed: usize,
    pub outcomes: Vec<ParallelStepOutcome>,
}

pub struct ParallelPluginWorkflowEngine {
    executor: Arc<PluginExecutor>,
    audit: Arc<dyn AuditStore>,
    outcomes: Option<OutcomeStore>,
}

impl ParallelPluginWorkflowEngine {
    pub fn new(
        executor: Arc<PluginExecutor>,
        audit: Arc<dyn AuditStore>,
        outcomes: Option<OutcomeStore>,
    ) -> Self {
        Self { executor, audit, outcomes }
    }

    fn is_parallel_safe(&self, step: &PluginGoalStep) -> bool {
        self.executor.registry()
            .get(&step.plugin_id)
            .and_then(|manifest| manifest.capability(&step.capability))
            .is_some_and(|capability| capability.parallel_safe && !capability.requires_approval)
    }

    fn validate_layers(
        steps: &[PluginGoalStep],
        layers: &[Vec<String>],
    ) -> Result<(), String> {
        let ids = steps.iter().map(|step| {
            step.id.clone().ok_or_else(|| "parallel workflow requires stable step ids".to_string())
        }).collect::<Result<BTreeSet<_>,_>>()?;
        let layered = layers.iter().flatten().cloned().collect::<Vec<_>>();
        let layered_set = layered.iter().cloned().collect::<BTreeSet<_>>();
        if layered.len() != layered_set.len() {
            return Err("parallel workflow execution layers contain duplicate step ids".into());
        }
        if ids != layered_set {
            return Err(format!(
                "parallel workflow execution layers do not exactly cover workflow steps: steps={ids:?} layers={layered_set:?}"
            ));
        }
        Ok(())
    }

    async fn execute_one(
        executor: Arc<PluginExecutor>,
        step: PluginGoalStep,
        context: Vec<PluginStepContext>,
    ) -> ParallelStepOutcome {
        let step_id = step.id.clone().unwrap_or_else(|| "unknown".into());
        let started = Instant::now();
        let prepared = prepare_plugin_step_call(&step, &context);
        let result = match prepared {
            Ok(call) => executor.execute(call).await.map_err(|error| error.to_string()),
            Err(error) => Err(error),
        };
        let latency_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        match result {
            Ok(result) => ParallelStepOutcome {
                step_id,
                plugin_id: step.plugin_id,
                capability: step.capability,
                success: true,
                latency_ms,
                result: serde_json::to_value(result).ok(),
                error: None,
            },
            Err(error) => ParallelStepOutcome {
                step_id,
                plugin_id: step.plugin_id,
                capability: step.capability,
                success: false,
                latency_ms,
                result: None,
                error: Some(error),
            },
        }
    }

    async fn record_outcome(
        &self,
        goal_fingerprint: &str,
        outcome: &ParallelStepOutcome,
    ) -> Result<(), String> {
        let Some(store) = &self.outcomes else { return Ok(()) };
        let mut record = OutcomeRecord::new(
            goal_fingerprint,
            format!("{}/{}", outcome.plugin_id, outcome.capability),
        );
        record.success = outcome.success;
        record.verified = outcome.success;
        record.latency_ms = outcome.latency_ms;
        record.quality_score = if outcome.success { 1.0 } else { 0.0 };
        record.error_class = outcome.error.as_ref().map(|_| "plugin_execution".into());
        store.record(&record).map_err(|error| error.to_string())
    }

    pub async fn run(
        &self,
        goal_fingerprint: &str,
        steps: Vec<PluginGoalStep>,
        execution_layers: Vec<Vec<String>>,
    ) -> Result<ParallelWorkflowSummary, String> {
        Self::validate_layers(&steps, &execution_layers)?;
        let by_id = steps.into_iter().map(|step| {
            let id = step.id.clone().expect("validated step id");
            (id, step)
        }).collect::<std::collections::BTreeMap<_,_>>();

        let correlation = CorrelationId::new();
        let mut context = Vec::<PluginStepContext>::new();
        let mut all_outcomes = Vec::new();

        for (layer_index, layer) in execution_layers.iter().enumerate() {
            let snapshot = context.clone();
            let mut safe = Vec::new();
            let mut exclusive = Vec::new();
            for step_id in layer {
                let step = by_id.get(step_id)
                    .ok_or_else(|| format!("unknown workflow step in layer: {step_id}"))?
                    .clone();
                if self.is_parallel_safe(&step) { safe.push(step); }
                else { exclusive.push(step); }
            }

            self.audit.append(AuditEvent::new(
                correlation,
                "ParallelDAG",
                format!("layer-{layer_index}-start"),
                RiskClass::Low,
                format!("parallel_safe={} exclusive={}", safe.len(), exclusive.len()),
            )).await.map_err(|error| error.to_string())?;

            let mut layer_outcomes = std::collections::BTreeMap::<String, ParallelStepOutcome>::new();
            let mut jobs = JoinSet::new();
            for step in safe {
                let executor = self.executor.clone();
                let snapshot = snapshot.clone();
                jobs.spawn(async move {
                    Self::execute_one(executor, step, snapshot).await
                });
            }
            while let Some(joined) = jobs.join_next().await {
                let outcome = joined.map_err(|error| format!("parallel plugin task join failure: {error}"))?;
                layer_outcomes.insert(outcome.step_id.clone(), outcome);
            }

            for step in exclusive {
                let outcome = Self::execute_one(
                    self.executor.clone(),
                    step,
                    snapshot.clone(),
                ).await;
                layer_outcomes.insert(outcome.step_id.clone(), outcome);
            }

            for step_id in layer {
                let outcome = layer_outcomes.remove(step_id)
                    .ok_or_else(|| format!("workflow step produced no outcome: {step_id}"))?;
                self.record_outcome(goal_fingerprint, &outcome).await?;
                if !outcome.success {
                    self.audit.append(AuditEvent::new(
                        correlation,
                        format!("step:{step_id}"),
                        "failed",
                        RiskClass::Low,
                        outcome.error.clone().unwrap_or_else(|| "unknown".into()),
                    )).await.map_err(|error| error.to_string())?;
                    all_outcomes.push(outcome);
                    return Ok(ParallelWorkflowSummary {
                        success: false,
                        layers_executed: layer_index + 1,
                        outcomes: all_outcomes,
                    });
                }
                let value = outcome.result.clone()
                    .ok_or_else(|| format!("successful step has no serialized result: {step_id}"))?;
                context.push(PluginStepContext {
                    id: Some(step_id.clone()),
                    value,
                });
                all_outcomes.push(outcome);
            }

            self.audit.append(AuditEvent::new(
                correlation,
                "ParallelDAG",
                format!("layer-{layer_index}-finish"),
                RiskClass::Low,
                "passed",
            )).await.map_err(|error| error.to_string())?;
        }

        Ok(ParallelWorkflowSummary {
            success: true,
            layers_executed: execution_layers.len(),
            outcomes: all_outcomes,
        })
    }
}

fn prepare_plugin_step_call(
    step: &PluginGoalStep,
    context: &[PluginStepContext],
) -> Result<PluginCall, String> {
    let resolved = resolve_plugin_refs(step.input.clone(), context)?;
    let input = resolve_semantic_plugin_input(resolved, context)?;
    Ok(PluginCall {
        plugin_id: step.plugin_id.clone(),
        capability: step.capability.clone(),
        input,
        approved: step.approved,
    })
}

pub struct CapabilityGraphPlanner {
    graph: Arc<CapabilityGraph>,
    max_routes: usize,
}

impl CapabilityGraphPlanner {
    pub fn new(graph: Arc<CapabilityGraph>, max_routes: usize) -> Self {
        Self { graph, max_routes: max_routes.max(1) }
    }
}

#[async_trait]
impl Planner for CapabilityGraphPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        let routes = self.graph.route_goal(&request.objective, self.max_routes.saturating_mul(4))
            .into_iter()
            .filter(|route| {
                let name = route.capability.to_ascii_lowercase();
                !["send","create","update","publish","execute","submit","write","delete","click","fill","run_command"]
                    .iter().any(|needle| name.contains(needle))
            })
            .take(self.max_routes)
            .collect::<Vec<_>>();
        if routes.is_empty() {
            return Err("capability graph found no safe autonomous providers for goal; use an explicit approved plugin plan for write actions".into());
        }
        routes.into_iter().map(|route| {
            let envelope = json!({
                "plugin_id": route.provider,
                "capability": route.capability,
                "input": {
                    "goal": request.objective,
                    "_route": {
                        "repository": route.repository,
                        "score": route.score,
                        "matched_terms": route.matched_terms,
                    }
                },
                "approved": false,
            });
            Ok(PlannedTask {
                role: AgentRole::Custom("CapabilityRouter".into()),
                objective: serde_json::to_string(&envelope).map_err(|error| error.to_string())?,
                capability: Capability::Custom(format!(
                    "plugin:{}/{}",
                    envelope["plugin_id"].as_str().unwrap_or("unknown"),
                    envelope["capability"].as_str().unwrap_or("unknown"),
                )),
                risk: RiskClass::Low,
            })
        }).collect()
    }
}

pub struct GraphGoalEngine {
    director: Director,
}

impl GraphGoalEngine {
    pub fn new(
        graph: Arc<CapabilityGraph>,
        audit: Arc<dyn AuditStore>,
        executor: Arc<PluginExecutor>,
        max_routes: usize,
    ) -> Self {
        let planner = Arc::new(CapabilityGraphPlanner::new(graph, max_routes));
        let bridge = Arc::new(PluginExecutorBridge {
            executor,
            context: Arc::new(Mutex::new(Vec::new())),
        });
        let verifier = Arc::new(PluginEvidenceVerifier);
        let director = Director::new(
            DirectorConfig { max_repairs: 0 },
            PolicyEngine::default(),
            audit,
            planner,
            bridge,
            verifier,
        );
        Self { director }
    }

    pub async fn run(&self, goal: impl Into<String>) -> Result<GoalRunSummary, DirectorError> {
        self.director.run_goal(GoalRequest::new(goal)).await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

impl VerificationCommand {
    pub fn new<I, S>(name: impl Into<String>, program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self { name: name.into(), program: program.into(), args: args.into_iter().map(Into::into).collect() }
    }
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub backend: BackendKind,
    pub model: Option<String>,
    pub workdir: PathBuf,
    pub allowed_env: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub verification: Vec<VerificationCommand>,
    pub max_repairs: usize,
}

struct SingleTaskPlanner;

#[async_trait]
impl Planner for SingleTaskPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        Ok(vec![PlannedTask {
            role: AgentRole::Coder,
            objective: request.objective.clone(),
            capability: Capability::ExecuteProcess,
            risk: RiskClass::Medium,
        }])
    }
}

struct HarnessExecutor {
    harness: AgentHarness,
    config: EngineConfig,
}

#[async_trait]
impl AgentExecutor for HarnessExecutor {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        let mut spec = RunSpec::new(self.config.backend.clone(), task.objective.clone(), self.config.workdir.clone())
            .profile(ExecutionProfile::WorkspaceWrite);
        if let Some(model) = &self.config.model { spec = spec.model(model.clone()); }
        for key in &self.config.allowed_env { spec = spec.allow_env(key.clone()); }
        if let Some(timeout_ms) = self.config.timeout_ms { spec = spec.timeout_ms(timeout_ms); }

        let result = self.harness.run(spec).await.map_err(|error| error.to_string())?;
        let success = matches!(result.termination, Termination::Exited);
        let summary = if success {
            result.stdout
        } else if result.stderr.is_empty() {
            format!("agent terminated with {:?}", result.termination)
        } else {
            result.stderr
        };
        Ok(AgentOutcome { objective: task.objective.clone(), success, summary })
    }
}

struct PipelineVerifier {
    runtime: Arc<dyn RuntimeAdapter>,
    workdir: PathBuf,
    checks: Vec<VerificationCommand>,
}

#[async_trait]
impl Verifier for PipelineVerifier {
    async fn verify(&self, _request: &GoalRequest, outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String> {
        if outcomes.is_empty() || outcomes.iter().any(|outcome| !outcome.success) {
            return Ok(VerificationOutcome {
                passed: false,
                evidence: vec![Evidence::failed("agent-execution", "one or more agent executions failed")],
            });
        }
        if self.checks.is_empty() {
            return Ok(VerificationOutcome {
                passed: true,
                evidence: vec![Evidence::passed("agent-execution", "agent completed and no verification commands were configured")],
            });
        }
        let checks = self.checks.iter().map(|check| {
            VerificationCheck::new(
                check.name.clone(),
                CommandSpec::new(check.program.clone())
                    .args(check.args.clone())
                    .cwd(self.workdir.clone())
                    .profile(ExecutionProfile::WorkspaceWrite),
            )
        }).collect();
        let report = VerificationPipeline::new(self.runtime.clone(), false, checks)
            .run().await.map_err(|error| error.to_string())?;
        Ok(VerificationOutcome { passed: report.passed, evidence: report.evidence })
    }
}

pub struct AutonomousEngine { director: Director }

impl AutonomousEngine {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, audit: Arc<dyn AuditStore>, config: EngineConfig) -> Self {
        let executor = Arc::new(HarnessExecutor {
            harness: AgentHarness::new(runtime.clone(), BackendRegistry::default()),
            config: config.clone(),
        });
        let verifier = Arc::new(PipelineVerifier {
            runtime,
            workdir: config.workdir.clone(),
            checks: config.verification.clone(),
        });
        let director = Director::new(
            DirectorConfig { max_repairs: config.max_repairs },
            PolicyEngine::default(),
            audit,
            Arc::new(SingleTaskPlanner),
            executor,
            verifier,
        );
        Self { director }
    }

    pub async fn run(&self, objective: impl Into<String>) -> Result<GoalRunSummary, DirectorError> {
        self.director
            .run_goal(GoalRequest::new(objective).require(Capability::ExecuteProcess, RiskClass::Medium))
            .await
    }
}

#[derive(Clone, Debug)]
pub struct FleetEngineConfig {
    pub repo_root: PathBuf,
    pub worktree_root: PathBuf,
    pub base_ref: String,
    pub routing_profile: RoutingProfile,
    pub candidates: Vec<ModelCandidate>,
    pub allowed_env: Vec<String>,
    pub timeout_ms: Option<u64>,
    pub verification: Vec<VerificationCommand>,
    pub max_repairs: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleRun {
    pub role: AgentRole,
    pub provider: String,
    pub model: String,
    pub backend: BackendKind,
    pub workdir: PathBuf,
    pub success: bool,
    pub summary: String,
}

#[derive(Clone, Debug)]
pub struct FleetRunSummary {
    pub final_state: GoalState,
    pub repairs: usize,
    pub evidence: Vec<Evidence>,
    pub role_runs: Vec<RoleRun>,
    pub worktree: Option<PathBuf>,
    pub correlation_id: CorrelationId,
}

pub fn backend_for_provider(provider: &str) -> Result<BackendKind, String> {
    match provider.to_ascii_lowercase().as_str() {
        "opencode" => Ok(BackendKind::OpenCode),
        "openai" | "codex" => Ok(BackendKind::Codex),
        "anthropic" | "claude" | "claude-code" => Ok(BackendKind::Claude),
        "google" | "gemini" => Ok(BackendKind::Gemini),
        other => Err(format!("unsupported routed provider: {other}")),
    }
}

pub struct FleetEngine {
    writer_runtime: Arc<dyn RuntimeAdapter>,
    review_runtime: Arc<dyn RuntimeAdapter>,
    audit: Arc<dyn AuditStore>,
    policy: PolicyEngine,
    config: FleetEngineConfig,
}

impl FleetEngine {
    pub fn new(runtime: Arc<dyn RuntimeAdapter>, audit: Arc<dyn AuditStore>, config: FleetEngineConfig) -> Self {
        Self::with_role_runtimes(runtime.clone(), runtime, audit, config)
    }

    pub fn with_role_runtimes(
        writer_runtime: Arc<dyn RuntimeAdapter>,
        review_runtime: Arc<dyn RuntimeAdapter>,
        audit: Arc<dyn AuditStore>,
        config: FleetEngineConfig,
    ) -> Self {
        Self {
            writer_runtime,
            review_runtime,
            audit,
            policy: PolicyEngine::default(),
            config,
        }
    }

    async fn audit(&self, correlation: CorrelationId, actor: &str, event: &str, status: &str) -> Result<(), String> {
        self.audit.append(AuditEvent::new(correlation, actor, event, RiskClass::Low, status)).await
    }

    fn route(&self, role: &AgentRole) -> Result<(ModelCandidate, BackendKind), String> {
        let candidate = ModelRouter::select(role, self.config.routing_profile, &self.config.candidates)
            .ok_or_else(|| format!("no healthy model candidate for role {role:?}"))?;
        let backend = backend_for_provider(&candidate.provider)?;
        Ok((candidate, backend))
    }

    fn role_runtime(&self, role: &AgentRole) -> (Arc<dyn RuntimeAdapter>, ExecutionProfile) {
        match role {
            AgentRole::Architect | AgentRole::Tester | AgentRole::Reviewer | AgentRole::Security => {
                (self.review_runtime.clone(), ExecutionProfile::ReadOnlyWorkspace)
            }
            AgentRole::Coder | AgentRole::Debugger => {
                (self.writer_runtime.clone(), ExecutionProfile::WorkspaceWrite)
            }
            _ => (self.writer_runtime.clone(), ExecutionProfile::Host),
        }
    }

    async fn run_role(
        &self,
        role: AgentRole,
        prompt: String,
        workdir: PathBuf,
        correlation: CorrelationId,
    ) -> Result<RoleRun, String> {
        let (candidate, backend) = self.route(&role)?;
        let (runtime, profile) = self.role_runtime(&role);
        self.audit(correlation, &format!("{role:?}"), "agent-start", "running").await?;

        let mut spec = RunSpec::new(backend.clone(), prompt, workdir.clone())
            .model(candidate.model.clone())
            .profile(profile);
        for key in &self.config.allowed_env { spec = spec.allow_env(key.clone()); }
        if let Some(timeout_ms) = self.config.timeout_ms { spec = spec.timeout_ms(timeout_ms); }

        let result = AgentHarness::new(runtime, BackendRegistry::default())
            .run(spec).await.map_err(|error| error.to_string())?;
        let success = matches!(result.termination, Termination::Exited);
        let summary = if success {
            result.stdout
        } else if result.stderr.is_empty() {
            format!("agent terminated with {:?}", result.termination)
        } else {
            result.stderr
        };
        self.audit(
            correlation,
            &format!("{role:?}"),
            "agent-finish",
            if success { "passed" } else { "failed" },
        ).await?;

        Ok(RoleRun {
            role,
            provider: candidate.provider,
            model: candidate.model,
            backend,
            workdir,
            success,
            summary,
        })
    }

    async fn verify(&self, workdir: &Path) -> Result<VerificationOutcome, String> {
        if self.config.verification.is_empty() {
            return Ok(VerificationOutcome {
                passed: true,
                evidence: vec![Evidence::passed("verification", "no verification commands configured")],
            });
        }
        let checks = self.config.verification.iter().map(|check| {
            VerificationCheck::new(
                check.name.clone(),
                CommandSpec::new(check.program.clone())
                    .args(check.args.clone())
                    .cwd(workdir.to_path_buf())
                    .profile(ExecutionProfile::WorkspaceWrite),
            )
        }).collect();
        let report = VerificationPipeline::new(self.writer_runtime.clone(), false, checks)
            .run().await.map_err(|error| error.to_string())?;
        Ok(VerificationOutcome { passed: report.passed, evidence: report.evidence })
    }

    fn fleet_scheduler() -> Result<FleetScheduler, String> {
        FleetScheduler::new(vec![
            FleetTask { id: "architect".into(), role: AgentRole::Architect, dependencies: vec![], writes: false, workspace_id: "source".into() },
            FleetTask { id: "coder".into(), role: AgentRole::Coder, dependencies: vec!["architect".into()], writes: true, workspace_id: "change".into() },
            FleetTask { id: "tester".into(), role: AgentRole::Tester, dependencies: vec!["coder".into()], writes: false, workspace_id: "change".into() },
            FleetTask { id: "reviewer".into(), role: AgentRole::Reviewer, dependencies: vec!["coder".into()], writes: false, workspace_id: "change".into() },
            FleetTask { id: "security".into(), role: AgentRole::Security, dependencies: vec!["coder".into()], writes: false, workspace_id: "change".into() },
        ]).map_err(|error| error.to_string())
    }

    fn worktree_label(objective: &str) -> String {
        let mut hasher = DefaultHasher::new();
        objective.hash(&mut hasher);
        format!("goal-{:x}", hasher.finish())
    }

    pub async fn run(&self, objective: impl Into<String>) -> Result<FleetRunSummary, String> {
        let objective = objective.into();
        let correlation = CorrelationId::new();
        self.audit(correlation, "Director", "Planning", "entered").await?;

        for (capability, risk) in [
            (Capability::ReadWorkspace, RiskClass::Low),
            (Capability::WriteWorkspace, RiskClass::Medium),
            (Capability::ExecuteProcess, RiskClass::Medium),
        ] {
            if self.policy.evaluate(capability, risk) != PolicyDecision::Allow {
                self.audit(correlation, "Director", "PolicyCheck", "blocked").await?;
                return Ok(FleetRunSummary {
                    final_state: GoalState::Failed,
                    repairs: 0,
                    evidence: vec![Evidence::failed("policy", "required fleet capability was not allowed")],
                    role_runs: vec![],
                    worktree: None,
                    correlation_id: correlation,
                });
            }
        }

        let scheduler = Self::fleet_scheduler()?;
        let workspace = WorkspaceManager::new(
            self.config.repo_root.clone(),
            self.config.worktree_root.clone(),
            self.writer_runtime.clone(),
        );
        let mut completed = BTreeSet::new();
        let mut role_runs = Vec::new();

        let architect_task = scheduler.ready(&completed, &[])
            .into_iter().find(|task| task.role == AgentRole::Architect)
            .ok_or_else(|| "architect task was not ready".to_string())?;
        let architect = self.run_role(
            architect_task.role,
            format!("Act as the Architect. Analyze the repository for this goal without editing files. Goal: {objective}. Return a concrete implementation plan, affected files, interfaces, risks, and acceptance checks."),
            self.config.repo_root.clone(),
            correlation,
        ).await?;
        if !architect.success {
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("architect", "architecture agent failed")],
                role_runs: vec![architect],
                worktree: None,
                correlation_id: correlation,
            });
        }
        let architecture = architect.summary.clone();
        role_runs.push(architect);
        completed.insert("architect".to_string());

        let worktree = workspace.create_worktree(&Self::worktree_label(&objective), &self.config.base_ref)
            .await.map_err(|error| error.to_string())?;

        let coder_task = scheduler.ready(&completed, &[])
            .into_iter().find(|task| task.role == AgentRole::Coder)
            .ok_or_else(|| "coder task was not ready".to_string())?;
        let coder = self.run_role(
            coder_task.role,
            format!("Act as the Coder. Implement the goal in this isolated worktree. Goal: {objective}\nArchitect plan:\n{architecture}\nDo not publish or force-push. Keep changes scoped and leave the worktree ready for independent review."),
            worktree.path.clone(),
            correlation,
        ).await?;
        if !coder.success {
            role_runs.push(coder);
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("coder", "coding agent failed")],
                role_runs,
                worktree: Some(worktree.path),
                correlation_id: correlation,
            });
        }
        role_runs.push(coder);
        completed.insert("coder".to_string());

        let ready_reviews = scheduler.ready(&completed, &[]);
        let by_role: HashMap<AgentRole, FleetTask> = ready_reviews.into_iter()
            .map(|task| (task.role.clone(), task)).collect();
        for role in [AgentRole::Tester, AgentRole::Reviewer, AgentRole::Security] {
            if !by_role.contains_key(&role) { return Err(format!("{role:?} task was not ready after coder")); }
        }

        let tester_prompt = format!("Act as an independent Tester. Inspect the completed implementation for goal: {objective}. Run or inspect relevant tests, identify missing regression coverage, and do not modify files.");
        let reviewer_prompt = format!("Act as an independent Reviewer. Review the implementation for goal: {objective}. Focus on correctness, regressions, maintainability, concurrency, portability and requirement coverage. Do not modify files.");
        let security_prompt = format!("Act as an independent Security reviewer. Review the implementation for goal: {objective}. Check secret exposure, command injection, path traversal, unsafe process execution, dependency and supply-chain risk. Do not modify files.");
        let review_workdir = worktree.path.clone();
        let (tester, reviewer, security) = tokio::join!(
            self.run_role(AgentRole::Tester, tester_prompt, review_workdir.clone(), correlation),
            self.run_role(AgentRole::Reviewer, reviewer_prompt, review_workdir.clone(), correlation),
            self.run_role(AgentRole::Security, security_prompt, review_workdir.clone(), correlation),
        );
        let tester = tester?;
        let reviewer = reviewer?;
        let security = security?;
        let reviews_passed = tester.success && reviewer.success && security.success;
        role_runs.extend([tester, reviewer, security]);
        if !reviews_passed {
            return Ok(FleetRunSummary {
                final_state: GoalState::Failed,
                repairs: 0,
                evidence: vec![Evidence::failed("cross-review", "one or more independent review agents failed")],
                role_runs,
                worktree: Some(worktree.path),
                correlation_id: correlation,
            });
        }

        let mut verification = self.verify(&worktree.path).await?;
        let mut repairs = 0usize;
        let mut evidence = verification.evidence.clone();
        while !verification.passed && repairs < self.config.max_repairs {
            repairs += 1;
            let debugger = self.run_role(
                AgentRole::Debugger,
                format!("Act as the Debugger. Verification failed for goal: {objective}. Diagnose the failures in this worktree, make the minimum corrective edits, and leave the repository ready for another verification cycle. Previous evidence: {evidence:?}"),
                worktree.path.clone(),
                correlation,
            ).await?;
            let debugger_success = debugger.success;
            role_runs.push(debugger);
            if !debugger_success { break; }
            verification = self.verify(&worktree.path).await?;
            evidence.extend(verification.evidence.clone());
        }

        let final_state = if verification.passed { GoalState::Completed } else { GoalState::Failed };
        self.audit(
            correlation,
            "Director",
            &format!("{final_state:?}"),
            if final_state == GoalState::Completed { "passed" } else { "failed" },
        ).await?;

        Ok(FleetRunSummary {
            final_state,
            repairs,
            evidence,
            role_runs,
            worktree: Some(worktree.path),
            correlation_id: correlation,
        })
    }
}


#[cfg(test)]
mod plugin_dataflow_tests {
    use super::*;

    #[test]
    fn resolves_nested_step_references() {
        let context = vec![
            json!({"output":{"items":[{"text":"lead-a"}]}}),
            json!({"output":{"score":42}}),
        ];
        let value = json!({
            "message":{"$ref":"steps.0.output.items.0.text"},
            "score":{"$ref":"steps.1.output.score"}
        });
        let resolved = resolve_plugin_refs(value, &context).expect("resolve refs");
        assert_eq!(resolved["message"], "lead-a");
        assert_eq!(resolved["score"], 42);
    }

    #[test]
    fn rejects_forward_or_missing_references() {
        let error = resolve_plugin_refs(
            json!({"$ref":"steps.3.output"}),
            &[json!({"output":{"ok":true}})],
        ).expect_err("missing step must fail");
        assert!(error.contains("unavailable step"));
    }
}
