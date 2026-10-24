# AGI Autopilot — scientific specification for bounded autonomous work

## Research goal

The project defines the future boundary between an AI's plan and actual authorized computer work. A helpful agent can decompose a complex task into a directed acyclic graph, but execution needs source permissions, dependency checks, timeouts, reversible actions and independently verifiable acceptance criteria.

This repository's current main is **architecture-only** (README rather than active Python runtime), so its strongest present result is a clear system-level contract rather than a demonstrated self-running agent. That distinction allows new research to remain credible.

## Formal model

Define an action graph $G=(V,E)$ with tasks V and precedence edges E. A valid plan requires acyclicity. Task v may execute only after all predecessors have successful evidence and the operator grants an allowed capability:

$$
\mathrm{Runnable}(v)=\mathrm{Authorized}(v)\land
\bigwedge_{u\in\mathrm{Pred}(v)}\mathrm{VerifiedDone}(u).
$$

Output generation is distinct from evidence-based completion. A claim that code is deployed should require an observed service check; a claim that money moved should require canonical settlement evidence.

## Experimental implementation programme

Start with a local simulator using typed Task, Capability, Artifact and Evidence records. Implement deterministic DAG validation, scheduling, executor sandboxing, retries with idempotency keys and rollback for reversible operations. Run negative tests: denied capabilities, cyclic dependencies, forged output artifacts, stale credentials, irreversible action requiring review and external provider timeout. Preserve full execution graph and source digests.

## Scientific and commercial potential

The promising application is an **auditable software-development and business-process autopilot** that composes coding, research, marketing and data agents without silently escalating permissions. It can integrate with NEXUS for planning, AI Coder for change evidence and Meta-OS for bounded execution.

An agent economy becomes genuinely useful when its claimed work is measured by independently inspectable artifacts, not by fluent status messages.

[Architecture overview](../README.md) · [Related AI Coder](https://github.com/Shtenco/synergy_ai_coder)
