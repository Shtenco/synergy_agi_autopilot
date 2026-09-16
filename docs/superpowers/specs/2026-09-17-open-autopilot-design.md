# OPEN AUTOPILOT — Architecture & Product Specification

## 1. Product mission

OPEN AUTOPILOT is a Windows-first, local-first autonomous AI engineering workstation. Its purpose is to turn a high-level human goal into a verified software result with minimal human intervention: inspect a repository, understand intent, plan, modify code, run commands and tests, coordinate multiple specialist agents, review changes, repair failures, create commits and pull requests, observe CI, and prepare a release.

The product is not a fork of OpenCode, OpenClaw, or Harness. It is an independent system that may reuse compatible MIT-licensed ideas or components with attribution where appropriate. The architecture must keep external runtimes replaceable behind stable adapters.

## 2. Approved product decisions

- Product mode: Hybrid. Version 1.0 prioritizes Coding Autopilot while the core is designed for later Windows Computer Use, browser automation, and arbitrary desktop tools.
- Autonomy: Full Autonomy is the primary operating mode. The agent may carry a task from request to PR/release under policy controls and complete audit logging.
- Deployment: Hybrid local-first. Secrets, local repositories, command execution, durable memory, policy decisions, and audit logs belong to the local machine. Optional cloud workers may handle parallel or heavy workloads.
- Intelligence topology: one Autopilot Director controls a specialized Agent Fleet.
- Agent roles in v1: Architect, Coder, Debugger, Tester, Reviewer, Security, Release.
- Parallelism: independent work executes in isolated git worktrees when possible.
- Model strategy: dynamic Multi-Model Router supporting OpenAI/Codex, Anthropic/Claude, Google/Gemini, OpenCode-compatible providers, local OpenAI-compatible endpoints, and future providers.
- Windows runtime: Dual Runtime. Native Windows/PowerShell and isolated WSL2/Docker execution are first-class targets.

## 3. Product principles

1. Goal-driven, not chat-driven. Chat is an interface; the unit of work is a Goal.
2. Evidence before completion. An agent cannot mark a goal complete without objective verification evidence.
3. Local ownership. Repositories, credentials, policies, durable memory, and action history remain local by default.
4. Provider neutrality. No core logic may depend on one model vendor.
5. Runtime neutrality. Native Windows, WSL2, Docker, remote workers, and future sandboxes implement one execution interface.
6. Isolation by default. Parallel code changes occur in worktrees or separate disposable sandboxes.
7. Reversible actions. Destructive operations require policy evaluation, snapshots/backups where feasible, and audit records.
8. Explainable automation. Every important decision records rationale, selected model/runtime, inputs, outputs, and verification.
9. Deterministic orchestration around probabilistic models. State transitions, retries, budgets, policies, and acceptance gates belong to deterministic code.
10. Open interfaces. Model providers, tools, agents, MCP servers, skills, and workers are plug-ins.

## 4. User experience

### 4.1 Main application areas

The Windows desktop app exposes the following primary surfaces:

- Command Center: current goals, run status, cost, blockers, active agents, current verification state.
- Goal Composer: natural-language goal plus repository, constraints, acceptance criteria, autonomy profile, and budget.
- Agent Fleet: live visualization of Director and specialists, queued/running/completed tasks, model assignment, runtime, and worktree.
- Workspace: file changes, diff viewer, repository state, branches, commits, PRs.
- Terminal: multiplexed shell sessions with command provenance and output streaming.
- Verification: tests, lint, type-check, build, security findings, review findings, CI checks.
- Models: providers, models, quotas, routing policy, latency/cost/quality statistics.
- MCP & Skills: registered tool servers, skills, capabilities, permission scopes.
- Memory: project facts, decisions, prior failures, recurring commands, conventions, constraints.
- Policies & Security: execution policies, allow/deny rules, protected paths, credential access, destructive action controls.
- Audit: immutable event timeline for every run.
- Settings: Windows runtime, WSL/Docker, GitHub, providers, paths, storage, updates.

### 4.2 Primary user flow

1. User opens or clones a repository.
2. User enters a goal such as: “Fix the crash in checkout, add regression tests, open a PR, and watch CI until green.”
3. Director creates a structured GoalPlan with acceptance criteria.
4. Policy Engine validates allowed scope and budgets.
5. Director decomposes the goal into a dependency graph.
6. Scheduler dispatches tasks to specialist agents.
7. Model Router assigns a model based on role, context, cost, latency, quality history, and provider health.
8. Workspace Manager creates isolated worktrees/sandboxes for parallel changes.
9. Executor runs tools and commands. All actions become AuditEvents.
10. Reviewer/Security/Tester cross-check implementation.
11. Director merges approved work, reruns project-level verification, and repairs failures in a bounded loop.
12. Git adapter creates commit(s), pushes a branch, opens a PR, and observes CI when configured.
13. Release Agent may prepare a release only when release policy permits it.
14. Director marks the goal complete only when all mandatory acceptance gates pass.

