# OPEN AUTOPILOT M4 Persistence, Recovery & Computer Use Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make OPEN AUTOPILOT durable across process/Windows restarts with SQLite-backed missions, checkpoints, memory and audit, then expose policy-governed Windows UI Automation and browser actions as typed tools.

**Architecture:** Add a dedicated `autopilot-store` crate that owns SQLite schema/migrations and durable mission/checkpoint/memory records while implementing the existing `AuditStore` interface. Add `autopilot-missions` for leasing/resume semantics so queued/running missions survive crashes without double execution. Add `autopilot-computer-use` with typed adapters; Windows desktop actions use the Microsoft `winapp ui` CLI through `RuntimeAdapter`, while browser actions use a small Playwright bridge process and remain capability-scoped. Tauri/CLI expose durable queue controls, but orchestration remains in Rust.

**Tech Stack:** Rust 2021, Tokio, rusqlite with bundled SQLite, serde/serde_json, UUID/chrono, existing RuntimeAdapter/PolicyEngine, Microsoft `winapp ui`, Node.js Playwright bridge for browser automation, Tauri 2 + React.

**Spec:** `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`

## Global Constraints

- Local-first: mission state, audit, memory, checkpoints and secrets metadata remain local by default.
- SQLite stores no provider secret values.
- Mission execution is lease-based; a crashed/expired lease can be recovered, but a live lease cannot be stolen.
- Mission completion requires terminal evidence; an interrupted mission resumes from its durable phase/checkpoint rather than being silently marked complete.
- Audit events remain append-only.
- Computer Use requires `Capability::ComputerUse`; browser network actions also require `Capability::NetworkAccess`.
- UIA-pattern operations are preferred. Raw/system keyboard injection and global OS hotkeys are not implemented in M4.
- Computer-use adapters receive explicit target selectors; no arbitrary shell strings are constructed.
- Browser automation executes only through the packaged bridge with structured JSON requests.
- `main` remains untouched until verified PR review.

---

### Task 1: Durable domain records

**Files:**
- Modify: `crates/autopilot-domain/src/lib.rs`
- Create: `crates/autopilot-domain/tests/missions.rs`

**Interfaces:**
- Produces: `MissionId`, `MissionState`, `MissionRecord`, `CheckpointRecord`, `MemoryRecord`.
- Mission states: `Queued`, `Leased`, `Running`, `Waiting`, `Completed`, `Failed`, `Cancelled`.

- [ ] Write tests for terminal states and legal mission transitions.
- [ ] Add UUID-backed `MissionId`.
- [ ] Add serializable mission/checkpoint/memory records with timestamps and attempt counters.
- [ ] Add transition validation so terminal missions cannot re-enter execution.

### Task 2: SQLite store and migrations

**Files:**
- Create: `crates/autopilot-store/Cargo.toml`
- Create: `crates/autopilot-store/src/lib.rs`
- Create: `crates/autopilot-store/tests/store.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces: `SqliteStore::open(path)`, `SqliteStore::open_in_memory()`, `migrate()`.
- Tables: `schema_migrations`, `audit_events`, `missions`, `mission_events`, `checkpoints`, `memory_records`.
- Implements existing `AuditStore`.

- [ ] Write failing tests for schema creation, audit persistence/reopen, mission persistence, checkpoint replacement and memory query.
- [ ] Add rusqlite bundled dependency and store crate.
- [ ] Use WAL mode for file-backed DB and foreign keys on.
- [ ] Serialize structured payloads as JSON while indexing ids/state/project/timestamps in relational columns.
- [ ] Implement append-only audit insertion and correlation lookup ordered by timestamp/row id.
- [ ] Ensure no API accepts or persists secret values.

### Task 3: Durable Mission Queue with leases

**Files:**
- Create: `crates/autopilot-missions/Cargo.toml`
- Create: `crates/autopilot-missions/src/lib.rs`
- Create: `crates/autopilot-missions/tests/queue.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces: `MissionQueue`, `MissionLease`, `EnqueueMission`, `RecoveryDecision`.
- Methods: `enqueue`, `lease_next`, `heartbeat`, `checkpoint`, `complete`, `fail`, `release`, `recover_expired`.

- [ ] Write tests proving FIFO queueing, exclusive live lease, lease expiry recovery and terminal immutability.
- [ ] Implement atomic lease acquisition in a SQLite transaction.
- [ ] Store `lease_owner`, `lease_until`, `attempt`, and durable phase.
- [ ] Heartbeat extends only the matching live lease.
- [ ] Recovery moves expired `Leased/Running` missions back to `Queued` while preserving checkpoint and incremented attempt history.

### Task 4: Checkpoint/resume orchestration contract

