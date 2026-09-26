# M9 — Universal Semantic Argument Mapper + Adaptive Workflow Synthesizer

Status: IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING

## Canonical flow

Human Goal
→ Capability Graph
→ typed provider?
→ if missing: M8 Self-Build Fabric
→ input contract
→ semantic argument mapping
→ dry-run validation
→ adaptive workflow synthesis
→ approvals / budgets
→ sequential execution
→ live prior-output remapping
→ final contract validation
→ plugin execution
→ verification

## Transport-neutral input contracts

synergy.plugin.v1 now supports input_contracts keyed by capability.
This lets one M9 mapper operate across OpenAPI, MCP inputSchema, declarative CLI and built-in capabilities.
Input contracts must reference declared capabilities and must be JSON object schemas.

## Typed built-ins

M9 contracts now cover core NEXUS intelligence/memory, Browser, Local Commander, Remote MCP, executable Agent Fabric schedule/lease operations, and Business Runtime.
Remote MCP contracts were derived from the actual synergy_shttps methods list_dir(path), read_file(path), and run(argv,cwd).
Declared but not yet executable Agent Fabric surfaces are intentionally left without M9 contracts.

## Contract validator

JsonContractValidator performs fail-closed validation before plugin dispatch.
Supported features include type, required, properties, additionalProperties=false, items, enum, const, minLength/maxLength, minimum/maximum, minItems/maxItems, email/URI formats, allOf, anyOf, oneOf and bounded local $ref resolution.
Remote schema fetching is intentionally disabled.

## Semantic Argument Mapper

Resolution priority:
1. explicit hints
2. exact memory field
3. controlled memory aliases
4. exact prior-step output
5. controlled prior-output aliases
6. schema const/default
7. deterministic goal extraction
8. unresolved

Mandatory values are never silently invented.
If a required value cannot be resolved, unresolved_required remains non-empty and execution stops unless the field can be deferred to a prior workflow step.

Controlled aliases include:
- to <- email / recipient
- body <- message / text / content / proposal
- subject <- title
- client_id <- lead_id / customer_id / id
- url <- link / website / site
- path <- file_path / filepath

Goal extraction understands email addresses, URLs, quoted strings, explicit IDs, path-like values, enums, field-bound numbers/booleans, and full goal text for query/prompt/goal/instruction.
A broad first-number heuristic was removed so one number cannot be copied into unrelated numeric fields.

## Adaptive runtime dataflow

PluginExecutorBridge remaps inputs immediately before each step using actual outputs of previous steps.
The Director core executes planned tasks sequentially, so prior outputs are deterministic.
Example: a previous output with email and message can satisfy a following contract requiring to and body through controlled aliases.
If the field remains missing, the external plugin is not called.

## M6 control-plane preservation

Semantic remapping preserves _approval_id and _budget.
OAuth settings are treated as trusted connector metadata rather than business arguments.
Persistent approval verification remains bound to the exact plugin_id + capability.

## Final runtime validation

PluginExecutor validates sanitized final input against the manifest input contract.
This applies to manual plugin-call, workflow-run, universal-goal and future callers.

## OpenAPI mapping

Generated OpenAPI contracts include path-level parameters, operation-level parameters and request body schemas.
Parameter locations are preserved through x-location.
Flat typed arguments are transformed into path_params, query_params, header_params and body.
Connector Gateway now supports encoded query parameters and bounded headers.
Generated headers cannot override Host, Content-Length, Authorization or Connection.
OAuth authorization remains in the trusted Secret Vault path.
Bounded local OpenAPI $ref values are expanded against the source document.

## MCP inputSchema

MCP discovery now stores tools/list inputSchema as the generated capability input contract.
Bounded local JSON Schema refs are resolved before mapping/validation.

## Declarative CLI

M9 introduces synergy.cli-capabilities.v1 with canonical schema schemas/synergy.cli-capabilities.v1.schema.json.
A descriptor declares program, exact capability, fixed args, risk, approval requirement and input_schema.
CLI capability matching is exact. Autopilot does not infer that crm.read equals crm.record.update.
The executable must still pass the M8 runtime allowlist and sandbox qualification.

## Action-aware capability inference

Examples:
- отправь email -> email.send
- прочитай email -> email.read
- создай запись crm -> crm.record.create
- обнови запись crm -> crm.record.update
- открой сайт -> browser.navigate
- нажми кнопку -> browser.click
- заполни форму -> browser.fill
- запиши файл -> computer.write_file

Capability Graph action-verb aliases were expanded accordingly.

## Typed-gap self-build

A provider is no longer sufficient for M9 merely because it exists.
If providers exist but none expose an input contract, the goal is treated as a typed semantic gap and universal-goal may continue into M8 discovery to build a typed adapter.

## Adaptive Workflow Synthesizer

Explicit multi-step goals separated by arrows, then/потом/затем, semicolons or lines become ordered workflow steps.
Each step records objective, plugin, capability, risk, contract, mapped arguments, unresolved fields, validation errors, dependencies and approval requirement.
The current execution graph is an ordered deterministic DAG, favoring correctness and data availability over speculative parallelism.

## No silent partial workflows

If any goal clause cannot be mapped, it is stored in unresolved_objectives and plan.ready becomes false.
The system cannot omit an unsupported part of the user's goal and claim success.

## CLI

argument-map: inspect typed arguments for one capability.
workflow-plan: synthesize and dry-run a workflow without execution.
workflow-run: execute a ready workflow.
Approval-required steps need --approve-step step-N.
With persistent M6 approvals, use --approval-id step-N=<approval-uuid>.

## Universal Goal integration

universal-goal now performs M7 routing, M8 typed self-build when needed, M9 workflow synthesis, dry-run readiness checks, explicit approval checks, PluginGoalEngine execution, runtime remapping, final validation and verification.
GraphGoal remains only a fallback when no semantic workflow can be synthesized at all.

## Tests and qualification

M9 tests cover semantic field extraction, unresolved required rejection, exact and alias prior-output dataflow, JSON types and combinators, bounds, local refs, deferred dependent arguments, partial workflow rejection, action-specific capability inference, OpenAPI path/body contracts, typed-gap rebuilding, declarative CLI discovery and MCP inputSchema propagation.

Canonical local verifier: scripts/verify-autopilot.ps1
It now represents M5–M9 and validates Rust fmt/clippy/tests, browser, sidecars, every plugin JSON manifest, synergy.plugin.v1 schema, synergy.cli-capabilities.v1 schema, M8 OpenAPI discovery smoke and M9 CLI descriptor discovery smoke.

## Deliberate boundaries

M9 does not invent mandatory identifiers, guess arbitrary CLI argv, fetch remote schemas, auto-approve write operations, bypass budgets/approvals, or accept partial workflows as success.
Model-assisted semantic proposals can be added later, but their output must still cross the same deterministic contract validation and approval boundaries.