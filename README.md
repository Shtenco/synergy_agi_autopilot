# OPEN AUTOPILOT

**OPEN AUTOPILOT** is a Windows-first, local-first autonomous AI engineering workstation under active development.

The long-term product goal is simple: give OPEN AUTOPILOT a software objective and let it carry that objective through planning, implementation, testing, independent review, repair, GitHub delivery, CI observation, and release preparation under explicit local policy controls.

This repository is an independent implementation, not a fork of OpenCode, OpenClaw, or Harness.

## Current status — M0 core vertical slice

Implemented on the `feature/open-autopilot-v0.1` line:

- deterministic autonomous goal lifecycle;
- `Autopilot Director` state machine;
- bounded verification/repair loop;
- capability + risk Policy Engine;
- append-only audit-store abstraction and in-memory implementation;
- provider-neutral Multi-Model Router scoring and deterministic fallback order;
- runtime abstraction with native process execution and deterministic fake runtime;
- specialist agent role domain model;
- CLI demonstration harness;
- cross-platform Rust CI contract for Windows and Ubuntu;
- unit/integration contracts for state transitions, routing, policy, audit, runtime, and Director behavior.

The M0 core deliberately makes **no production claim** that real Claude, Codex, Gemini, OpenCode, WSL2, Docker, GitHub PR automation, or desktop Computer Use adapters are already complete. Those are subsequent milestones behind the interfaces established here.

## Core architecture

```text
Human Goal
    |
    v
Autopilot Director
    |
    +--> Policy Engine
    +--> Agent Fleet
    |      Architect / Coder / Debugger / Tester / Reviewer / Security / Release
    |
    +--> Multi-Model Router
    |      OpenAI/Codex / Claude / Gemini / OpenCode-compatible / Local
    |
    +--> Runtime Layer
    |      Native Windows / WSL2 / Docker / Remote Worker
    |
    +--> Verification + bounded repair
    |
    +--> Audit / Memory / Evidence
    |
    v
Verified change -> Git/PR/CI -> Release preparation
```

## Full Autonomy does not mean no controls

Full Autonomy is the primary product mode. The system is designed to minimize unnecessary confirmation dialogs while still enforcing capability policies, protected operations, evidence gates, budgets, audit logs, and emergency stop behavior.

A successful goal must be supported by verification evidence. The Director cannot jump directly from execution to completion.

## Run the M0 demo

Requires stable Rust.

```bash
cargo run -p open-autopilot -- demo --goal "verify the OPEN AUTOPILOT architecture"
```

The demo uses deterministic local fake Planner/Agent/Verifier implementations to exercise the real Director lifecycle without making network or LLM calls.

## Verify

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Roadmap

- **M0 — Core vertical slice:** deterministic Director, policy, audit, routing, runtime interfaces, tests and CI.
- **M1 — Real coding loop:** repository context, Git worktrees, filesystem tools, OpenCode/Codex/Claude/Gemini adapters, build/test verification.
- **M2 — Windows desktop:** Tauri 2 + React Command Center, live Agent Fleet, terminal streams, diffs, models, policies and audit.
- **M3 — GitHub autopilot:** commits, PRs, CI monitoring and automatic repair until acceptance gates pass.
- **M4 — Hybrid workers:** capability-scoped remote workers and resumable execution.
- **M5 — Computer Use:** browser and Windows UI automation exposed as policy-governed tools.

## Engineering specification

See:

- `docs/superpowers/specs/2026-09-17-open-autopilot-design.md`
- `docs/superpowers/plans/2026-09-17-open-autopilot-v0.1.md`

## Security model

The trusted core is intended to keep secrets, durable memory, repositories, policy decisions, and audit history local by default. Tool and model adapters receive only capabilities and context required for their task. Destructive operations and future Computer Use actions remain policy-governed.

## License and provenance

See `THIRD_PARTY_NOTICES.md`. Third-party source may only be incorporated when license-compatible and explicitly attributed.
