# NEXUS compatibility bridge

This sidecar gives SYNERGY AI AUTOPILOT a stable loopback HTTP surface for:

- `POST /v1/memory/search`
- `POST /v1/memory/upsert`
- `POST /v1/infer`

The SQLite memory layer is a compatibility cache for Autopilot integration. It is **not** a claim that SQLite is the canonical NEXUS memory implementation.

Real inference is delegated only when `NEXUS_INFERENCE_CMD` is configured. The configured executable receives one JSON object on stdin and must return JSON or text on stdout.

The bridge binds to loopback by default and supports an optional `NEXUS_BRIDGE_TOKEN`.