## 5. System architecture

### 5.1 Desktop shell

Use Tauri 2 for the Windows application shell. The UI is TypeScript + React. Tauri is chosen for smaller footprint and direct integration with a Rust core.

Desktop responsibilities:
- render application state;
- receive event streams from the core;
- submit goals and user interventions;
- display diffs, terminal streams, fleet state, policies, model/provider health;
- never store provider secrets in browser/localStorage.

### 5.2 Core control plane

The Rust core is the trusted local control plane. It owns:
- state machine;
- orchestration;
- agent scheduling;
- policy evaluation;
- model routing;
- runtime selection;
- persistence;
- event/audit log;
- cancellation and emergency stop;
- budgets and retry limits.

Core modules:
- `domain`: stable types and state machines.
- `director`: goal planning and orchestration loop.
- `fleet`: specialist agent definitions and task dispatch.
- `router`: model/provider ranking, selection, fallback, circuit breakers.
- `runtime`: command execution abstraction for native, WSL2, Docker, remote.
- `workspace`: repository/worktree lifecycle.
- `policy`: capability-based authorization and risk classification.
- `verification`: acceptance gates and evidence aggregation.
- `memory`: project/run knowledge and retrieval interface.
- `audit`: append-only event recording.
- `tools`: normalized tool registry, MCP bridge, Git/GitHub operations.
- `storage`: SQLite persistence.

### 5.3 Domain model

`Goal` is the top-level user intent.

`GoalPlan` is the Director’s structured decomposition containing:
- goal id;
- objective;
- constraints;
- acceptance criteria;
- task DAG;
- budget;
- autonomy policy;
- repository context.

`AgentTask` contains:
- id and parent goal;
- specialist role;
- objective;
- dependencies;
- allowed capabilities;
- preferred runtime;
- context references;
- verification requirements;
- retry budget.

`AgentRun` is one execution attempt of an AgentTask using a specific model/runtime/worktree.

`Evidence` is a structured verification result: command exit code, test summary, build result, review finding resolution, security result, CI status, artifact checksum, or human approval when explicitly required.

`AuditEvent` is immutable and timestamped.

### 5.4 Autopilot Director

The Director is not another unrestricted chat loop. It executes a deterministic orchestration state machine:

`Created -> Planning -> PolicyCheck -> Scheduling -> Executing -> Reviewing -> Verifying -> Repairing -> Publishing -> Completed | Failed | Cancelled`

The Director uses an LLM for planning and reasoning but the transition logic lives in Rust.

Responsibilities:
- construct/revise GoalPlan;
- decompose tasks;
- determine parallelizable work;
- dispatch roles;
- collect results;
- detect contradictions;
- request cross-review;
- trigger repair loops;
- decide whether evidence satisfies gates;
- publish final result.

Repair loops are bounded by configurable attempt, time, and cost budgets.

### 5.5 Agent Fleet

V1 roles:

Architect:
- inspect repository structure and conventions;
- propose interfaces and decomposition;
- identify architectural risks.

Coder:
- implement scoped changes;
- preserve project conventions;
- keep changes minimal to task requirements.

Debugger:
- reproduce failures;
- isolate root cause;
- produce targeted fixes.

Tester:
- define regression/behavior tests;
- run tests and report exact evidence.

Reviewer:
- review code independently from implementation context;
- flag correctness, maintainability, concurrency, portability, and regression risks.

Security:
- inspect dependency, secret, command, permission, injection, path traversal, unsafe shell, supply-chain, and credential risks.

Release:
- validate versioning/changelog/artifacts;
- prepare release actions after verification gates.

Roles are templates. Future versions can register custom roles dynamically.

### 5.6 Multi-Model Router

Providers implement a common adapter.

Required provider classes for v1:
- OpenAI API / Codex-compatible CLI or API bridge;
- Anthropic API / Claude Code CLI bridge;
- Google Gemini API / Gemini CLI bridge;
- OpenCode CLI bridge;
- generic OpenAI-compatible HTTP provider for local servers such as Ollama/vLLM/LM Studio-compatible endpoints where supported.

