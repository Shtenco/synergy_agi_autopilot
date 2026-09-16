# OPEN AUTOPILOT M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans task-by-task.

**Goal:** Extend the M0 deterministic core into a practical local coding-agent harness with explicit CLI backends, isolated Git workspaces, reusable verification pipelines, and dependency-aware fleet scheduling.

**Architecture:** M1 remains local-first and provider-neutral. External coding agents are treated as replaceable executables behind a typed `RunSpec -> RunResult` contract; Git/worktree operations use the existing runtime abstraction; verification is evidence-producing deterministic code; fleet scheduling works on a dependency graph and never lets parallel writers share a checkout.

**Tech Stack:** Rust stable, existing workspace crates, async-trait, tokio, serde where needed.

**Spec:** `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`

## Global constraints

- No copied third-party source code.
- Explicit backend selection; no silent backend escalation.
- No permission bypass flags by default.
- All subprocesses are launched through argument arrays.
- Working directories are explicit.
- Git destructive operations are absent from M1 defaults.
- Verification returns structured evidence.
- Parallel write tasks require distinct workspaces/worktrees.

---

### Task 1: Agent Harness

Create `crates/autopilot-harness` with:
- `BackendKind` for OpenCode, Codex, Claude, Gemini, Custom;
- `RunSpec`, `RunResult`, `Termination`;
- `CliBackendSpec` command builder;
- builtin backend registry;
- `AgentHarness` that executes via `RuntimeAdapter`;
- tests for explicit selection, command construction, model projection, non-zero exit mapping, and unknown custom backend rejection.

### Task 2: Git Workspace Manager

Create `crates/autopilot-workspace` with:
- repository status probe;
- current branch detection;
- safe worktree creation/removal;
- diff collection;
- branch-name normalization;
- path containment checks;
- tests using a fake runtime command recorder.

### Task 3: Verification Pipeline

Create `crates/autopilot-verification` with:
- named verification checks;
- command-based checks using `RuntimeAdapter`;
- sequential pipeline execution;
- fail-fast configurable behavior;
- evidence conversion;
- tests for pass, failure, fail-fast, and full collection.

### Task 4: Fleet Scheduler

Create `crates/autopilot-fleet` with:
- `FleetTask` containing id, role, dependencies, write intent and workspace id;
- validation for unknown dependencies and cycles;
- deterministic ready-task selection;
- conflict prevention for simultaneous writers in the same workspace;
- tests for DAG ordering, cycles, and workspace write conflicts.

### Task 5: CLI integration

Extend `open-autopilot` CLI with:
- `backends` command listing builtin CLI agents;
- `agent --backend ... --prompt ... --workdir ... [--model ...]` command;
- no implicit privilege bypass.

### Task 6: Documentation and review

Update README architecture/status and third-party notices wording. Keep the PR draft until verification is available and no merge is performed automatically.
