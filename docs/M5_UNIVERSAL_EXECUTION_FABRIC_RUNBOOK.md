# M5 — Universal Execution Fabric runbook

This runbook exercises the first universal SYNERGY AI AUTOPILOT vertical slice.

## Implemented execution providers

| Provider | Runtime path | Contract |
|---|---|---|
| Local Node Commander | `synergy-node mcp --stdio` | MCP JSON-RPC |
| Remote MCP | `synergy-mcp-gateway` | MCP over authenticated SHTTPS/STCP |
| Agent Fabric | `agent-fabric-stdio` | `synergy.worker.v1` JSON-RPC |
| NEXUS | `sidecars/nexus-bridge` | loopback HTTP compatibility surface |
| Business Runtime | `sidecars/business-runtime` | `synergy.capability.v1` |
| Auto-Company | `synergy_ai_company` Auto-Company 2.0 | loopback HTTP `/v1` |

The Rust Autopilot remains the authority boundary. Provider availability does not grant execution authority.

## 1. Build P7 executables

From a checkout of `Shtenco/synergy_ai_blockchain` branch
`feat/p7-node-commander-agent-fabric`:

```bash
cargo build --manifest-path node-runtime/Cargo.toml --bins
```

Put `node-runtime/target/debug` on `PATH`, or otherwise expose:

- `synergy-node`
- `agent-fabric-stdio`

## 2. Start Business Runtime

From `sidecars/business-runtime`:

```bash
python -m venv .venv
# activate the venv
pip install -e ".[test]"
uvicorn agi_autopilot.app:app --host 127.0.0.1 --port 8120
```

Optional auth:

```text
AUTOPILOT_API_TOKEN=<secret>
```

External effects remain disabled unless the existing business-runtime policy is explicitly enabled.

## 3. Start NEXUS compatibility bridge

From `sidecars/nexus-bridge`:

```bash
pip install -e .
synergy-nexus-bridge
```

Default endpoint: `127.0.0.1:8130`.

The bridge's SQLite memory is an integration cache, not the canonical NEXUS model memory.

For real NEXUS inference, configure:

```text
NEXUS_INFERENCE_CMD=<reviewed executable receiving JSON on stdin>
NEXUS_BRIDGE_TOKEN=<optional secret>
```

Without `NEXUS_INFERENCE_CMD`, memory search/upsert still work but `/v1/infer` fails closed with HTTP 503.

## 4. Start Auto-Company 2.0

From `Shtenco/synergy_ai_company` branch `feat/auto-company-v2`:

```bash
pip install -e ".[dev]"
python scripts/run_api.py
```

Canonical default bind from Auto-Company settings: `127.0.0.1:8788`.

Optional auth:

```text
SYNERGY_API_TOKEN=<secret>
```

Keep `SYNERGY_COMPANY_MODE=paper` for M5 verification.

## 5. Optional Remote MCP

On the remote node, from `Shtenco/synergy_shttps` branch
`feat/stcp-shttps-remote-mcp-v2-20260925`:

```bash
pip install -e .
synergy-remote-mcp-node --root /approved/workspace --allow-commands git,python,python3,cargo,rustc
```

On the controller node, configure the reviewed SHTTPS/STCP identity/session environment expected by `synergy-mcp-gateway`.

Remote actions remain limited by the server-side filesystem jail and executable allowlist.

## 6. Inspect the capability graph

```bash
cargo run -p open-autopilot --bin open-autopilot -- plugins
```

## 7. Test one plugin

Safe local system-information example:

```bash
cargo run -p open-autopilot --bin open-autopilot -- plugin-call \
  --plugin computer.local.commander \
  --capability computer.system_info \
  --input-json "{}" \
  --workspace .
```

A high-risk capability such as `computer.run_command` fails unless the call has explicit approval.

## 8. Run the first multi-provider Director goal

```bash
cargo run -p open-autopilot --bin open-autopilot -- plugin-goal \
  --goal "Find a potential client and prepare the sales package" \
  --plan examples/m5/sales-prep-plan.json \
  --workspace .
```

The plan exercises:

```text
Director
 -> NEXUS memory
 -> Auto-Company opportunity
 -> Sales capability
 -> SMM capability
 -> Local Commander file evidence
 -> Agent Fabric lease
 -> structured verification
 -> audit trail
```

Remote MCP is intentionally optional in this baseline because a second physical node may not exist during a local test.

## GREEN definition

M5 is GREEN only when all of these are evidenced on an actual host:

1. `cargo check --workspace`
2. `cargo test --workspace`
3. business-runtime base tests
4. P7 Node Runtime build/tests including `agent-fabric-stdio`
5. `open-autopilot plugins`
6. safe Local Commander plugin call
7. Agent Fabric lease call
8. NEXUS memory call
9. Business Runtime call
10. Auto-Company call
11. full `plugin-goal` plan reaches `Completed`
12. optional two-device Remote MCP execution when a second node is available

Do not call M5 fully GREEN merely because source compiles.
