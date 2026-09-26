# OPEN AUTOPILOT Multi-Agent Fleet

## Execution graph

The current fleet engine executes the following goal graph:

```text
Human Goal
   |
   v
Architect (read/plan)
   |
   v
Coder (managed Git worktree, write)
   |
   +----------------+----------------+
   |                |                |
   v                v                v
Tester           Reviewer         Security
(read-only intent) (cross-review) (security review)
   \                |               /
    +---------------+--------------+
                    |
                    v
          Acceptance Verification
                    |
            pass ---+--- fail
                    |      |
                    v      v
                Complete  Debugger (write)
                           |
                           v
                    Verify again
                    |
              bounded repair budget
```

`FleetScheduler` enforces single-writer / multi-reader scheduling semantics for a workspace: a writer is exclusive, while multiple readers may execute together after the writer has completed.

## Model routing

Every specialist role is routed independently through `ModelRouter`. A candidate contains provider, model id, quality, cost, latency, health and role-affinity signals. Routing profiles are:

- `best-quality`
- `balanced`
- `budget`

Provider mapping is explicit:

- `opencode` -> OpenCode CLI
- `openai` or `codex` -> Codex CLI
- `anthropic`, `claude`, `claude-code` -> Claude Code CLI
- `google` or `gemini` -> Gemini CLI

Unknown providers are rejected; the fleet engine never silently substitutes a backend.

An empty model id means “use the selected backend's own default model”. The Harness omits the model flag entirely in this case.

## Managed worktree

The Coder and Debugger operate on a goal-specific Git worktree created under the configured `worktree_root`. The root defaults to:

```text
<repo>/.open-autopilot/worktrees
```

The Workspace Manager normalizes branch fragments and rejects worktree deletion outside its managed root.

The worktree is intentionally preserved after the run so a human or subsequent Git/PR stage can inspect, review and publish it. Automatic merging/pushing is not part of this milestone.

## CLI

Use a single provider for all roles:

```bash
cargo run -p open-autopilot -- fleet \
  --goal "Implement the requested feature and verify it" \
  --repo "C:\\dev\\project" \
  --provider opencode \
  --verify-program cargo \
  --verify-arg test \
  --verify-arg --workspace
```

Use different providers/models per role:

```bash
cargo run -p open-autopilot -- fleet \
  --goal "Implement the feature, cross-review it and repair failures" \
  --repo "C:\\dev\\project" \
  --provider opencode \
  --route "architect=anthropic:ARCHITECT_MODEL" \
  --route "coder=openai:CODER_MODEL" \
  --route "tester=google:TEST_MODEL" \
  --route "reviewer=anthropic:REVIEW_MODEL" \
  --route "security=anthropic:SECURITY_MODEL" \
  --route "debugger=openai:DEBUG_MODEL" \
  --profile best-quality \
  --allow-env OPENAI_API_KEY \
  --allow-env ANTHROPIC_API_KEY \
  --verify-program cargo \
  --verify-arg test \
  --verify-arg --workspace
```

Only explicitly allowlisted environment variables are projected into agent subprocesses in addition to the minimal base system environment.

## Windows Command Center

The Tauri host exports `run_fleet`. The React Command Center exposes `FULL FLEET` and `SINGLE AGENT` modes.

Full Fleet fields include:

- repository;
- human goal;
- default provider/model;
- role routing overrides using `role=provider[:model]`;
- routing profile;
- base ref;
- explicit environment allowlist;
- timeout;
- repair budget;
- acceptance program and arguments.

The result surface shows final goal state, repair count, managed worktree, every routed role/provider/model/backend result and verification evidence.

Emergency STOP uses the same shared `NativeRuntime` cancellation signal as direct agent runs.

## Current security boundary

Current guarantees:

- local-first process execution;
- minimal environment inheritance plus explicit allowlist;
- no automatic force-push or release publication;
- managed Git worktree containment;
- explicit provider/backend mapping;
- command arguments are passed as arrays rather than interpolated shell strings;
- bounded process timeout and shared cancellation;
- independent reviewer/security/tester roles;
- bounded repair loop.

Current limitation: role prompts instruct Architect/Tester/Reviewer/Security not to edit files, but OS-level read-only filesystem sandboxing for those roles is not yet implemented. WSL2/Docker/read-only sandbox adapters are a subsequent milestone and are required before treating read-only role semantics as a hard containment guarantee.

## Verification status

The repository defines root Rust checks with formatting, clippy and workspace tests. GitHub Actions is currently terminating jobs before any workflow steps are assigned on both Windows and Ubuntu, so this branch does not claim an executable green CI result yet.
