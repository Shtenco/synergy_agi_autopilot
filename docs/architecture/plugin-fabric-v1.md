# SYNERGY Plugin Fabric v1

## Purpose

`synergy.plugin.v1` is the universal capability contract for SYNERGY AI AUTOPILOT.

The trusted Rust control plane does not import arbitrary integration code into its authority boundary. External systems are represented as manifests and invoked through reviewed adapters/sidecars.

## Core rule

```text
Goal
 -> Director
 -> capability requirements
 -> Plugin Registry
 -> Policy Engine
 -> adapter/sidecar
 -> result + evidence + audit event
```

A plugin may expose tools, models, memory, workers, business functions, browser/desktop automation or remote devices, but it cannot grant itself more authority than the control plane policy allows.

## Manifest

Every plugin declares:

- stable plugin id;
- schema `synergy.plugin.v1`;
- semantic version;
- plugin kind;
- provider/provenance;
- capabilities;
- risk class per capability;
- explicit approval requirement;
- mapping into core Autopilot capabilities;
- transport endpoints;
- dependencies;
- default enablement state.

## Initial plugin families

- `computer.local.commander` — P7 Node Commander over MCP stdio;
- `computer.remote.shttps` — authenticated Remote MCP over SHTTPS/STCP;
- `business.capability.runtime` — CEO/CFO/SMM/Sales/Video business sidecar.

Next providers:

- NEXUS intelligence/memory;
- Agent Fabric distributed workers;
- Auto-Company policy/revenue pack;
- browser;
- desktop;
- GitHub;
- email/calendar;
- OpenAPI/OAuth;
- databases;
- queues/webhooks;
- mobile;
- IoT.

## Authority boundary

Plugin manifests are declarations, not permissions.

The Policy Engine remains authoritative for whether a capability may execute. High-risk operations remain approval-gated. Secret material must stay outside renderer/model contexts and must never be embedded in manifests.
