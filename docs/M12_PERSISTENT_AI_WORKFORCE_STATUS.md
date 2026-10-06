# M12 — Persistent AI Workforce + Department Autonomy + Cross-Device Swarm Company

Status: IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING / REMOTE EXECUTION BLOCKED BY P7.4

## Objective

M12 turns M11 AI employees from ephemeral step identities into durable digital workers.

Canonical lifecycle:

Human / CEO / Director / Automation
-> persistent work item
-> employee inbox
-> eligibility / skill / tool / node / risk checks
-> durable lease
-> NEXUS employee memory hydration
-> PluginExecutor
-> M6 approval + budget authority
-> typed output validation
-> performance record
-> NEXUS memory checkpoint
-> KPI / history
-> retry / escalation
-> next company cycle.

## 1. Persistent Workforce Core

New crate:

`crates/autopilot-workforce`

New schema/config:

- `schemas/synergy.workforce.v1.schema.json`
- `config/workforce.default.json`

Default durable DB:

`.autopilot/workforce.db`

The DB is the M12 source of truth for workforce runtime state.

It does not replace:
- M11 organization authority;
- NEXUS memory authority;
- M6 approval/budget authority;
- Agent Fabric scheduling authority;
- P7/P6 remote transport authority.

## 2. Persistent Employee Runtime

Each employee has durable runtime state:

- employee ID
- department
- idle/working status
- current work item
- bound node
- memory namespace
- personal workspace
- model profile
- budget identity
- heartbeat
- updated timestamp

`workforce-init` seeds all canonical M11 employees into the persistent runtime.

## 3. Inbox + Work Items

Every durable task records:

- work item ID
- department
- optional assigned employee
- creator
- parent task
- goal
- plugin + capability
- typed input
- risk
- priority
- status
- retry attempts
- max attempts
- lease owner / expiry
- required skills
- required tools
- budget ID / amount
- approval ID
- escalation level
- escalation deadline
- result
- error
- timestamps

Inbox selection fails closed when:
- employee lacks capability;
- task risk exceeds employee authority;
- required skill is below threshold;
- required tools are missing;
- bound node is offline;
- bound remote node cannot yet be executed safely.

## 4. Durable Lease / Recovery

`lease_next` atomically moves a task:

`pending -> leased`

and employee:

`idle -> working`

Expired leases are recovered by M12 maintenance.

This prevents abandoned work from permanently blocking a digital employee after process/node failure.

## 5. Retry + Escalation

A failed task:

- releases the employee;
- records failed performance;
- returns to pending while attempts remain;
- schedules escalation;
- becomes failed after max attempts.

Escalation target:

1. direct supervisor when present;
2. otherwise department manager.

This gives the organization a real management chain rather than flat agent retries.

## 6. Persistent Employee Memory

NEXUS remains the memory authority.

Before execution M12 performs:

`memory.search`

inside the employee's own namespace.

The retrieved memory is injected into the trusted workforce context.

After successful verified work M12 stores a bounded summary with:

`memory.upsert`

and records a local checkpoint:

`nexus://<namespace>/work:<work-item-id>`

The local DB stores only checkpoint/provenance metadata, not a competing semantic memory database.

## 7. Skills

Persistent table:

`employee_skills`

Each skill has:

- employee
- skill key
- level 0..1
- evidence count
- updated timestamp

Tasks may require explicit skill thresholds.

Skill eligibility is checked before lease acquisition.

## 8. KPIs + Long-Term Performance

Persistent KPI table contains:

- target
- current value
- weight

Persistent performance history contains:

- employee
- work item
- success
- verified flag
- latency
- quality
- cost
- timestamp

Employee score combines:

- verified performance: 50%
- quality: 30%
- KPI attainment: 20%

M12 therefore has long-horizon workforce performance rather than only M11 per-route outcomes.

## 9. Initiatives

Persistent initiatives contain:

- owner
- department
- title
- objective
- status
- score
- timestamps

Lifecycle currently implemented:

`proposed -> planning`

Promotion creates a persistent manager work item.

All department managers have bounded `intelligence.reason` capability.

The planning task instructs the manager to use only department-authorized capabilities.

It does not grant execution authority for side effects.

## 10. Schedules

M6 Automation remains scheduling authority.

M12 stores links:

`employee -> automation_id -> purpose`

