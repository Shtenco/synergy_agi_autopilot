# M11 — Verified Transform Fabric + Parallel DAG Runtime + Outcome Learning + AI Organization

Status: IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING

## Objective

M11 turns the M10 verified cognitive DAG into a multi-employee operating system:

Human goal
-> typed/cognitive planning
-> verified transforms
-> AI department staffing
-> parallel-safe DAG layers
-> plugin execution
-> output verification
-> outcome history
-> next-plan performance evidence.

The model remains a proposal generator. Trusted code owns contracts, transform proof, authority, staffing eligibility, approvals, parallel-safety, execution and outcome records.

## 1. Verified Transform Fabric

New crate:

`crates/autopilot-transform`

Allowed deterministic operations:

- get_field
- index
- first
- map_field
- filter_eq
- parse_json
- join_strings
- assert_string

Every transform has two boundaries:

1. schema proof before execution;
2. runtime application against the actual producer output.

Unsafe array projection fails closed. `first` requires `minItems >= 1`; an arbitrary index requires the schema to prove the item exists.

`assert_string` strengthens a weak string schema only by adding a runtime assertion. Example:

browser.snapshot.results[0].text
-> assert_string(min_length=1)
-> business.sales.plan.message

If the actual page text is empty, execution stops before Sales receives the value.

Canonical fixture:

`examples/m11/browser-to-sales-transform-plan.json`

## 2. Transform-aware cognitive DAG

`synergy.cognitive-plan.v1` now supports explicit `bindings`.

A binding contains:

- from_step
- to_step
- to_field
- a bounded VerifiedTransform

The proposal still cannot authoritatively set risk, approval, budgets or policy.

VerifiedCognitivePlanner:

- locates the declared producer output contract;
- infers the transform output schema;
- proves compatibility with the consumer input field;
- creates the typed dependency;
- recomputes unresolved fields;
- recomputes topological execution layers;
- rejects unknown steps, unsafe transforms, incompatible targets and cycles.

Runtime repeats the transform and target-field validation on the actual value.

## 3. Explicit parallel-safety

`PluginCapability` now contains:

`parallel_safe: bool`

Default is false.

Generated plugins and legacy plugins therefore remain exclusive unless explicitly reviewed.

Current opt-in includes selected read/local-compute capabilities across:

- NEXUS
- Local Commander reads/system info
- Remote MCP list/read
- Browser read actions
- Business analytical/composition capabilities
- Auto-Company status
- Connector Gateway read/consume operations

Write, send, publish, process, scheduling and approval-sensitive operations remain exclusive unless explicitly proven otherwise.

## 4. True parallel DAG runtime

New engine:

`ParallelPluginWorkflowEngine`

For each verified execution layer:

- take an immutable snapshot of all completed prior layers;
- partition steps into parallel-safe and exclusive;
- launch parallel-safe steps through Tokio JoinSet;
- execute exclusive steps serially;
- prevent same-layer results from leaking into each other's semantic mapper;
- append the layer outputs only after the layer is complete.

This preserves M10 determinism while enabling actual concurrency.

Integration test:

`crates/autopilot-engine/tests/m11_parallel.rs`

Two independent 120 ms parallel-safe plugin calls must complete below the sequential 240 ms envelope.

## 5. Outcome Learning

New crate:

`crates/autopilot-outcomes`

SQLite outcome records contain:

- stable SHA-256 goal fingerprint
- route key
- employee ID
- plugin provider
- model when runtime reports it
- device/transport endpoint class
- success
- verified success
- latency
- cost in micro-USD when runtime reports it
- error class
- quality score
- timestamp

The store calculates route statistics and a conservative score dominated by verified reliability.

A route needs a configurable minimum history before it can become preferred. Current staffing uses a minimum of two runs.

M11 records both:

- per-step employee/plugin outcomes;
- complete workflow route outcomes.

The cognitive fallback receives the top aggregate history as advisory evidence. It may use that history when proposing a new DAG, but all M10 verification remains mandatory.

## 6. AI Departments and AI Employees

New crate:

`crates/autopilot-org`

New schema:

`schemas/synergy.organization.v1.schema.json`

Canonical company:

`config/organization.default.json`

The default organization contains 12 AI departments and 30 AI employees.

Departments:

