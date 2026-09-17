# OPEN AUTOPILOT v0.1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a tested Rust core vertical slice for OPEN AUTOPILOT proving autonomous orchestration, policy control, model routing, audit logging, runtime execution, bounded repair, and CI.

**Architecture:** A Rust workspace separates stable domain types from orchestration, routing, policy, audit, and runtime concerns. The deterministic core is provider-neutral and testable without network or LLM calls. A minimal CLI demonstrates the autonomous lifecycle using fake agents and the real native runtime interface.

**Tech Stack:** Rust stable, Cargo workspace, serde, thiserror, async-trait, tokio, uuid, chrono, tracing, clap; GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`

## Global Constraints

- Windows-first and hybrid local-first.
- Full Autonomy is the primary mode but every action remains policy-governed and audited.
- Provider-neutral core; no vendor SDK in M0.
- Runtime-neutral core; native process execution plus deterministic fake runtime in M0.
- No secrets in source, logs, tests, or examples.
- No automatic destructive git operations.
- Production behavior follows test-first development.
- `main` stays untouched until review.

---

### Task 1: Rust workspace and domain model

**Files:**
- Create: `Cargo.toml`
- Create: `crates/autopilot-domain/Cargo.toml`
- Create: `crates/autopilot-domain/src/lib.rs`
- Create: `crates/autopilot-domain/tests/state_transitions.rs`

**Interfaces:**
- Produces `GoalId`, `TaskId`, `RunId`, `CorrelationId`, `AgentRole`, `GoalState`, `Capability`, `RiskClass`, `Evidence`, `AuditEvent`.
- Produces `GoalState::can_transition_to(&self, next: &GoalState) -> bool`.

- [ ] Write failing transition tests covering legal lifecycle, illegal backwards transition, terminal-state immutability, repair path, cancellation.
- [ ] Run `cargo test -p autopilot-domain` and verify failure due to missing API.
- [ ] Implement minimal serializable domain types and transition table.
- [ ] Run domain tests and formatting.
- [ ] Commit `feat(domain): add core autopilot state model`.

### Task 2: Policy engine

**Files:**
- Create: `crates/autopilot-policy/Cargo.toml`
- Create: `crates/autopilot-policy/src/lib.rs`
- Create: `crates/autopilot-policy/tests/policy.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Consumes `Capability`, `RiskClass`.
- Produces `PolicyDecision::{Allow, RequireApproval, Deny}`.
- Produces `PolicyRule { capability: Option<Capability>, min_risk: Option<RiskClass>, decision: PolicyDecision }`.
- Produces `PolicyEngine::evaluate(capability, risk) -> PolicyDecision`.

- [ ] Write failing tests for explicit deny, risk approval boundary, default allow, deny precedence.
- [ ] Verify tests fail.
- [ ] Implement ordered deterministic rule evaluation with deny precedence.
- [ ] Run package tests.
- [ ] Commit `feat(policy): add capability risk engine`.

### Task 3: Audit store

**Files:**
- Create: `crates/autopilot-audit/Cargo.toml`
- Create: `crates/autopilot-audit/src/lib.rs`
- Create: `crates/autopilot-audit/tests/audit.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Consumes `AuditEvent`.
- Produces async trait `AuditStore` with `append` and `list_by_correlation`.
- Produces `InMemoryAuditStore` preserving insertion order.

- [ ] Write failing tests for ordered append and correlation filtering.
- [ ] Verify tests fail.
- [ ] Implement thread-safe in-memory store.
- [ ] Run tests.
- [ ] Commit `feat(audit): add append-only audit abstraction`.

### Task 4: Multi-model router

**Files:**
- Create: `crates/autopilot-router/Cargo.toml`
- Create: `crates/autopilot-router/src/lib.rs`
- Create: `crates/autopilot-router/tests/router.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces `RoutingProfile::{BestQuality, Balanced, Budget}`.
- Produces `ModelCandidate { provider, model, quality, latency_ms, cost_per_million, healthy, role_affinity }`.
- Produces `ModelRouter::rank(role, profile, candidates) -> Vec<ModelCandidate>`.
- Produces `ModelRouter::select(...) -> Option<ModelCandidate>`.

