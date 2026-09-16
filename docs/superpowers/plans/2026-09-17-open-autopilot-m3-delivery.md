# OPEN AUTOPILOT M3 Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add hard read-only execution profiles, WSL2/Docker runtimes, and an automated Git delivery loop that can commit, push, open a PR, inspect CI, dispatch Debugger repairs, and republish safely.

**Architecture:** Extend the existing typed runtime boundary instead of adding shell-string special cases. Sandboxing is represented as explicit execution profiles consumed by runtime adapters; Git publishing is isolated behind a new delivery crate that uses typed git/gh command specifications and structured CI states. FleetEngine remains the orchestrator and calls the delivery layer only after verification evidence passes.

**Tech Stack:** Rust 2021, Tokio, existing `autopilot-runtime`, `autopilot-engine`, `autopilot-workspace`, `autopilot-policy`, Git CLI, GitHub CLI (`gh`) adapter, WSL2 (`wsl.exe`), Docker CLI, Tauri 2/React.

**Spec:** `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`

## Global Constraints

- Full Autonomy remains policy-governed; force-push and release publication remain denied by default.
- No production command is constructed by concatenating untrusted shell strings; use executable + argument vectors.
- Secrets remain environment-allowlisted and are not copied into audit/UI payloads.
- Non-writer agent roles must run with a hard read-only workspace profile when the selected runtime can enforce it.
- Native Windows cannot honestly claim filesystem sandboxing until an OS-backed sandbox implementation exists; Native read-only requests therefore fail closed rather than silently degrade.
- WSL2 read-only execution uses a copied/read-only projection strategy only when explicitly configured; Docker read-only execution uses a read-only bind mount plus tmpfs scratch.
- Git publishing may create only `autopilot/*` branches by default and never force-push.
- CI repair loops are bounded by `max_ci_repairs` and preserve evidence for each iteration.
- `main` is never directly modified by autonomous delivery.

---

### Task 1: Typed execution profiles and fail-closed sandbox contract

**Files:**
- Modify: `crates/autopilot-runtime/src/lib.rs`
- Modify: `crates/autopilot-runtime/tests/runtime.rs`
- Modify: `crates/autopilot-engine/src/lib.rs`
- Test: `crates/autopilot-runtime/tests/runtime.rs`

**Interfaces:**
- Produces: `ExecutionProfile::{Host, ReadOnlyWorkspace, WorkspaceWrite}` and `CommandSpec::profile(ExecutionProfile)`.
- Runtime adapters return `RuntimeError::UnsupportedIsolation` when a requested profile cannot be enforced.
- FleetEngine uses `ReadOnlyWorkspace` for Architect/Tester/Reviewer/Security and `WorkspaceWrite` for Coder/Debugger.

- [ ] Write failing tests that native runtime rejects `ReadOnlyWorkspace` instead of silently executing unsandboxed and that fake runtime preserves the requested profile.
- [ ] Add `ExecutionProfile` to `CommandSpec` with `Host` as the default.
- [ ] Add `UnsupportedIsolation { runtime, profile }` error.
- [ ] Make NativeRuntime reject hard read-only requests and continue to support `Host`/`WorkspaceWrite` execution.
- [ ] Propagate profiles through `RunSpec`/AgentHarness and FleetEngine role execution.
- [ ] Verify runtime and engine tests.

### Task 2: WSL2 and Docker runtime adapters

**Files:**
- Modify: `crates/autopilot-runtime/src/lib.rs`
- Create: `crates/autopilot-runtime/tests/isolation_adapters.rs`

**Interfaces:**
- Produces: `WslRuntime { distribution: Option<String> }` and `DockerRuntime { image: String, workspace_host: PathBuf, workspace_container: String }` implementing `RuntimeAdapter`.
- Both adapters expose `probe()` by invoking their respective CLI version/status commands through NativeRuntime.

- [ ] Write failing command-construction tests for WSL2 and Docker.
- [ ] Implement WSL argument translation with explicit `--distribution` and `--exec` argument vectors.
- [ ] Implement Docker command construction using `docker run --rm`, a read-only or read-write bind depending on profile, `--network none` for read-only review by default, and `--tmpfs /tmp:rw,noexec,nosuid,size=256m`.
- [ ] Add runtime cancellation propagation through the shared NativeRuntime instance.
- [ ] Add probe tests that use a recording runtime abstraction for deterministic behavior.

### Task 3: Runtime selection policy for Fleet roles

**Files:**
- Modify: `crates/autopilot-engine/src/lib.rs`
- Modify: `crates/autopilot-engine/tests/fleet_engine.rs`

