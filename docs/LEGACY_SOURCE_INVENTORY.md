# AUTOPILOT legacy source inventory

Source basis used for this runtime:

1. User-supplied `AUTOPILOT2.zip`, especially V5/V3.
2. User-supplied `AI-Sales-Bot.zip`.
3. User-supplied `SMM AI BOT.zip`.
4. GitHub branch `federation-v3-domain-expansion` (policy contract lineage).
5. GitHub branch `feat/autopilot-v5-nexus-smm-ai-node-v1-20260823` (node/evidence architecture).

## Preserved capability ideas

- V5 agents: CEO, Memory, SMM, Sales, Coder, CFO/Finance, Pay, Desktop, Security, Verifier, plus Cline/OpenClaw wrappers.
- V5 pipeline concept: `SMM -> ADS -> SALES -> CFO`.
- official API inventory: Telegram, VK, Facebook/Instagram Graph, WhatsApp, Discord, Yandex Direct, VK Ads, Facebook Ads, myTarget, Bitrix24, 1C, amoCRM, RetailCRM, Yandex Business, SMTP/IMAP and YouTube Data.
- Sales Bot: lead stages, lead score, objection handling, follow-up and product brief.
- SMM Bot: content generation, official-platform publishing, insights/comments and Instagram video/Reels/Stories support.

## Deliberately not imported

Uploaded credentials, Telegram session files, private recipient/user dumps, runtime databases, certificates/keys, bulk invite tooling and broad desktop/shell execution are not copied into the active runtime.

The historical source contains video **publishing** paths but no verified generative video rendering provider. `AI_VIDEO` therefore provides storyboard/production planning and an explicitly gated provider boundary rather than claiming a renderer that does not exist in the sources.
