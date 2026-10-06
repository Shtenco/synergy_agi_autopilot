# SYNERGY AI AUTOPILOT v1 integration

This branch is the convergence line for the universal SYNERGY Autopilot: trusted Rust goal-to-result control plane + plugin fabric + bounded Python business sidecars + local/remote computer workers + NEXUS/Agent Fabric/Auto-Company providers.

Current integration status:
- OPEN AUTOPILOT Rust control plane retained as the authority boundary;
- PR #5 business runtime imported under `sidecars/business-runtime/`;
- universal `synergy.plugin.v1` contract and `autopilot-plugins` registry added;
- initial manifests: Local Commander, Remote MCP, Business Runtime, NEXUS, Agent Fabric, Auto-Company;
- CLI command `open-autopilot plugins` added for capability discovery;
- external side effects remain policy/approval scoped.

See:
- `docs/SYNERGY_AI_AUTOPILOT_V1_INTEGRATION.md`
- `docs/M5_UNIVERSAL_EXECUTION_FABRIC_STATUS.md`
- `docs/M6_FULL_AUTOMATION_STATUS.md`
- `docs/M7_CAPABILITY_GRAPH_STATUS.md`
- `docs/M8_SELF_BUILDING_PLUGIN_FABRIC_STATUS.md`
- `docs/M9_SEMANTIC_ARGUMENT_MAPPER_STATUS.md`
- `docs/architecture/plugin-fabric-v1.md`

Current convergence line includes restart-safe automation/event/approval/budget state, Universal Connector Gateway, a Capability Graph over all 73 currently accessible Shtenco repositories, a self-building plugin fabric, M9 typed semantic inputs/workflows, M10 verified typed outputs/cognitive DAGs, and M11 verified transforms, parallel DAG execution, outcome learning and a staffed AI organization.

---

# OPEN AUTOPILOT

**OPEN AUTOPILOT** is a Windows-first, hybrid-local-first autonomous AI engineering workstation.

Its target workflow is goal-to-result autonomy: accept a software objective, plan it, route work to specialist agents and models, isolate parallel changes, execute tools, verify evidence, repair failures, and ultimately deliver reviewed Git/PR/CI/release output under a local policy engine.

This is an **independent implementation**, not a fork of OpenCode, OpenClaw, or either Harness project. Their public MIT-licensed repositories were studied as architectural references; provenance is recorded in `THIRD_PARTY_NOTICES.md`.

## Current source status

The `feature/open-autopilot-v0.1` branch contains three implementation layers.

### M0 — deterministic autonomous core

Implemented in source:

- `Autopilot Director` deterministic goal state machine;
- bounded verification/repair loop;
- specialist roles: Director, Architect, Coder, Debugger, Tester, Reviewer, Security and Release;
- capability + risk Policy Engine;
- secure default policy: force-push and release publication denied; high-risk secret access and deletion require an approval boundary;
- append-only Audit Store abstraction;
- provider-neutral Multi-Model Router with deterministic ranking/fallback order;
- Native/Fake runtime abstractions;
- structured verification Evidence;
- deterministic unit/integration contracts for core behavior.

### M1 — coding-agent harness and autonomous execution infrastructure

Implemented in source:

- explicit `RunSpec -> RunResult` Agent Harness;
- builtin CLI backends for OpenCode, Codex, Claude Code and Gemini CLI;
- no implicit permission-bypass or danger flags;
- explicit model override, timeout and environment-variable allowlist;
- process environment isolation: agents receive a minimal system environment plus explicitly allowed variables;
- cancellable Native Runtime and emergency-stop signal;
- Git Workspace Manager with sanitized autopilot branches and worktree containment;
- Verification Pipeline producing structured pass/fail evidence;
- dependency-aware Agent Fleet Scheduler;
- cycle/unknown-dependency validation;
- parallel-writer protection for shared workspaces;
- `AutonomousEngine` connecting Director -> Agent Harness -> Verification Pipeline -> bounded repair loop;
- CLI commands for backend discovery, direct agent execution and autonomous goal execution.

