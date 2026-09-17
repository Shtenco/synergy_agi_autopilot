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
