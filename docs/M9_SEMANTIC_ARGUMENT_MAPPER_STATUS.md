# M9 — Universal Semantic Argument Mapper + Adaptive Workflow Synthesizer

Status: **IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING**

## Goal

M9 turns human objectives into typed, validated plugin inputs and multi-step workflows.

```text
Human goal
  -> semantic clauses
  -> typed capability selection
  -> input contract
  -> argument mapping
  -> dry-run validation
  -> workflow DAG
  -> approval/budget gates
  -> runtime remapping from prior outputs
  -> execution
  -> verification
```

## Transport-neutral input contracts

`synergy.plugin.v1` now supports:

```json
{
  "input_contracts": {
    "capability.name": {
      "type": "object",
      "properties": {},
      "required": []
    }
  }
}
```

The contract is independent of transport and can originate from:

- OpenAPI parameters + requestBody schema;
- MCP `inputSchema`;
- declarative CLI capability descriptors;
- built-in plugin manifests.

## Semantic argument mapper

`autopilot-semantic-planner` provides deterministic mapping from:

- goal text;
- explicit hints;
- memory;
- actual outputs from prior workflow steps;
- schema defaults / const / enum;

into typed JSON arguments.

Supported evidence includes:

- email;
- URL;
- path;
- query/prompt/goal;
- title/subject/name;
- body/message/content;
- IDs;
- integer/number/boolean fields;
- aliases such as `email -> to`, `message -> body`, `lead_id -> client_id`.

The mapper returns:

- `arguments`;
- `unresolved_required`;
- `validation_errors`;
- per-field mapping evidence/confidence.

It does not invent missing required values.

## JSON contract validation

The validator currently supports practical JSON Schema features including:

- object/array/string/number/integer/boolean/null;
- required properties;
- additionalProperties=false;
- enum / const;
- minLength / maxLength;
- minimum / maximum;
- minItems / maxItems;
- email / URI formats;
- allOf / anyOf / oneOf;
- local JSON Schema references.

Final plugin execution validates the actual arguments again inside `PluginExecutor`.

Manual `plugin-call` cannot bypass the contract validator.

## OpenAPI argument routing

Generated OpenAPI capabilities now map flat typed arguments into:

- path parameters;
- query parameters;
- header parameters;
- JSON request body.

OpenAPI parameter location is preserved in the input contract through `x-location`.

Connector Gateway supports typed query/header/path/body dispatch.

Generated adapters cannot supply protected headers such as:

- Host;
- Content-Length;
- Authorization;
- Connection.

OAuth configuration is control metadata (`_oauth2`) and remains outside the business input contract.

## MCP input contracts

MCP discovery now persists the real `inputSchema` returned by `tools/list`.

That schema becomes the capability's `input_contract`, so the same semantic mapper and validator are used for MCP and OpenAPI.

## Declarative CLI contracts

`FilesystemCliDescriptorDiscovery` supports:

- `synergy.cli.json`;
- `cli-capabilities.json`;
- schema `synergy.cli-capabilities.v1`.

CLI argument semantics are not guessed. A CLI adapter is typed only when an explicit `input_schema` is declared.

## Adaptive workflow synthesis

`AdaptiveWorkflowSynthesizer` splits goals by explicit sequencing markers such as:

- `→`;
- `->`;
- `затем`;
- `потом`;
- `and then`;
- semicolon.

Each clause must resolve to a typed provider.

The planner never silently drops an unresolved clause. Missing coverage is reported in:

`unresolved_objectives`

and forces:

`ready=false`.

## Runtime remapping

Dependent steps may intentionally defer required fields.

Before every plugin call, `PluginExecutorBridge` reruns semantic mapping using actual prior outputs.

Example:

```text
step 1 result:
{
  "lead": {
    "email": "client@example.com"
  },
  "proposal": {
    "message": "Здравствуйте..."
  }
}

step 2 contract:
{
  "to": "email",
  "body": "string"
}

runtime remap:
email -> to
message -> body
```

If required fields remain unresolved, execution fails before the external call.

## Workflow approvals

`workflow-run` supports per-step:

- `--approve-step step-N`;
- `--approval-id step-N=<uuid>`.

When `--control-db` is enabled, an approval-gated step requires its persisted approval ID during preflight.

The runtime still re-verifies that the approval is:

- approved;
- bound to the exact plugin;
- bound to the exact capability.

## Universal goal integration

`universal-goal` now:

1. splits the full objective into clauses;
2. detects typed gaps per clause;
3. self-builds missing providers through OpenAPI / MCP / declarative CLI;
4. updates Plugin Registry and Capability Graph after every safe promotion;
5. refuses High-risk auto-promotion;
6. requires typed coverage for all workflow clauses;
7. synthesizes the workflow;
8. refuses unresolved arguments;
9. refuses approval-required steps in automatic mode;
10. executes via `PluginGoalEngine`.

The old untyped GraphGoal fallback has been removed from `universal-goal`.

## CLI

```text
argument-map
workflow-plan
workflow-run
universal-goal
```

### Dry-run argument mapping

```powershell
open-autopilot argument-map   --plugin generated.crm   --capability crm.read   --goal "найди клиента ID abc-42"
```

### Workflow dry-run

```powershell
open-autopilot workflow-plan   --goal "найди клиента → подготовь предложение → отправь email"
```

### Controlled workflow execution

```powershell
open-autopilot workflow-run   --goal "найди клиента → отправь email"   --approve-step step-2   --approval-id step-2=<approved-request-uuid>   --control-db .autopilot/control.db
```

## Typed built-in capabilities

Typed contracts are present for major built-ins including:

- NEXUS intelligence/memory;
- Business Runtime;
- Browser;
- Local Commander;
- Remote MCP;
- Agent Fabric;
- Auto-Company.

Generic Email/Calendar/CRM packs remain connector abstractions until bound to a concrete API/provider; they are not falsely treated as fully configured typed providers.

## Safety invariants

- No missing required argument is fabricated.
- Untyped workflow clauses are never silently skipped.
- Final arguments are validated again immediately before plugin dispatch.
- Control metadata is excluded from business-contract validation.
- OAuth secrets remain in Secret Vault references.
- High-risk self-built capabilities are never auto-promoted.
- Approval-required workflow steps never auto-run in universal mode.
- Persistent approval IDs are required in control-plane execution.
- Old untyped fallback is removed from universal mode.

## Tests

M9 tests cover:

- email/message mapping;
- missing required argument rejection;
- prior-output alias mapping;
- local schema references;
- combinators and bounds;
- type/additional-property validation;
- dependent-step deferred mapping;
- incomplete workflow coverage;
- linear workflow synthesis;
- wrapped email/URL/ID token extraction.

## Qualification gate

M9 remains **PHYSICAL QUALIFICATION PENDING** until a real runner captures:

1. `cargo fmt --all -- --check`;
2. `cargo clippy --workspace --all-targets -- -D warnings`;
3. `cargo test --workspace`;
4. OpenAPI parameterized fixture E2E;
5. MCP inputSchema fixture E2E;
6. prior-step output -> next-step required fields E2E;
7. approval preflight rejection;
8. persistent approval success;
9. universal-goal multi-clause self-build + workflow execution.

Canonical verifier:

`scripts/verify-autopilot.ps1`
