# M10 — Verified Cognitive Planner + Typed Output Contracts + Dynamic DAG Optimizer

Status: IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING

## Objective

M10 makes Autopilot workflows typed in both directions:

input_contract -> plugin -> output_contract -> typed dataflow -> next step.

It also introduces a model-assisted planner that is treated as an untrusted proposal generator.
Risk, approvals, contracts, dependencies and execution authority are reconstructed and verified deterministically.

## Typed output contracts

synergy.plugin.v1 now supports per-capability output_contracts.
PluginExecutor validates actual PluginResult.output after transport success.
A successful HTTP status, MCP response or CLI exit code is not sufficient if the returned value violates its output contract.
Invalid output is reported as PluginRuntimeError::InvalidOutput and counts as a circuit-breaker failure.

Generated adapters can derive output contracts automatically:
- OpenAPI: JSON response schema from 2xx/default responses, with bounded local $ref resolution.
- MCP: tools/list outputSchema.
- Declarative CLI: output_schema from synergy.cli-capabilities.v1.

## MCP output normalization

MCP tool results are normalized before output validation:
1. structuredContent if present;
2. a single text content item parsed as JSON when possible;
3. otherwise a single text content item becomes a scalar string;
4. otherwise the MCP result envelope remains intact.

This aligns outputSchema with the actual typed payload while preserving compatibility with existing MCP servers.

## Typed built-in outputs

M10 currently defines verified output contracts for:
- NEXUS intelligence and memory;
- Local Commander read/write/run/system information;
- Remote MCP list/read/run;
- Browser run/navigate/snapshot/click/fill/wait/screenshot;
- Business Runtime CEO/CFO/SMM/Sales/Verifier;
- Business sales.compose_offer, exposing result.message;
- Agent Fabric worker.schedule/worker.lease TaskLease;
- Auto-Company status/opportunity/approve/dispatch/pause;
- direct Connector Gateway DB and queue operations.

Generic Email/Calendar/CRM packs remain intentionally untyped until bound to a concrete external provider.

## Typed schema matcher

TypedSchemaMatcher proves whether an output field can satisfy an input field.

Supported safe compatibility includes:
- exact primitive types;
- integer -> number widening;
- exact required formats such as email;
- enum/const restrictions;
- numeric, string and array bounds;
- recursively compatible required object properties;
- bounded schema combinators.

Unknown producer types fail closed.
A generic string cannot satisfy format=email.
A generic producer cannot satisfy a restricted enum/const consumer.

## Controlled field aliases

Typed edges can use explicit controlled aliases, including:
- recipient_email/email -> to;
- message/text/proposal -> body/content;
- title -> subject;
- lead_id/customer_id/id -> client_id;
- link/site -> url.

Aliases are lower confidence than exact-name matches.
Equal-score ambiguous sources are not auto-bound.

## Verified typed dataflow edges

WorkflowPlan now contains typed_edges.

Each edge records:
- from_step;
- from_path;
- to_step;
- to_field;
- compatibility class;
- score.

Example:

proposal.result.message -> save.content

The edge exists only when both the name/alias and schemas are compatible.

## Runtime typed binding verification

Typed edges are executable.
Before each consumer step:
- source step is located by stable step_id;
- exact from_path is read from the actual prior PluginResult.output;
- the value is revalidated against the target field schema;
- only then is it injected into semantic hints.

If the field disappeared, changed type, changed format or violated bounds, the next external action is stopped.

## Dynamic DAG optimizer

VerifiedDagOptimizer removes artificial linear dependencies and creates only real typed dependencies.

WorkflowPlan now contains execution_layers.
Topological ordering is computed from verified edges.
A model may list a consumer before a producer; M10 can reorder them when schemas prove the dependency.
Cyclic DAGs are rejected.

Independent steps can occupy the same execution layer, although the current PluginGoalEngine executes the verified order sequentially.
Parallel layer execution is deliberately deferred to a later milestone.

## SalesProposal -> file fixture

examples/m10/sales-to-file-cognitive-plan.json is the canonical offline M10 fixture.

It proposes:
1. business.sales.compose_offer
2. computer.write_file

The verifier must prove:

proposal.result.message -> save.content

while path/mode come from explicit hints.

This fixture requires no model call and is included in CI/local qualification.

## Verified Cognitive Planner

Models do not produce executable plans directly.
They produce synergy.cognitive-plan.v1 proposals containing only:
- stable step id;
- bounded objective;
- exact plugin_id;
- exact capability;
- optional hints.

The proposal schema intentionally has no fields for:
- risk;
- approval;
- dependency;
- credentials;
- budget;
- policy;
- authority.

VerifiedCognitivePlanner reconstructs those from PluginRegistry and contracts, maps arguments, builds typed edges, optimizes the DAG and rejects unknown/untyped capabilities.

## NEXUS cognitive proposal path

cognitive_planner_prompt exposes only typed capabilities to NEXUS.
The model is instructed to return JSON only and not invent providers or authority.

CLI:
- cognitive-plan: NEXUS proposal -> deterministic verification -> verified DAG.
- cognitive-plan-verify: verify an offline proposal without a model call.

## universal-goal integration

universal-goal now follows:

Goal
-> M7 Capability Graph
-> M8 typed self-build where possible
-> M9 deterministic workflow synthesis
-> if deterministic workflow is proven: use it
-> otherwise invoke NEXUS M10 cognitive proposal
-> VerifiedCognitivePlanner
-> typed edge proof
-> topological DAG
-> unresolved/ambiguous/cyclic? STOP
-> approval-required? STOP for explicit approval
-> PluginGoalEngine
-> runtime typed binding validation
-> input validation
-> plugin execution
-> output validation
-> verification.

M8 DiscoveryEmpty/CandidateRejected can now defer to the cognitive fallback instead of terminating the whole goal prematurely.
QualifiedAwaitingPromotion remains a hard security gate and cannot be bypassed by the cognitive planner.

## Qualification

scripts/verify-autopilot.ps1 now represents M5-M10.

CI/local verification includes:
- cargo fmt;
- clippy;
- workspace tests;
- M10 cognitive-plan-verify fixture;
- built-in plugin manifests;
- synergy.plugin.v1 schema;
- synergy.cli-capabilities.v1 schema;
- synergy.cognitive-plan.v1 schema;
- M8 OpenAPI self-build smoke;
- M9 CLI input/output contract smoke;
- Python sidecars;
- browser/desktop checks.

## Physical status

M10 is not GREEN until a real Rust runner executes the qualification suite.
The current environment has no rustc/cargo and cannot reach GitHub over DNS, so local physical Rust qualification is unavailable here.

## Explicit boundaries

M10 does not:
- trust a model's risk or approval claims;
- execute unknown/untyped model-proposed capabilities;
- auto-resolve ambiguous equal-score data sources;
- infer array projections implicitly;
- fetch remote schemas;
- bypass M6 budgets or approvals;
- treat invalid plugin output as success;
- execute approval-required cognitive steps automatically.

Array projections/transforms and true parallel execution are intentionally left for the next milestone.