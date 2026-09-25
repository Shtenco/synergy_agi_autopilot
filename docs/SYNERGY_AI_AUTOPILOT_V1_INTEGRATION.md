# SYNERGY AI AUTOPILOT v1 — integration baseline

## Canonical product repository

`Shtenco/synergy_agi_autopilot` is the canonical product control plane.

No new Autopilot repository is required.

## v1 composition

### Trusted control plane — Rust

Inherited from `feature/open-autopilot-v0.1`:

- Director and deterministic goal state machine;
- Agent Fleet;
- Multi-Model Router;
- Agent Harness;
- Runtime adapters;
- policy and audit layers;
- verification/repair loop;
- computer use;
- mission persistence/recovery;
- Tauri desktop Command Center.

### Business capability sidecar — Python

Imported from `feat/capability-runtime-v1` under:

`sidecars/business-runtime/`

It contains bounded CEO/CFO/SMM/Sales/Video/Coder/Verifier capabilities and the existing `synergy.capability.v1` HTTP task runtime.

The Python runtime remains outside the trusted Rust authority boundary.

### Universal Plugin Fabric

New crate:

`crates/autopilot-plugins`

Contract:

`synergy.plugin.v1`

Initial manifests:

1. `computer.local.commander`
2. `computer.remote.shttps`
3. `business.capability.runtime`
4. `intelligence.nexus`
5. `workers.agent.fabric`
6. `business.auto-company`

## Target execution path

```text
Human Goal
   ↓
Director
   ↓
Goal DAG
   ↓
required capabilities
   ↓
Plugin Registry
   ↓
Policy / approvals / budget
   ↓
Model + Agent Fleet
   ↓
local tools | browser | desktop | business APIs | remote workers
   ↓
verification evidence
   ↓
bounded repair
   ↓
result + audit trail
```

## Authority rules

- plugin declaration is not permission;
- model output is not authorization;
- financial/accounting authority is not granted to Autopilot plugins;
- external side effects can be approval-gated;
- dangerous local/remote computer actions remain capability-scoped;
- secrets stay in trusted storage/process environment and are not embedded in manifests;
- remote execution must use authenticated transport and workspace/tool boundaries.

## Integration gates

### Gate A — repository convergence

- [x] create v1 integration branch from OPEN AUTOPILOT;
- [x] import PR #5 business runtime as sidecar;
- [x] create `synergy.plugin.v1`;
- [x] add Plugin Registry crate;
- [x] declare first six provider manifests;
- [ ] make plugin manifests discoverable from CLI/Desktop;
- [ ] add policy evaluation for plugin capabilities.

### Gate B — executable adapters

- [ ] local Node Commander MCP adapter;
- [ ] Remote MCP/SHTTPS adapter;
- [ ] Agent Fabric worker adapter;
- [ ] NEXUS compatibility sidecar;
- [ ] Auto-Company adapter;
- [ ] business runtime process lifecycle.

### Gate C — universal automation

- [ ] trigger/event engine;
- [ ] scheduler;
- [ ] OAuth and secret vault;
- [ ] OpenAPI adapter;
- [ ] webhook/queue/database adapters;
- [ ] mobile/IoT adapter contracts;
- [ ] plugin health/circuit-breaker telemetry.

### Gate D — first universal vertical slice

One human goal must execute across multiple capability domains:

```text
find/ingest lead
→ create sales/content plan
→ perform approved browser/API action
→ update business state
→ delegate one task to local/remote worker
→ verify evidence
→ produce final report
```

Completion requires deterministic evidence for every external side effect.
