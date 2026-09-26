# M5 — Universal Execution Fabric

Status: **IMPLEMENTATION COMPLETE / PHYSICAL GREEN GATE PENDING**

## Implemented in `integration/synergy-ai-autopilot-v1`

### Universal execution core

- `synergy.plugin.v1` manifest contract;
- Rust Plugin Registry;
- Rust Plugin Executor;
- policy + approval gate before dispatch;
- explicit environment allowlists;
- bounded local workspace for Node Commander;
- loopback-only HTTP sidecar client;
- MCP stdio JSON-RPC transport.

### Executable providers

- P7 Local Node Commander;
- SHTTPS Remote MCP gateway;
- P7 Agent Fabric worker bridge (`synergy.worker.v1`);
- NEXUS memory/inference compatibility bridge;
- Business Runtime sidecar;
- Auto-Company 2.0 sidecar adapter;
- Playwright browser capability pack.

### Goal dataflow

Plugin goals now support deterministic references to previous results:

```json
{"$ref":"steps.1.output.results.0.text"}
```

Forward/missing references fail closed.

### Browser safety

`browser.run_plan` preserves one Playwright session for multi-action plans.
Read/navigation plans can run without approval. Plans containing `click` or `fill` require explicit approval.

### First universal vertical slice

CLI command:

```text
open-autopilot e2e-sales
```

Execution graph:

```text
NEXUS memory
 → browser public research
 → Auto-Company opportunity
 → Sales qualification
 → SMM plan
 → Local Commander artifact
 → Agent Fabric lease
 → optional Remote MCP node
 → evidence verifier
 → Director completion
```

The graph passes actual outputs to subsequent steps rather than executing independent mock calls.

## Verification

GitHub Actions currently creates all jobs but they terminate before runner assignment:

- `runner_id=0`
- `steps=[]`

Therefore those failures are classified as CI infrastructure failures, not code-test failures.

A Windows physical-node run was started successfully earlier and reached Rust dependency resolution, but that node disconnected before a complete result could be captured.

Local verification is reproducible with:

```powershell
.\scripts\verify-m5.ps1
```

Full development stack:

```powershell
.\scripts\run-m5-stack.ps1 -BlockchainDir C:\src\synergy_ai_blockchain -AutoCompanyDir C:\src\synergy_ai_company
```

## Green gate

M5 becomes physically GREEN only after all of the following produce captured evidence:

1. `cargo fmt --all -- --check`;
2. `cargo clippy --workspace --all-targets -- -D warnings`;
3. `cargo test --workspace`;
4. browser bridge syntax/build;
5. Business Runtime tests;
6. NEXUS bridge import/compile;
7. P7 `synergy-node` + `agent-fabric-stdio` build;
8. one local `e2e-sales` run;
9. one optional remote-node E2E run over SHTTPS/STCP.

Until then the PR stays draft.
