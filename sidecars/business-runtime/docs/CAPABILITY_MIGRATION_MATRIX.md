# AUTOPILOT capability migration matrix

## Historical agents

| Legacy role | New disposition | Runtime surface |
|---|---|---|
| CEO | active, bounded planner | `ceo.plan` |
| Memory | delegated | NEXUS `/v1/memory/*` |
| SMM | active | `smm.*` + approved official publishing |
| Sales | active | `sales.*` + approved CRM/messaging |
| Coder | active without shell | `coder.change_plan` |
| Finance / CFO | active analytical only | `cfo.*` |
| Payments | delegated / hard denied in AUTOPILOT | `payment.execute` returns policy DENY |
| Desktop | hard denied in core runtime | `host.shell` returns policy DENY |
| Security | absorbed into deterministic policy gates | `PolicyEngine` |
| Verifier | active evidence-only | `verifier.check_evidence` |
| Cline / OpenClaw wrappers | not activated | broad host/tool authority; require separate reviewed capability adapters |

## Historical business APIs

**Activated wrappers:** Telegram Bot, VK wall/stats, Facebook Page, Instagram Business image/video/Reels/Stories, WhatsApp Business text, Discord webhook, Bitrix24, amoCRM read, RetailCRM read, configurable 1C HTTP read, Yandex Direct campaign read, Meta Ads campaign read, YouTube Data search, SMTP send.

**Found in legacy inventory but not activated until a current API review:** VK Ads mutations, myTarget mutations, Yandex Business mutations, generic IMAP mailbox automation, DocumentsHub document mutation and broad PC-local automation.

The distinction is intentional: finding an old integration is not evidence that its API contract or permission model is still safe/current.

## AI providers found historically

Groq, OpenCode Zen / Big Pickle, DeepSeek, OpenRouter, DeepInfra, OpenAI, Claude/Anthropic, Gemini, Mistral, Grok/xAI, NVIDIA NIM, Together AI, Cloudflare, YandexGPT, GigaChat and local providers.

The new runtime keeps these as inventory and uses an explicit OpenAI-compatible `AUTOPILOT_LLM_BASE_URL` + model configuration. It does not guess provider URLs or silently reuse archived credentials.