**Files:**
- Create: `crates/autopilot-recovery/Cargo.toml`
- Create: `crates/autopilot-recovery/src/lib.rs`
- Create: `crates/autopilot-recovery/tests/recovery.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces: `MissionCheckpoint { phase, worktree, correlation_id, repairs, ci_repairs, pr_url, metadata }` and `RecoveryManager`.
- Recovery phases: `Planned`, `WorktreeReady`, `FleetVerified`, `Published`, `WaitingCi`, `RepairingCi`.

- [ ] Write tests for save/load and restart recovery decisions.
- [ ] Persist checkpoint after each irreversible/high-value phase.
- [ ] On restart, verify referenced worktree exists before resuming; otherwise fail the mission with explicit evidence.
- [ ] Never replay commit/push blindly: `Published/WaitingCi` resumes by reading existing branch/PR state.

### Task 5: Persistent audit and project memory

**Files:**
- Modify: `crates/autopilot-store/src/lib.rs`
- Create: `crates/autopilot-store/tests/memory.rs`

**Interfaces:**
- Produces: `put_memory`, `search_memory`, `delete_project_memory`, scoped by project/key/category.

- [ ] Write tests for upsert, project isolation and recency ordering.
- [ ] Store concise structured project facts/decisions, not raw secret-bearing transcripts.
- [ ] Add optional TTL/expires_at and ignore expired memory in search.

### Task 6: Windows UI Automation adapter

**Files:**
- Create: `crates/autopilot-computer-use/Cargo.toml`
- Create: `crates/autopilot-computer-use/src/lib.rs`
- Create: `crates/autopilot-computer-use/tests/winapp.rs`
- Modify: root `Cargo.toml`

**Interfaces:**
- Produces: `ComputerUseAction`, `WindowsTarget`, `WinAppAdapter`.
- Initial actions: `Inspect`, `Search`, `GetValue`, `SetValue`, `Invoke`, `WaitFor`, `Screenshot`.

- [ ] Write command-construction tests for each action.
- [ ] Build only `winapp ui ...` argv arrays; never shell-concatenate selectors or values.
- [ ] Target by app/process, hwnd, or explicit selector slug.
- [ ] Classify `Inspect/Search/GetValue/Screenshot` as read operations and `SetValue/Invoke` as write computer-use operations.
- [ ] Omit raw `send-keys`, global hotkeys, drag/touch/pen from M4.
- [ ] Parse JSON output when requested and return structured `ComputerUseResult`.

### Task 7: Browser automation bridge

**Files:**
- Create: `tools/browser/package.json`
- Create: `tools/browser/bridge.mjs`
- Create: `crates/autopilot-computer-use/tests/browser.rs`
- Modify: `crates/autopilot-computer-use/src/lib.rs`

**Interfaces:**
- Produces: `BrowserAction::{Open, Navigate, Snapshot, Click, Fill, WaitFor, Close}` and `PlaywrightAdapter`.
- Rust sends one JSON request to `node bridge.mjs`; bridge emits one JSON response.

- [ ] Add deterministic Rust tests for JSON request construction without launching a browser.
- [ ] Implement bridge validation for allowed action enum and required fields.
- [ ] Use Playwright locators/selectors; do not evaluate arbitrary JavaScript from mission text.
- [ ] Add timeout and optional persistent profile directory owned by OPEN AUTOPILOT.
- [ ] Treat Navigate/Open as network-capable actions requiring policy permission.

### Task 8: Mission runner and CLI queue controls

**Files:**
- Modify: `apps/autopilot-cli/Cargo.toml`
- Modify: `apps/autopilot-cli/src/main.rs`

**Interfaces:**
- Commands: `mission enqueue`, `mission list`, `mission run-next`, `mission recover`, `mission show`.
- Uses `%LOCALAPPDATA%/OpenAutopilot/state.db` on Windows unless `--db` overrides.

- [ ] Add command parsing and durable DB initialization.
- [ ] `run-next` acquires lease before execution and checkpoints phases.
- [ ] Recovery command requeues only expired missions.
- [ ] Display mission id/state/attempt/phase without exposing secret values.

### Task 9: Windows Command Center Mission Queue

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/styles.css`

**Interfaces:**
- Tauri commands: `mission_enqueue`, `mission_list`, `mission_recover`, `mission_cancel`.
- UI adds durable queue cards and resume/recovery indicators.

- [ ] Initialize one local SQLite store in Tauri AppState.
- [ ] Add Mission Queue navigation/status view.
- [ ] Show queued/running/waiting/completed/failed counts.
- [ ] Show checkpoint phase and attempt count.
- [ ] Preserve shared STOP behavior; STOP cancels execution but does not erase durable mission/checkpoint records.

### Task 10: CI and documentation

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Create: `docs/architecture/persistence-recovery.md`
- Create: `docs/architecture/computer-use.md`

**Interfaces:**
- CI adds store/recovery unit tests as part of workspace and validates browser bridge syntax/dependency install separately.

- [ ] Document SQLite location/schema ownership and crash recovery semantics.
- [ ] Document UI Automation safety model and Microsoft `winapp` prerequisite.
- [ ] Document browser bridge prerequisite and policy scopes.
- [ ] Keep PR draft unless executable CI evidence exists.

### Task 11: Verification

- [ ] Inspect `cargo fmt --all -- --check` workflow evidence.
- [ ] Inspect `cargo clippy --workspace --all-targets -- -D warnings` workflow evidence.
- [ ] Inspect `cargo test --workspace` workflow evidence.
- [ ] Inspect desktop renderer build evidence.
- [ ] Inspect browser bridge install/syntax evidence.
- [ ] Update PR with exact implemented scope and remaining limitations.