- [ ] Write failing tests for unhealthy exclusion, deterministic quality selection, budget preference, role affinity, stable fallback order.
- [ ] Verify tests fail.
- [ ] Implement normalized deterministic scoring and tie-breaking by provider/model identifier.
- [ ] Run tests.
- [ ] Commit `feat(router): add deterministic multi-model routing`.

### Task 5: Runtime abstraction

**Files:**
- Create: `crates/autopilot-runtime/Cargo.toml`
- Create: `crates/autopilot-runtime/src/lib.rs`
- Create: `crates/autopilot-runtime/tests/runtime.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces `CommandSpec { program, args, cwd, env_allowlist }`.
- Produces `CommandResult { exit_code, stdout, stderr, duration_ms }`.
- Produces async trait `RuntimeAdapter` with `probe`, `execute`, `cancel`.
- Produces `NativeRuntime` and `FakeRuntime`.

- [ ] Write failing tests for fake deterministic result and native harmless command.
- [ ] Verify tests fail.
- [ ] Implement execution via argument arrays, not shell concatenation.
- [ ] Run tests on available host and guard platform-specific test expectations.
- [ ] Commit `feat(runtime): add native and fake execution adapters`.

### Task 6: Director orchestration state machine

**Files:**
- Create: `crates/autopilot-core/Cargo.toml`
- Create: `crates/autopilot-core/src/lib.rs`
- Create: `crates/autopilot-core/tests/director.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Consumes domain, policy, audit.
- Produces `GoalRequest`, `AcceptanceCriterion`, `DirectorConfig { max_repairs }`, `AgentOutcome`, `VerificationOutcome`.
- Produces traits `Planner`, `AgentExecutor`, `Verifier`.
- Produces `Director::run_goal(request) -> GoalRunSummary`.

- [ ] Write failing integration-style tests for complete happy path, one repair then success, repair-budget exhaustion, policy denial, cancellation.
- [ ] Verify tests fail.
- [ ] Implement deterministic transition loop and audit emission at each state.
- [ ] Ensure completion requires verifier evidence.
- [ ] Run tests.
- [ ] Commit `feat(core): add bounded autonomous director loop`.

### Task 7: CLI vertical slice

**Files:**
- Create: `apps/autopilot-cli/Cargo.toml`
- Create: `apps/autopilot-cli/src/main.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Consumes core public API.
- Produces CLI command `autopilot demo --goal <text>`.

- [ ] Add a failing core/CLI contract test for demo summary formatting in core where practical.
- [ ] Implement a small fake Planner/Agent/Verifier demonstration using actual Director.
- [ ] Print final state, repair count, evidence count, and audit-event count.
- [ ] Run `cargo run -p open-autopilot -- demo --goal "verify architecture"`.
- [ ] Commit `feat(cli): add autonomous demo harness`.

### Task 8: Documentation, provenance, and CI

**Files:**
- Modify: `README.md`
- Create: `THIRD_PARTY_NOTICES.md`
- Create: `.github/workflows/ci.yml`
- Create: `.gitignore`

**Interfaces:** none.

- [ ] Document only implemented M0 features as current; roadmap features clearly marked.
- [ ] Record OpenCode, OpenClaw, twaldin/harness, and majiayu000/harness as architectural references; state no source copied unless later recorded explicitly.
- [ ] CI runs `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` on Windows and Ubuntu where feasible.
- [ ] Run all checks locally where execution environment permits.
- [ ] Commit `ci: verify open autopilot core`.

### Task 9: Review gate

**Files:** no new production files unless review finds defects.

- [ ] Compare feature branch to main.
- [ ] Verify no secrets/binaries/vendor dumps.
- [ ] Verify all spec M0 acceptance criteria have test or command evidence.
- [ ] Open draft PR from `feature/open-autopilot-v0.1` to `main` with implementation status and known roadmap gaps.
- [ ] Do not merge automatically.
