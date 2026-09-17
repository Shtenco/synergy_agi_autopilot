# Persistence and Recovery

OPEN AUTOPILOT uses a local SQLite database as the durable source of truth for missions, checkpoints, project memory and audit history.

## Database location

Windows default:

`%LOCALAPPDATA%\OpenAutopilot\state.db`

Non-Windows fallback:

`~/.open-autopilot/state.db`

The CLI accepts an explicit database path for testing and isolated workers.

## Durable records

The store owns six logical areas:

- `schema_migrations` — schema version history;
- `audit_events` — append-only correlated execution events;
- `missions` — durable goal queue and lease metadata;
- `mission_events` — mission lifecycle history;
- `checkpoints` — latest structured recovery checkpoint per mission;
- `memory_records` — project-scoped structured facts/decisions with optional expiry.

Provider secret values are not part of these schemas.

## Mission lifecycle

A mission moves through:

`Queued -> Leased -> Running -> Waiting -> Completed | Failed | Cancelled`

Recovery transitions allow an expired `Leased`, `Running` or `Waiting` mission to return to `Queued`. Terminal states cannot return to execution.

`lease_next` uses a single SQLite `UPDATE ... RETURNING` selection/update operation inside a transaction. The selected mission increments its attempt counter and receives `lease_owner` plus `lease_until` before it is returned to a worker.

Workers must heartbeat long-running missions. A dead worker eventually loses its lease; recovery clears ownership and requeues the mission while retaining the checkpoint.

## Checkpoints

Recovery checkpoints are structured rather than raw transcript dumps.

Current phases:

- `Planned`
- `WorktreeReady`
- `FleetVerified`
- `Published`
- `WaitingCi`
- `RepairingCi`

The payload records worktree, correlation id, local repair count, CI repair count, PR URL and structured metadata.

## Restart rules

OPEN AUTOPILOT never blindly replays irreversible actions after a restart.

- `Planned`: restart the fleet.
- `WorktreeReady` / `FleetVerified`: verify the worktree still exists, then resume from it.
- `Published`: verify the worktree and inspect existing branch/PR state instead of creating another publication blindly.
- `WaitingCi` / `RepairingCi`: resume by reading current CI state for the existing PR.
- Missing referenced worktree: fail closed with explicit recovery evidence.

## Emergency stop

STOP cancels active runtime execution. It does not delete the mission, checkpoint, audit events or worktree. This is intentional: the next process can inspect/recover the interrupted mission.

## CLI

The dedicated durable queue binary is `open-autopilot-missions`.

Supported commands:

- `enqueue`
- `list`
- `show`
- `recover`
- `run-next`

`run-next` acquires a lease before running Full Fleet, stores a planned checkpoint, and stores a `FleetVerified` checkpoint before marking the mission complete. Infrastructure interruption returns the mission to the queue instead of silently dropping it.