### M2 — Windows Command Center

Implemented as source under `apps/desktop`:

- Tauri 2 native desktop host;
- React + TypeScript + Vite renderer;
- Full Autonomy Command Center;
- backend selector for OpenCode / Codex / Claude / Gemini;
- repository/workdir, prompt, model, timeout and explicit environment allowlist controls;
- direct invocation of the Rust Agent Harness from the native Tauri host;
- local execution output panel;
- Director + specialist Agent Fleet visualization;
- navigation surfaces for Goals, Workspace, Terminal, Verification, Models, MCP & Skills, Memory, Policy and Audit;
- emergency STOP command wired to the shared Native Runtime cancellation signal;
- secret **values never cross into the renderer** — the UI sends only names of environment variables that the native process may inherit.

The desktop UI currently exposes a direct single-agent execution slice. Full graphical multi-agent goal orchestration, streaming events, worktree/diff controls, persistent memory and GitHub automation remain subsequent integration work.

## Architecture

```text
Human Goal / Windows Command Center
              |
              v
      Autopilot Director
              |
      +-------+---------------------------+
      |                                   |
      v                                   v
 Policy Engine                       Agent Fleet Scheduler
                                          |
                     +--------------------+--------------------+
                     | Architect | Coder | Debugger | Tester   |
                     | Reviewer | Security | Release            |
                     +--------------------+--------------------+
                                          |
                                          v
                                 Multi-Model Router
                     OpenAI/Codex | Claude | Gemini | Local
                                          |
                                          v
                                   Agent Harness
                        OpenCode / Codex / Claude / Gemini
                                          |
                                          v
                               Runtime + Workspaces
                     Native Windows | WSL2* | Docker* | Remote*
                                          |
                                          v
                              Verification Evidence
                                          |
                               fail -------+------ pass
                                |                    |
                                v                    v
                         bounded repair       Git / PR / CI*
```

`*` = planned adapter/integration rather than a production-ready implementation in the current branch.

## Full Autonomy with guardrails

Full Autonomy is the primary operating model, but it is not unrestricted execution.

The trusted local control plane owns policy decisions, command execution, runtime cancellation and future secret storage. Model/tool adapters are intended to receive the minimum capability and context needed for their task. Dangerous Git/release operations are not silently escalated.

The Director cannot transition directly from execution to completion: verification evidence is required, and failed verification enters a bounded repair loop.

## CLI

Requires stable Rust and whichever external coding-agent CLI you choose to invoke.

### Deterministic core demo

```bash
cargo run -p open-autopilot -- demo --goal "verify the OPEN AUTOPILOT architecture"
```

### List builtin coding-agent backends

```bash
cargo run -p open-autopilot -- backends
```

### Run an installed coding agent directly

```bash
cargo run -p open-autopilot -- agent \
  --backend opencode \
  --workdir "C:\\dev\\project" \
  --prompt "Review the repository and implement the highest-impact verified fix" \
  --timeout-ms 1800000
```

### Run a goal through the autonomous Director + verification + repair loop

```bash
cargo run -p open-autopilot -- autopilot \
  --backend opencode \
  --workdir "C:\\dev\\project" \
  --goal "Fix the failing checkout flow and keep repairing until tests pass" \
  --verify-program cargo \
  --verify-arg test \
  --verify-arg --workspace \
  --max-repairs 2 \
  --timeout-ms 1800000
```

The verification command is represented as an executable plus argument array, not a shell command string.

Credentials are not inherited wholesale. Explicitly allow only the environment variables a selected tool needs, for example:

```bash
cargo run -p open-autopilot -- autopilot \
  --backend codex \
  --workdir "C:\\dev\\project" \
  --goal "Review and repair the project" \
  --allow-env OPENAI_API_KEY \
  --verify-program cargo \
  --verify-arg test
```

## Windows desktop development

From `apps/desktop`:

```bash
npm install
npm run tauri dev
```

