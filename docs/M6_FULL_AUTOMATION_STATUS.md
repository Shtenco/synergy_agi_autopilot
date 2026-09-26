# M6 — FULL AUTOMATION CONTROL PLANE

Status: **IMPLEMENTED / PHYSICAL QUALIFICATION PENDING**

## Implemented

### Trigger + scheduler

- Manual trigger model.
- Webhook trigger.
- Event trigger with field matching.
- One-shot schedule.
- Interval schedule.
- Persistent last-fired state.
- Due-schedule dispatch into MissionQueue.

### Persistent event bus

SQLite-backed `automation_events` with:

- pending / leased / completed / dead states;
- lease owner + lease expiry;
- crash recovery;
- retry scheduling;
- exponential backoff;
- dead-letter transition.

Webhook ingress from Connector Gateway can write directly into the same control-plane DB when `AUTOPILOT_CONTROL_DB` is configured.

### Approvals

SQLite-backed approval requests:

- pending / approved / denied;
- actor;
- reason;
- timestamp.

When PluginExecutor uses `control_plane_db`, an approval-gated capability requires:

1. `approved=true`;
2. `_approval_id` in plugin input;
3. persisted approval status = approved;
4. approval action bound to the exact `plugin_id + capability`.

An approval for one operation cannot be reused for another capability.

### Budgets

Daily micro-USD budgets are persistent.

A plugin input may include:

```json
{
  "_budget": {
    "id": "campaign-main",
    "amount_microusd": 250000
  }
}
```

The budget is reserved before external execution. Insufficient budget fails closed.

### Secret Vault

- `SecretRef`;
- allowlisted env-backed vault;
- secret values redact `Debug` / `Display`;
- bearer tokens resolved only in trusted runtime;
- connector-side secret references require their own allowlist.

### Health / resilience

Per-plugin circuit breaker:

```text
CLOSED
  -> repeated failures
OPEN
  -> recovery window
HALF_OPEN
  -> success => CLOSED
  -> failure => OPEN
```

### Universal Connector Fabric

Rust connector primitives:

- OpenAPI operation discovery;
- OAuth2 client-credentials token acquisition;
- read-only SQLite adapter;
- queue abstraction.

Connector Gateway sidecar:

- HTTPS/HTTP requests with host allowlist;
- OpenAPI operation execution;
- OAuth2 client credentials;
- secret references;
- read-only SQLite queries under a jailed root;
- persistent queue;
- webhook ingress.

### Capability packs

- `connector.gateway`
- `capability.email`
- `capability.calendar`
- `capability.crm`

Email/Calendar/CRM are not separate Autopilots; they are capability packs over the common connector gateway.

## CLI operations

```text
automation-add
automation-tick
automation-event
approval-request
approval-decide
budget-set
budget-reserve
plugin-call --control-db ...
plugin-goal --control-db ...
graph-goal --control-db ...
e2e-sales --control-db ...
```

## Persistence model

The same SQLite database can hold:

- mission queue;
- checkpoints;
- audit events;
- memory records;
- automations;
- event queue;
- approvals;
- budgets.

This provides restart-safe automation state without making SQLite a distributed consensus authority.

## Qualification gate

M6 remains unqualified until a physical runner executes:

- Rust fmt/clippy/tests;
- business runtime tests;
- NEXUS bridge import/compile;
- Connector Gateway import/compile;
- webhook -> control DB -> automation event -> mission dispatch;
- approval-bound high-risk plugin call;
- insufficient-budget rejection;
- circuit-breaker open/recovery test.

GitHub Actions currently fails before runner allocation (`runner_id=0`, `steps=[]`).