Routing inputs:
- agent role;
- task complexity;
- required context window;
- tool-use needs;
- provider health;
- observed success rate;
- latency;
- price budget;
- user routing profile (`best_quality`, `balanced`, `budget`, custom);
- privacy constraints.

Router behavior:
- score candidate models;
- select primary;
- fallback on retryable failure;
- maintain provider circuit breakers;
- avoid repeated selection of unhealthy models;
- record cost/latency/outcome telemetry locally;
- allow role-specific pins and deny lists.

### 5.7 Runtime abstraction

`RuntimeAdapter` exposes:
- probe availability;
- execute command;
- stream stdout/stderr;
- cancel;
- resolve environment metadata;
- optional snapshot/cleanup.

V1 implementations:
- Native Windows using PowerShell 7 when available, fallback to Windows PowerShell where explicitly supported;
- WSL2 using `wsl.exe`;
- Docker using Docker Desktop/Engine CLI;
- Mock runtime for deterministic tests.

Runtime selection considers repository metadata and task policy.

### 5.8 Workspace and Git isolation

`WorkspaceManager` owns:
- opening local repos;
- repository fingerprint;
- working-tree cleanliness checks;
- worktree creation/removal;
- branch naming;
- safe merge/rebase strategy;
- protected branch detection;
- diff collection;
- rollback checkpoints.

Parallel write tasks must not share the same mutable checkout.

### 5.9 Policy Engine

The policy engine is mandatory even in Full Autonomy.

Capabilities include:
- read workspace;
- write workspace;
- execute process;
- network access;
- git commit;
- git push;
- create PR;
- modify CI;
- package/install dependencies;
- access secrets;
- Docker execution;
- WSL execution;
- delete files;
- destructive git operations;
- release publication;
- future browser/computer-use actions.

Rules are evaluated from:
- global policy;
- project policy;
- goal policy;
- task/role capability scope;
- risk classifier.

Hard-deny examples by default:
- reading arbitrary credential stores outside configured scopes;
- force-push to protected branches;
- deletion outside workspace/sandbox;
- commands matching configurable catastrophic patterns;
- silent exfiltration of files/secrets to model providers.

Full Autonomy means no unnecessary confirmation dialogs, not absence of controls.

### 5.10 Memory

V1 memory tiers:
- Run memory: current goal/task state.
- Project memory: conventions, commands, architecture facts, previous failures and verified resolutions.
- User preferences: routing profile, runtimes, policy choices.

Memory writes should be explicit structured records, not a raw unbounded transcript dump.

SQLite stores metadata. Large logs/artifacts live in application data directories with database references.

### 5.11 Audit and observability

Every important action becomes an AuditEvent with:
- id;
- run/goal/task ids;
- timestamp;
- actor;
- event type;
- risk class;
- inputs summary;
- outputs summary;
- command/model/runtime metadata;
- status;
- parent correlation id.

The audit log supports replay of state transitions but not automatic replay of destructive side effects.

Metrics include:
- tokens/cost per model and goal;
- task latency;
- retry count;
- verification pass/fail;
- model/provider success rates;
- runtime failures;
- repair-loop count.

### 5.12 Emergency control

The app exposes an immediate STOP button.

Emergency stop must:
- cancel queued tasks;
- signal running processes;
- terminate known process trees where safe;
- stop dispatching models/tools;
- persist state as interrupted;
- preserve logs/worktrees for inspection.

## 6. Tooling and extension layer

### 6.1 Tool registry

All tools are normalized behind typed capability declarations. A tool declares:
- name/version;
- input schema;
- output schema;
- capabilities;
- risk class;
- runtime requirements.

### 6.2 MCP

MCP support is an extension boundary, not the internal architecture. MCP tools are imported into the Tool Registry and remain subject to local policy.

### 6.3 Skills

Skills are versioned instruction packages containing role guidance, workflow logic, references, and optional tool requirements. Skills cannot bypass policy.

### 6.4 Future Windows Computer Use

The v1 interfaces reserve future adapters for:
- browser automation;
- Windows UI Automation;
- screen/vision actions;
- office applications;
- local desktop workflows.

These will appear as policy-governed tools rather than special cases in the Director.

## 7. Persistence

Use SQLite with migrations.

Initial entities:
- goals;
- goal_plans;
- agent_tasks;
- agent_runs;
- evidence;
- audit_events;
- provider_profiles;
- model_stats;
- project_memory;
- projects;
- policies;
- tool_registry;
- runtime_profiles.

Secrets are never stored as plaintext in SQLite. On Windows use Windows Credential Manager / DPAPI-backed storage through a dedicated secret adapter.