Production packaging is intended to use Tauri's Windows bundler once desktop build verification and signing/release infrastructure are added.

## Core verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

A GitHub Actions workflow is committed for Windows and Ubuntu. At the time this branch was prepared, the connected repository's Actions jobs were terminating before runner assignment and exposing no executed workflow steps, so this README does **not** claim that CI has verified the branch yet.

The Tauri crate is deliberately excluded from the root Rust workspace until dedicated desktop CI installs the appropriate Tauri system/build dependencies.

## Repository layout

```text
apps/
  autopilot-cli/          Rust CLI
  desktop/                React/Tauri Windows Command Center
crates/
  autopilot-domain/       Stable IDs, roles, states, evidence
  autopilot-core/         Director orchestration
  autopilot-engine/       Real Director -> agent -> verification integration
  autopilot-policy/       Capability/risk policy engine
  autopilot-audit/        Audit abstractions
  autopilot-router/       Multi-model routing
  autopilot-runtime/      Native command execution/cancellation
  autopilot-harness/      Coding-agent CLI adapters
  autopilot-workspace/    Git/worktree isolation
  autopilot-verification/ Evidence-producing checks
  autopilot-fleet/        Dependency-aware fleet scheduling
docs/superpowers/         Architecture and implementation plans
```

## Next integration milestones

- Wire the Agent Fleet Scheduler directly into Director goal decomposition and parallel worktrees.
- Route each specialist role through Multi-Model Router before selecting an Agent Harness backend/model.
- Add WSL2 and Docker runtime adapters with health probing.
- Add streaming stdout/stderr and structured agent events to the desktop app.
- Persist goals, runs, audit events and project memory in SQLite.
- Add Git commit/push/PR and CI observation with automatic repair cycles.
- Add MCP/Skills registry under the same capability policy engine.
- Add remote capability-scoped workers.
- Add browser/Windows Computer Use as policy-governed tools.

## Engineering documents

- `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`
- `docs/superpowers/plans/2026-09-17-open-autopilot-v0.1.md`
- `docs/superpowers/plans/2026-09-17-open-autopilot-m1.md`
- `docs/superpowers/plans/2026-09-17-open-autopilot-m2-desktop.md`

## License and provenance

See `THIRD_PARTY_NOTICES.md`. Any future third-party source incorporation must be license-compatible and recorded with exact provenance; architectural inspiration alone is implemented clean-room here.


## M8 — Self-Building Plugin Fabric

When the Capability Graph cannot satisfy a goal, Autopilot can now create a structured semantic gap plan and attempt safe self-extension.

Current automatic discovery paths:

- OpenAPI documents;
- MCP configurations + sandbox `tools/list`.

Generated adapters move through:

`Quarantined -> Qualified/Rejected -> Promoted`

Safe read-oriented adapters may be auto-promoted under policy. High-risk or approval-required adapters remain blocked until explicit promotion/approval.

Use:

```text
open-autopilot semantic-gap
open-autopilot plugin-build
open-autopilot plugin-qualify
open-autopilot plugin-promote
open-autopilot universal-goal
```

Generated MCP/CLI programs require an explicit runtime executable allowlist, and Low/Medium execution requires a read-only isolating runtime.

Canonical qualification command:

```powershell
.\scripts\verify-autopilot.ps1
```


## M9 — Universal Semantic Argument Mapper + Adaptive Workflows

M9 adds transport-neutral `input_contracts`, deterministic semantic argument mapping, dry-run JSON-contract validation, runtime remapping from prior step outputs, and multi-step workflow synthesis.

Core commands:

```text
open-autopilot argument-map
open-autopilot workflow-plan
open-autopilot workflow-run
open-autopilot universal-goal
```

`universal-goal` now requires typed provider coverage for every workflow clause. Missing providers are self-built per clause through OpenAPI, MCP or declarative CLI discovery before workflow synthesis.

The old untyped fallback is intentionally removed from universal mode.

Canonical qualification:

```powershell
.\scripts\verify-autopilot.ps1
```


