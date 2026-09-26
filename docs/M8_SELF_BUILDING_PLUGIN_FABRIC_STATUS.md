# M8 — Universal Semantic Planner + Self-Building Plugin Fabric

Status: **IMPLEMENTED CORE / PHYSICAL QUALIFICATION PENDING**

## Objective

When Director cannot satisfy a human goal with an existing executable capability, Autopilot must not simply fail.

The M8 control flow is:

```text
Human goal
  -> Capability Graph
  -> executable provider?
       yes -> normal Director execution
       no
         -> Semantic Gap
         -> repository/transport discovery targets
         -> OpenAPI / MCP / CLI surface discovery
         -> generated adapter candidate
         -> quarantine
         -> static validation
         -> sandbox qualification
         -> policy/risk decision
         -> safe promotion
         -> Plugin Registry
         -> Capability Graph refresh
         -> Director retries the same goal
```

## Implemented

### Semantic gap planning

`SemanticGapDetector` detects when no executable provider matches the goal.

`SelfBuildPlan` contains:

- suggested capability;
- semantic terms;
- ranked repository candidates from the 73-repository Capability Graph;
- discovery strategies;
- mandatory qualification gates;
- `auto_execution_allowed=false` until the adapter passes the fabric.

### Static surface discovery

`scripts/discover-plugin-surfaces.py` scans without executing repository code.

It detects:

- OpenAPI/Swagger JSON documents;
- MCP configuration files;
- Cargo binary entrypoints;
- Python project scripts;
- Node package `bin` entrypoints.

The output schema is:

`synergy.plugin-discovery.v1`

Security invariant:

```text
executed_code = false
network_access = false
```

### Build proposal generation

`scripts/propose-plugin-build.py` ranks discovered surfaces and emits:

`synergy.plugin-proposals.v1`

OpenAPI read operations can become complete portable `BuildRequest` objects automatically.

MCP proposals require a tool resolved from sandbox `tools/list`.

CLI entrypoints are discovered, but arbitrary argv semantics are **not guessed**.

### Universal generated adapter contract

`synergy.plugin.v1` now has an optional `adapter` field.

Supported generated adapters:

- OpenAPI;
- MCP stdio;
- CLI.

Canonical JSON Schema:

`schemas/synergy.plugin.v1.schema.json`

The contract validator rejects:

- generated adapters enabled by default;
- kind/adapter mismatch;
- non-HTTPS external OpenAPI base URLs;
- missing OpenAPI operationId;
- shell metacharacters in MCP/CLI command specs;
- empty MCP tools/programs.

### Candidate lifecycle

```text
Quarantined
   -> qualification pass -> Qualified
   -> qualification fail -> Rejected

Qualified
   -> promotion policy -> Promoted
```

A quarantined or rejected candidate cannot be promoted.

### Sandbox qualification

`SandboxQualifier` checks:

- manifest validity;
- disabled-by-default invariant;
- approval requirement for High/Critical risk;
- transport source contract;
- sandbox probe.

MCP qualification performs:

```text
initialize
 -> notifications/initialized
 -> tools/list
 -> requested tool must exist
```

MCP/CLI program execution also requires an explicit executable allowlist.

Low/Medium generated MCP/CLI execution uses `ReadOnlyWorkspace`.

High/Critical generated MCP/CLI execution uses `WorkspaceWrite` and remains approval-gated.

Native runtime intentionally cannot satisfy `ReadOnlyWorkspace`; Docker or another isolating runtime is required for safe generated MCP/CLI execution.

### Automatic OpenAPI discovery

`FilesystemOpenApiDiscovery` recursively scans a bounded local repository tree.

It:

- ignores build/vendor directories;
- parses OpenAPI JSON;
- semantically ranks operations against the missing capability;
- prefers parameterless GET operations;
- marks GET as Medium;
- marks mutating methods High + approval-required.

Operations requiring unresolved path/query inputs rank below autonomously invocable reads.

### Automatic MCP discovery

`FilesystemMcpDiscovery`:

1. statically locates MCP configs;
2. accepts only allowlisted server commands;
3. starts the server in read-only isolation;
4. calls `tools/list`;
5. scores tool name + description against the semantic gap;
6. classifies write-like tools as High risk;
7. generates a portable MCP `BuildRequest`.