## 8. Repository layout

```text
/
  apps/
    desktop/                 # Tauri + React desktop application
  crates/
    autopilot-core/          # orchestration facade
    autopilot-domain/        # stable domain types/state
    autopilot-router/        # model routing
    autopilot-runtime/       # execution adapters
    autopilot-policy/        # policy engine
    autopilot-audit/         # audit events/store interfaces
  packages/
    protocol/                # TypeScript shared DTO/schema package
  tests/
    fixtures/
  docs/
    architecture/
    superpowers/specs/
    superpowers/plans/
  .github/workflows/
```

V0.1 may begin with fewer crates, but public boundaries above should be preserved so later extraction is mechanical.

## 9. V0.1 implementation scope

V0.1 is a vertical slice proving the architecture, not a fake UI.

Required:
- Rust workspace;
- stable domain types for Goal, Task, Role, State, Evidence, AuditEvent;
- deterministic Director state machine;
- Multi-Model Router scoring/fallback core independent of provider SDKs;
- runtime abstraction with native process implementation and test fake;
- policy engine with capability and risk decisions;
- append-only in-memory audit store plus interfaces for SQLite implementation;
- small CLI harness that can execute a sample autonomous run locally;
- React/Tauri shell can follow after core vertical slice, but repository structure must anticipate it;
- unit and integration tests for all deterministic logic;
- GitHub Actions for formatting, clippy, and tests;
- README describing product and current status;
- third-party notices document.

V0.1 explicitly does not claim full production-ready integration with every model provider or GUI automation. Provider adapters are added behind proven interfaces in subsequent milestones.

## 10. Testing strategy

Test deterministic code without LLM calls.

Unit tests:
- state transition legality;
- policy decisions;
- routing score/order/fallback;
- budget and retry bounds;
- audit append behavior;
- runtime command result mapping.

Integration tests:
- sample goal lifecycle using fake agents/model provider and fake runtime;
- repair cycle after failed verification;
- policy denial halts execution;
- router fallback after simulated provider failure;
- emergency stop.

Contract tests later validate real provider/runtime adapters.

## 11. Security requirements

- No arbitrary shell string concatenation in trusted code where argument arrays can be used.
- Normalize and validate workspace paths before file/destructive operations.
- Do not pass unrelated files or environment variables to providers.
- Redact secrets in audit events.
- Separate provider API secrets from agent-visible context.
- Use explicit capability scopes for MCP/tools.
- No automatic force push to protected branches.
- No automatic release publication unless project policy opts in.
- Dependency additions must be visible in audit and diff.
- Future auto-update mechanism must verify signed artifacts.

## 12. Licensing and provenance

OPEN AUTOPILOT is an independent codebase. Any copied or adapted third-party code must be license-compatible, attributable, and listed in `THIRD_PARTY_NOTICES.md`. Architectural inspiration alone does not require code copying. Prefer clean-room implementation of interfaces and orchestration behavior.

## 13. Milestones

M0 — Core vertical slice:
- domain, Director state machine, policy, audit, router, runtime abstraction, tests, CI.

M1 — Real coding loop:
- filesystem/repository context, Git/worktrees, test/build verification, OpenCode/Codex/Claude/Gemini adapters, provider health and fallback.

M2 — Windows desktop:
- Tauri 2 + React Command Center, live fleet, terminal streaming, diffs, models, policies, audit.

M3 — Full GitHub autopilot:
- branches, commits, PRs, CI observation, repair loop, release preparation.

M4 — Hybrid workers:
- remote worker protocol, encrypted job envelopes, capability-scoped execution, resume across devices.

M5 — Computer Use:
- browser and Windows UI automation as policy-governed tools.

## 14. V0.1 acceptance criteria

V0.1 is accepted only when:

1. `cargo test --workspace` passes.
2. `cargo clippy --workspace --all-targets -- -D warnings` passes.
3. `cargo fmt --all -- --check` passes.
4. Director state transitions reject illegal transitions.
5. Router chooses the highest-scoring healthy candidate and falls back deterministically.
6. Policy Engine can allow, deny, or require an approval boundary by capability/risk rule.
7. Audit Store records ordered events with correlation ids.
8. Runtime interface can execute a harmless command and return structured output.
9. Integration test demonstrates Goal -> Planning -> Execution -> Verification -> Completion using fake model/agent behavior.
10. Integration test demonstrates a failed verification entering a bounded repair loop.
11. Integration test demonstrates policy denial stopping the run.
12. README accurately distinguishes implemented features from roadmap features.
13. `main` is untouched until a reviewed PR is ready.
