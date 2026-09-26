# AGI AUTOPILOT — SYNERGY capability runtime

A cleaned, policy-gated runtime reconstructed from the historical AUTOPILOT V5 / AI Sales / SMM line and the Federation V3 contracts.

## Agents now exposed

- **AI_CEO** — goal planning into bounded capabilities.
- **AI_CFO** — budget, ROAS, CAC/LTV, payback and non-authoritative capital proposals.
- **AI_SMM** — content plans, briefs, metrics and approved official-platform publishing.
- **AI_SALES** — lead qualification, objection handling and follow-up/offer drafts.
- **AI_VIDEO** — storyboard/asset plan, YouTube research and approved video/Reels publication; generative rendering is a separate paid-provider boundary.
- **AI_ADS / CRM / Business API** — official integration surfaces.

Runtime contract: `synergy.capability.v1`.

## Start

```bash
python -m pip install -e .
python -m agi_autopilot
```

Default bind: `127.0.0.1:8120`.

Useful endpoints:

- `GET /health`
- `GET /v1/capabilities`
- `POST /v1/tasks`
- `GET /v1/tasks/{id}`
- `POST /v1/tasks/{id}/approve`
- `POST /v1/tasks/{id}/execute`

External side effects require both task approval and `AUTOPILOT_EXTERNAL_EFFECTS=1`. Accounting, payment signing, blockchain submission and arbitrary shell execution remain denied.