1. AI Executive Office
2. AI Sales Department
3. AI Marketing & SMM
4. AI Finance Office
5. AI Research & Intelligence
6. AI Product Department
7. AI Engineering
8. AI QA & Security
9. AI Operations & Automation
10. AI CRM & Customer Success
11. AI Creative Studio
12. AI Distributed Infrastructure

Each department defines:

- mission
- manager
- workspace
- memory namespace
- monthly budget namespace/value
- allowed capabilities

Each employee defines:

- stable employee ID
- name/title
- department
- supervisor
- instructions
- allowed capabilities
- allowed tools
- max risk
- model policy
- device policy
- workspace
- memory namespace
- autonomous flag

Organization validation rejects:

- duplicate departments/employees
- unknown managers/departments/supervisors
- supervision cycles
- employee capabilities outside department policy

Organization authority does not replace M6 approval/policy. A Sales employee may be eligible for `email.send`, but the plugin still requires explicit approval.

## 7. Outcome-aware staffing

After the DAG is verified, each step is matched to eligible autonomous employees by capability and max risk.

Selection:

1. eligible employees from organization policy;
2. if at least two historical runs exist for this goal and employee route, use best verified historical route;
3. otherwise deterministic organization order.

The selected employee ID and department are attached to the workflow step.

The AgentRole becomes a concrete identity such as:

`sales::AI Proposal & Outreach Specialist::sales-copy`

so audit and execution can preserve employee identity.

## 8. Agent Fabric bridge

Organization can compile every AI employee into an `EmployeeAgentDescriptor` containing:

- agent ID
- role
- department
- supervisor
- capabilities
- tools
- workspace
- memory namespace
- model policy
- device policy
- remote eligibility
- autonomous flag

CLI:

- `org-validate`
- `org-swarm`
- `org-route`
- `outcome-best`

`org-swarm` is the canonical bridge from organizational semantics to Agent Fabric / node swarm registration.

## 9. OpenDots architectural reference

The uploaded OpenDots source was reviewed as an MIT-licensed architectural reference.

Useful concepts incorporated clean-room at the architecture level:

- persistent specialist coworkers
- per-agent computer/workspace
- human review
- learning containers
- per-agent permissions
- stable agent/channel identity

SYNERGY does not import OpenDots/CopilotKit runtime into the trusted core.

SYNERGY implements these ideas using its own:

- Plugin Policy
- Agent Fabric
- Node Commander
- NEXUS memory
- M6 approvals/budgets
- M10 typed DAG
- M11 organization/outcome layers

Provenance is recorded in `THIRD_PARTY_NOTICES.md`.

## 10. universal-goal M11 path

Goal
-> Capability Graph
-> typed self-build
-> deterministic M9 plan
-> M10 cognitive fallback when needed
-> typed output proof
-> Verified Transform proof
-> dynamic DAG
-> approval gate
-> AI employee staffing
-> outcome-aware employee selection
-> parallel-safe execution layers
-> runtime transform/input/output validation
-> verification
-> per-step outcomes
-> workflow outcome
-> next-run learning evidence.

Default outcome DB:

`.autopilot/outcomes.db`

A custom organization file can be supplied to universal-goal.

Parallel execution is enabled by default and can be disabled to use the legacy sequential PluginGoalEngine.

## Qualification

Canonical verifier:

`scripts/verify-autopilot.ps1`

Now covers M5 through M11.

Rust workspace tests include:

- transform proof/runtime tests
- organization policy tests
- default organization validation
- outcome learning tests
- real timing-based parallel DAG test

CLI smoke includes:

- M10 cognitive fixture
- M11 transform fixture
- organization validation

JSON validation includes:

- plugin schema
- CLI schema
- cognitive-plan schema
- organization schema
- default organization
- M10/M11 fixtures

## Physical status

M11 must not be called GREEN until a real Rust runner executes the suite.

Known environment limitation remains external to the source tree when Actions jobs receive no runner and expose no executed steps.

## Explicit boundaries

M11 does not claim:

- unrestricted parallel writes
- implicit array element selection
- model authority over transforms
- automatic bypass of approval gates
- exact model cost when the provider/runtime does not report it
- production multi-device swarm evidence
- that one or two successful runs are enough to promote a route
- that outcome history is ground truth rather than advisory evidence

Next logical milestone after physical M11 qualification is a distributed organization runtime: durable employee sessions, cross-device department swarms, explicit task delegation/escalation, department budgets and long-horizon performance evaluation.