## M9 — Semantic Arguments + Adaptive Workflows

Autopilot now has a transport-neutral typed input layer.

The canonical path is:

```text
Human Goal
  -> Capability Graph
  -> typed provider?
       no -> M8 Self-Build Fabric
       yes
  -> input_contract
  -> Semantic Argument Mapper
  -> dry-run validation
  -> Adaptive Workflow Synthesizer
  -> approval / budget gates
  -> sequential execution
  -> live prior-output remapping
  -> final contract validation
  -> verification
```

M9 input contracts work across:

- OpenAPI parameters and request bodies;
- MCP `inputSchema`;
- declarative CLI descriptors;
- typed built-in capabilities.

Key operator commands:

```text
open-autopilot argument-map
open-autopilot workflow-plan
open-autopilot workflow-run
open-autopilot universal-goal
```

The workflow engine fails closed when:

- a required argument cannot be resolved;
- a goal clause has no mapped capability;
- a capability has no typed contract;
- schema validation fails;
- an approval-required step has no explicit approval.

Generated and built-in plugin calls cross the same final PluginExecutor input-contract validation boundary.

Canonical qualification:

```powershell
.\scripts\verify-autopilot.ps1
```

This verifier now covers M5 through M9.


## M10 — Verified Cognitive Planner + Typed DAG

Autopilot now treats outputs as typed contracts as well as inputs.

```text
input_contract
  -> plugin
  -> output_contract
  -> typed dataflow edges
  -> verified DAG
  -> next step
```

M10 adds:

- per-capability `output_contracts`;
- post-execution output validation in `PluginExecutor`;
- OpenAPI/MCP/declarative-CLI output contract discovery;
- exact MCP output normalization;
- typed schema assignability;
- ambiguity rejection;
- runtime-verified typed bindings;
- dynamic topological DAG ordering;
- `synergy.cognitive-plan.v1`;
- NEXUS model-assisted plan proposals behind a deterministic verifier.

A model is allowed to propose only:

```text
step id
objective
plugin_id
capability
hints
```

It cannot authoritatively set risk, approval, budgets, dependencies, credentials, or execution authority.

Useful commands:

```text
open-autopilot cognitive-plan
open-autopilot cognitive-plan-verify
open-autopilot universal-goal
```

Canonical offline M10 fixture:

```text
examples/m10/sales-to-file-cognitive-plan.json
```

It proves a typed edge from:

```text
business.sales.compose_offer.result.message
  -> computer.write_file.content
```

Canonical qualification remains:

```powershell
.\scripts\verify-autopilot.ps1
```

The verifier now covers M5 through M10.


## M11 — Verified Transforms + Parallel DAG + Outcome Learning + AI Company

M11 makes the verified DAG executable as a staffed, learning organization.

```text
Goal
 -> M10 Verified DAG
 -> Verified Transform Fabric
 -> AI Department / Employee staffing
 -> parallel-safe execution layers
 -> runtime validation
 -> verified outcome
 -> OutcomeStore
 -> advisory performance history for the next plan
```

New core crates:

- `autopilot-transform`
- `autopilot-outcomes`
- `autopilot-org`

Canonical organization:

`config/organization.default.json`

It currently defines **12 AI departments and 30 AI employees**.

Useful commands:

```text
open-autopilot org-validate
open-autopilot org-swarm
open-autopilot org-route
open-autopilot outcome-best
open-autopilot universal-goal
```

Canonical M11 transform fixture:

`examples/m11/browser-to-sales-transform-plan.json`

It proves:

```text
browser.snapshot
 -> results
 -> first
 -> text
 -> assert_string(min_length=1)
 -> business.sales.plan.message
```

Parallel execution is opt-in per capability with `parallel_safe=true`; the default is exclusive.

`universal-goal` now staffs the verified DAG with eligible AI employees, uses prior verified outcome history when enough evidence exists, and runs safe execution layers concurrently.

Canonical qualification:

```powershell
.\scripts\verify-autopilot.ps1
```

The verifier now covers M5 through M11.
