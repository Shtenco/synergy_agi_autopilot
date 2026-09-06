# Federation V3 — AUTOPILOT / Bounded Executive Control expansion

Historical Part VIII imagined one AI management layer optimizing the whole system. Federation V3 splits reasoning from authority.

## AUTOPILOT role

`NEXUS proposal → policy check → capability check → approval check → domain adapter → action request → evidence receipt`.

## Domains to orchestrate

- household/personal-capital workflows;
- Business/Growth OS;
- MetaPay/payment workflows;
- Financial OS bounded administrative workflows;
- science experiments and commercialization workflows;
- productive-project workflows;
- public-budget analytical workflows;
- Meta-OS runtime operations;
- communications/content workflows.

## Required control objects

- `PolicyDecision`;
- `CapabilityGrant`;
- `ApprovalRequirement`;
- `ActionPlan`;
- `ActionStep`;
- `ExecutionReceipt`;
- `RollbackPlan`;
- `EscalationEvent`;
- `HumanOverride`;
- `ExpiredProposal`.

## Safety/authority invariants

- reasoning is not authorization;
- authorization is not execution;
- service running is not financial authority;
- every consequential action must be attributable to an explicit policy/capability chain;
- irreversible/high-impact actions require stronger approval classes;
- no silent privilege escalation.

## Historical AI-uplift translation

Old fixed claims such as “AI +15%/+20%/+30%” become measurable experiments with baseline, treatment, time window and evidence. AUTOPILOT records interventions; domain systems measure outcomes.