**Interfaces:**
- Produces: `FleetRuntimePolicy` mapping writer/reviewer roles to runtime targets `Native | Wsl | Docker` and execution profiles.
- FleetEngine resolves a runtime per role before invoking AgentHarness.

- [ ] Write failing tests proving reviewer/security are assigned a hard read-only profile and coder/debugger a write profile.
- [ ] Add runtime policy configuration with secure defaults preferring Docker read-only for review roles when configured.
- [ ] Reject a Full Fleet run before agent launch when a required hard sandbox is unavailable and `require_hard_read_only=true`.
- [ ] Preserve existing single-agent compatibility.

### Task 4: Typed Git delivery layer

**Files:**
- Create: `crates/autopilot-delivery/Cargo.toml`
- Create: `crates/autopilot-delivery/src/lib.rs`
- Create: `crates/autopilot-delivery/tests/delivery.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces: `DeliveryEngine`, `DeliveryConfig`, `DeliveryResult`, `CiState`, `PullRequestRef`.
- Methods: `prepare_commit`, `push_branch`, `ensure_pr`, `read_ci`, `publish_verified_change`.
- Uses `RuntimeAdapter`; never uses shell strings.

- [ ] Write tests for safe branch validation, `git add --all`, commit creation, non-force push, `gh pr create`, and CI state parsing.
- [ ] Enforce `autopilot/` branch prefix and reject `main`, `master`, empty, or ref/path escape values.
- [ ] Build typed git commands with explicit working directory.
- [ ] Build typed `gh pr create --base <base> --head <branch> --title <title> --body <body>` command.
- [ ] Implement `gh pr checks --json name,state,bucket,link` parsing using serde_json.
- [ ] Return `Passing`, `Failing`, `Pending`, or `Unknown` without guessing success.

### Task 5: CI repair loop in FleetEngine

**Files:**
- Modify: `crates/autopilot-engine/Cargo.toml`
- Modify: `crates/autopilot-engine/src/lib.rs`
- Modify: `crates/autopilot-engine/tests/fleet_engine.rs`

**Interfaces:**
- Fleet config adds optional `DeliveryConfig` and `max_ci_repairs`.
- Result adds `delivery: Option<DeliveryResult>` and CI evidence.

- [ ] Write failing tests for verified run -> commit/push/PR -> failing CI -> Debugger -> verify -> recommit/push -> passing CI.
- [ ] Call DeliveryEngine only after local verification passes.
- [ ] When CI fails, create a Debugger prompt containing only structured failing check names/links and local context, not secrets.
- [ ] Re-run local verification after Debugger.
- [ ] Create a new normal commit and push without force.
- [ ] Stop with Failed when CI remains failing after `max_ci_repairs`.
- [ ] Mark Completed only after local verification and CI are both passing when delivery is enabled.

### Task 6: CLI and Windows Command Center delivery controls

**Files:**
- Modify: `apps/autopilot-cli/Cargo.toml`
- Modify: `apps/autopilot-cli/src/main.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

**Interfaces:**
- CLI `fleet` adds `--runtime`, `--review-runtime`, `--docker-image`, `--wsl-distro`, `--publish`, `--remote`, `--base`, `--max-ci-repairs`.
- Tauri `run_fleet` request mirrors these fields; renderer displays PR/CI state.

- [ ] Add argument/request parsing without transmitting secret values to the renderer.
- [ ] Add explicit publish opt-in; Full Autonomy may perform push/PR only when project/user policy enables it.
- [ ] Add UI badges for runtime isolation, PR URL, CI state, and CI repair count.
- [ ] Keep Emergency STOP wired to all local child runtimes.

### Task 7: CI workflow and documentation

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Create: `docs/architecture/delivery-loop.md`

**Interfaces:**
- Core CI remains Rust-only and cross-platform; a separate desktop job installs Node and runs `npm ci && npm run build` without requiring release signing.

- [ ] Keep core format/clippy/test matrix for Windows + Ubuntu.
- [ ] Add static desktop TypeScript/Vite build job on Windows.
- [ ] Document runtime isolation guarantees and explicit limitations.
- [ ] Document `fleet --publish` end-to-end flow and required `gh auth` state.
- [ ] Document that CI repair is bounded and force-push is never used.

### Task 8: Verification and handoff

**Files:** no new production files.

- [ ] Run/inspect `cargo fmt --all -- --check` evidence.
- [ ] Run/inspect `cargo clippy --workspace --all-targets -- -D warnings` evidence.
- [ ] Run/inspect `cargo test --workspace` evidence.
- [ ] Run/inspect desktop `npm ci && npm run build` evidence.
- [ ] Inspect GitHub Actions jobs and logs.
- [ ] Keep PR draft if executable verification cannot be obtained.
- [ ] Update PR body with exact implemented scope, limitations, and verification evidence.
