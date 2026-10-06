# Third-Party Notices

OPEN AUTOPILOT is an independent codebase.

The following projects were reviewed as architectural references during initial design. No source code from them is intentionally copied into the current M0 implementation. If future changes incorporate or adapt third-party source, this file must be updated with the exact component, provenance, copyright notice, and license.

- OpenCode (`anomalyco/opencode`) — MIT License.
- OpenClaw (`openclaw/openclaw`) — MIT License at the time reviewed; its repository also maintains its own third-party notices.
- Harness (`twaldin/harness`) — MIT License.
- Harness (`majiayu000/harness`) — MIT License.
- OpenDots (`CopilotKit/OpenDots`) — MIT License, copyright Atai Barkai. Reviewed in M11 as an architectural reference for persistent specialist coworkers, per-agent computers/workspaces, human review, learning containers, channel identity and permission-scoped agent UX. No OpenDots runtime source is intentionally copied into the trusted Rust M11 core.

Architectural concepts reviewed include provider-neutral coding-agent invocation, agent orchestration, governance/policy, parallel workers, review, observability, desktop agent UX, and harness/runtime separation.
