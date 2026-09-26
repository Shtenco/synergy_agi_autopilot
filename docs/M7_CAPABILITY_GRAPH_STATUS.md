# M7 — CAPABILITY GRAPH

Status: **FOUNDATION + DIRECTOR ROUTING IMPLEMENTED / FULL REPOSITORY ENRICHMENT PENDING**

## Canonical repository catalog

Current GitHub discovery returns **73 repositories** under `Shtenco`.

A seed catalog containing all 73 current repositories is stored at:

`catalog/repositories.json`

Schema:

`synergy.repository-capability-catalog.v1`

The catalog is now evidence-enriched for all 73 repositories:
- 70 repositories from default-branch README evidence;
- 2 GitHub Pages repositories from `index.html`;
- `sinergy_super_app` from internal documentation + mobile tree evidence.

The catalog remains conservative: repositories without a demonstrated executable interface are not advertised as executable plugins.

Catalog counters:
- `repository_count = 73`
- `readme_enriched_count = 70`
- `alternate_evidence_count = 3`
- `evidence_enriched_count = 73`

## Automatic enrichment

`scripts/build-capability-catalog.py` can regenerate/enrich the catalog from GitHub by inspecting:

- repository metadata;
- README;
- `synergy.plugin.v1` manifests;
- declared capabilities;
- endpoint transports.

It uses `GITHUB_TOKEN` / `GH_TOKEN` and writes no token into the catalog.

## Graph model

```text
Human Goal
   ↓
Goal Terms / aliases
   ↓
Capability nodes
   ↓
Plugin providers
   ↓
Repositories
   ↓
Transports
   ↓
Policy / approval / budget
   ↓
Execution
```

Nodes track:

- capability;
- plugin providers;
- repositories;
- transports;
- tags.

## Goal routing

The graph supports natural-language goal routing.

Russian/English aliases currently include examples such as:

- клиент / лид -> sales, crm, lead;
- контент -> smm, content;
- реклама -> ads, marketing;
- письмо / почта -> email;
- календарь -> calendar, schedule;
- браузер / сайт -> browser, web;
- файл / компьютер -> computer/filesystem/desktop;
- память -> memory;
- анализ -> analyze/analysis;
- трейд -> trading/market.

## Director integration

`CapabilityGraphPlanner` implements the canonical `Planner` trait.

`GraphGoalEngine` combines:

- Capability Graph;
- Director;
- Policy Engine;
- PluginExecutor;
- verification/audit.

The default graph-routed Director filters destructive/write capabilities such as:

- send;
- create;
- update;
- publish;
- execute;
- submit;
- write;
- delete;
- click;
- fill;
- run_command.

Those operations require an explicit approved plugin plan rather than autonomous default routing.

## CLI

### Search repositories/providers

```text
open-autopilot capability-search --capability business.sales.qualify --keyword sales
```

### Show route plan for a human goal

```text
open-autopilot capability-plan --goal "найди клиента и подготовь контент"
```

### Execute safe graph-routed goal

```text
open-autopilot graph-goal --goal "проанализируй лид и найди релевантную память"
```

## Next enrichment pass

The next M7 pass should enrich each of the 73 repository descriptors with:

- canonical ownership domain;
- actual runtime/API surfaces;
- plugin eligibility;
- input/output schemas;
- risk class;
- evidence freshness;
- health;
- version;
- deployment location;
- resource requirements.

Repositories without a real executable interface should remain knowledge/providers-to-wrap, not falsely advertised as executable plugins.
