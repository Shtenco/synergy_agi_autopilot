# OPEN AUTOPILOT — Sandbox and Delivery Loop

## Runtime isolation

OPEN AUTOPILOT assigns execution profiles by role.

- Architect, Tester, Reviewer and Security request `ReadOnlyWorkspace`.
- Coder and Debugger request `WorkspaceWrite`.
- Native Windows deliberately rejects hard read-only execution because the current native adapter does not provide an OS-enforced filesystem sandbox.
- WSL2 currently also rejects hard read-only execution rather than silently degrading isolation.
- Docker enforces read-only review with a read-only bind mount, network disabled by default for the review profile, and a tmpfs scratch directory.

A typical Windows configuration is therefore:

```text
writer runtime: native or WSL2
review runtime: Docker
```

The selected Docker image must contain the coding-agent CLI used by the routed role. Environment values are not sent through the renderer; the UI sends only an allowlist of environment variable names.

## Emergency stop

Native, WSL2 and Docker runtimes created by the Windows application share the same `NativeRuntime` cancellation signal. The Command Center STOP action therefore cancels the active host process used by all three runtime adapters.

## Autonomous Git delivery

Publishing is explicit opt-in. A Fleet run cannot publish unless a local acceptance command is configured and the Fleet reaches `Completed` locally.

The delivery sequence is:

```text
Full Fleet
  -> local acceptance evidence
  -> resolve managed autopilot/* branch
  -> git add --all
  -> normal git commit
  -> normal git push <remote> <autopilot/* branch>
  -> reuse existing PR or gh pr create
  -> gh pr checks JSON
  -> Passing => verified delivery
  -> Pending => bounded poll
  -> Failing => Debugger repair
      -> local acceptance again
      -> normal repair commit
      -> normal push
      -> CI check again
  -> Unknown => fail closed
```

The delivery engine rejects branches outside `autopilot/*`, rejects `main` and `master`, and never uses force-push.

## CI interpretation

`gh pr checks` may return a non-zero process exit status when checks are failing or pending. OPEN AUTOPILOT therefore parses the JSON payload itself rather than treating the process exit code as the CI verdict.

CI states are normalized to:

- `Passing`
- `Failing`
- `Pending`
- `Unknown`

`Unknown` is never promoted to success.

## CLI example

```powershell
cargo run -p open-autopilot -- fleet `
  --repo C:\dev\project `
  --goal "Fix the reported bug, add regression coverage, open a PR and repair CI until green" `
  --provider openai `
  --runtime native `
  --review-runtime docker `
  --docker-image open-autopilot-agents:latest `
  --verify-program cargo `
  --verify-arg test `
  --verify-arg --workspace `
  --allow-env OPENAI_API_KEY `
  --allow-env GH_TOKEN `
  --publish `
  --max-ci-repairs 2
```

## Current limitations

- Hard read-only isolation is currently Docker-backed. Native Windows and WSL2 fail closed for that profile.
- The project does not yet ship a canonical Docker image containing all supported agent CLIs.
- CI polling is bounded; it is not an indefinitely running background monitor.
- GitHub CLI must be installed and authenticated for autonomous PR publication.
- Release publication remains denied by the default policy and is not part of this loop.