This avoids creating a second scheduler with conflicting truth.

## 11. Budgets + Approvals

M6 Control Plane remains authority.

For persistent employees M12 is stricter than one-shot execution:

- approval-required capability + no approval ID -> fail/retry;
- approval ID + no persistent control-plane DB -> fail/retry;
- non-zero budget reservation + no persistent control-plane DB -> fail/retry.

Therefore a long-lived worker cannot treat an arbitrary approval string as authority.

## 12. Personal Computer / Node

Persistent node registry stores:

- node ID
- kind
- endpoint
- advertised capabilities
- CPU
- RAM
- online state
- heartbeat

Employees can be bound to a node.

Stale nodes are automatically marked offline.

## 13. Cross-Device Boundary

Control-plane binding is implemented now.

Examples:

`local://desktop-1`

can be used by the current M12 local execution path.

Remote endpoints are registered and visible, but execution fails closed when endpoint is not local.

Reason: the current P7 Agent Fabric source proves local scheduling/leases but P7.4 still marks these as incomplete:

- remote agent advertisement over PQ-MESH
- encrypted task delegation
- remote lease acceptance
- result return over PQ-MESH
- worker failover across physical nodes

M12 therefore does not fake remote execution.

## 14. Agent Fabric Bridge

`worker.register` is now implemented through PluginExecutor.

However current `agent-fabric-stdio` is a one-shot process.

Therefore:
- M12 Workforce DB is the durable employee authority;
- Agent Fabric registration inside one stdio invocation is ephemeral;
- scheduling/lease decisions remain useful primitives;
- persistent distributed registration requires a long-lived P7 service or P7.4 transport.

This distinction is explicit.

## 15. Department Autonomy

`run_department_once` starts all autonomous employees in one department concurrently.

Each employee:
- checks personal/department inbox;
- leases one eligible task;
- hydrates memory;
- executes through PluginExecutor;
- persists result/performance/memory;
- releases itself.

## 16. Company Operating Cycle

`run_company_once` executes:

1. maintenance / expired lease recovery / escalations / stale-node detection;
2. leadership department;
3. every other department concurrently.

This creates a persistent company cadence:

`CEO -> directors -> departments -> employees`

without bypassing capability policy.

## 17. CLI

Implemented commands include:

- `workforce-init`
- `workforce-status`
- `workforce-node-register`
- `workforce-bind-node`
- `workforce-heartbeat`
- `workforce-skill-set`
- `workforce-kpi-set`
- `workforce-initiative-add`
- `workforce-initiative-list`
- `workforce-initiative-promote`
- `workforce-schedule-link`
- `workforce-enqueue`
- `workforce-run`
- `workforce-department-cycle`
- `workforce-company-cycle`
- `workforce-maintain`
- `workforce-sync-swarm`

## 18. Qualification

Canonical verifier:

`scripts/verify-autopilot.ps1`

now covers M5 through M12.

M12 qualification includes:
- workforce Rust unit tests;
- organization bootstrap;
- durable employee runtime;
- skill + node gated lease;
- escalation to supervisor;
- employee performance/KPI score;
- workforce schema validation;
- default workforce config validation;
- CLI offline workforce-init smoke;
- CLI workforce-status smoke.

## 19. Physical Status

Do not mark M12 GREEN until a real Rust runner executes:

- fmt
- clippy
- cargo test workspace
- CLI workforce smoke
- full M5-M12 verifier

Remote multi-device GREEN additionally requires the missing P7.4 transport and physical multi-node evidence.

## 20. Current Evidence Boundary

Implemented:
- persistent employees
- persistent inbox
- leases/recovery
- retry/escalation
- skills
- KPIs
- initiatives
- schedule links
- personal workspace/model/budget identity
- NEXUS memory hydration/checkpoint
- node registry/binding/heartbeat
- department cycles
- company cycles
- persistent approval/budget enforcement
- Agent Fabric registration adapter

Not yet claimed:
- durable Agent Fabric daemon registry
- encrypted cross-device task delivery
- remote result return
- phone/server physical execution
- autonomous conversion of arbitrary manager prose into unverified side-effect tasks
- P7 execution receipts for every M12 work item
- SYNA reward settlement for employee work
- production GREEN

The next integration after M12 core qualification should connect durable workforce tasks to P7.4 PQ-MESH delegation + execution receipts + verifier/reward settlement.
