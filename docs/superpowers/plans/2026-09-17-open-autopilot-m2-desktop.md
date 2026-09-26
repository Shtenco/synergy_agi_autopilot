# OPEN AUTOPILOT M2 Windows Desktop Plan

> **Goal:** Deliver a Windows-first Tauri 2 desktop shell that exposes the M0/M1 Rust core through a premium Command Center UI and can run installed coding-agent CLIs through the secure Agent Harness.

## Architecture

- `apps/desktop`: React + TypeScript + Vite renderer.
- `apps/desktop/src-tauri`: Tauri 2 native Windows host.
- Native host depends on `autopilot-harness` and `autopilot-runtime` by path.
- Renderer never receives provider secret values; it may submit environment variable names to explicitly allow for a run.
- Long-running agent execution uses async Tauri commands.
- Desktop crate is excluded from the root CI workspace until dedicated Tauri system dependencies/build jobs are added.

## Functional M2 slice

1. Command Center dashboard with Full Autonomy, local-first, runtime and policy status.
2. Goal/Agent launcher supporting backend, prompt, workdir, model, timeout and environment allowlist names.
3. Live execution state and final stdout/stderr panel.
4. Builtin backend discovery for OpenCode, Codex, Claude and Gemini.
5. Agent Fleet visualization for Director, Architect, Coder, Debugger, Tester, Reviewer, Security and Release roles.
6. Sidebar surfaces reserved for Workspace, Terminal, Verification, Models, MCP & Skills, Memory, Policy and Audit.
7. Emergency STOP control in the UI contract. M2 marks it as control-plane stop; full owned-process-tree cancellation is finalized with the runtime session subsystem.
8. No implicit privilege or permission-bypass flags.

## Files

- `apps/desktop/package.json`
- `apps/desktop/index.html`
- `apps/desktop/tsconfig.json`
- `apps/desktop/vite.config.ts`
- `apps/desktop/src/main.tsx`
- `apps/desktop/src/App.tsx`
- `apps/desktop/src/styles.css`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/src-tauri/build.rs`
- `apps/desktop/src-tauri/tauri.conf.json`
- `apps/desktop/src-tauri/src/main.rs`

## Acceptance

- Renderer type-checks under TypeScript strict mode when dependencies are installed.
- Tauri host contains explicit commands for backend listing and agent execution.
- Agent execution passes prompt as an argument, not through shell concatenation.
- Secret environment values remain in the native process environment; renderer submits only allowlisted variable names.
- UI clearly distinguishes implemented M2 features from roadmap panels.