### Multi-provider discovery

`BuildDiscoveryChain` combines multiple discovery providers.

Current `universal-goal` chain:

```text
FilesystemOpenApiDiscovery
  + FilesystemMcpDiscovery
```

The coordinator tries multiple candidates rather than failing on the first invalid/rejected proposal.

A safe auto-promotable candidate is preferred over a qualified High-risk candidate.

### Safe promotion policy

Default `SelfBuildPolicy`:

- auto-promotion enabled;
- maximum auto-promote risk = Medium;
- approval-required candidates are never auto-promoted.

High/Critical or approval-required candidates finish as:

`QualifiedAwaitingPromotion`

### Generic generated execution

`PluginExecutor` no longer requires a hard-coded plugin ID for promoted adapters.

Generic execution paths:

- generated OpenAPI -> trusted Connector Gateway;
- generated MCP -> MCP stdio runtime;
- generated CLI -> fixed program/argv + JSON stdin.

Generated MCP/CLI execution requires:

1. promoted manifest;
2. runtime executable allowlist;
3. policy/approval gates;
4. risk-based isolation profile.

### Registry integration

`PluginRegistry::load_dir()` is now the canonical loader for promoted manifests.

`AUTOPILOT_GENERATED_PLUGINS` can point the CLI at a promoted plugin root.

Invalid or dependency-incomplete generated manifests fail closed.

### Universal goal loop

CLI:

```text
open-autopilot universal-goal
```

Behavior:

```text
goal
 -> existing provider? execute
 -> gap?
 -> OpenAPI + MCP discovery
 -> build
 -> quarantine
 -> qualify
 -> auto-promote if safe
 -> register
 -> refresh graph
 -> retry same goal through Director
```

Example:

```powershell
cargo run -p open-autopilot -- universal-goal \
  --goal "получи статус внешнего сервиса" \
  --discovery-root C:\src\external-service \
  --workspace C:\work
```

For generated MCP/CLI use an isolating runtime and allow the exact executable:

```powershell
cargo run -p open-autopilot -- universal-goal \
  --goal "прочитай данные через новый MCP" \
  --discovery-root C:\src\mcp-service \
  --workspace C:\work \
  --runtime docker \
  --docker-image synergy/autopilot-sandbox:latest \
  --allow-generated-program my-mcp-server
```

## Operator lifecycle commands

- `semantic-gap`
- `plugin-build`
- `plugin-qualify`
- `plugin-promote`
- `universal-goal`

These commands expose the same lifecycle for inspection and manual control.

## Tests added

M8 tests cover:

- semantic gap detection;
- OpenAPI candidate generation;
- shell-metacharacter rejection;
- quarantined candidate promotion denial;
- safe auto-promotion;
- High-risk hold;
- OpenAPI parameterless-read ranking;
- MCP sandbox tools discovery;
- unallowlisted CLI rejection;
- generated manifest contract rejection;
- promoted manifest directory loading.

CI also includes a static OpenAPI discovery -> proposal smoke test.

## Remaining M8 qualification gates

M8 is not physically GREEN until a real runner captures:

1. `cargo fmt --all -- --check`;
2. `cargo clippy --workspace --all-targets -- -D warnings`;
3. `cargo test --workspace`;
4. M8 Python static discovery/proposal smoke;
5. OpenAPI `universal-goal` against a real local fixture service;
6. MCP `universal-goal` in Docker read-only isolation;
7. rejection of an unallowlisted MCP/CLI executable;
8. High-risk candidate remains unpromoted without approval;
9. promoted adapter reload through `PluginRegistry::load_dir`.

Canonical local verification:

`scripts/verify-autopilot.ps1`

## Explicit limitations

M8 does **not** invent arbitrary CLI semantics.

CLI self-building requires either:

- an explicit BuildRequest mapping; or
- a future declarative CLI capability descriptor.

Parameterized OpenAPI operations may still require semantic argument mapping. That belongs to the next planner layer rather than being guessed unsafely.

No generated adapter receives payment, financial signing, destructive host, or secret authority merely because it was discovered.
