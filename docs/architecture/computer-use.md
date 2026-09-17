# Computer Use Architecture

OPEN AUTOPILOT treats desktop and browser automation as typed tools behind the same local policy engine used for terminal, Git and network actions.

## Windows UI Automation

The first Windows adapter uses Microsoft `winapp ui` through `RuntimeAdapter`.

M4 intentionally supports only UI Automation pattern operations:

- inspect
- search
- get-value
- set-value
- invoke
- wait-for
- screenshot

The adapter constructs an executable plus argument vector. Selectors and values are passed as individual arguments; mission text is never concatenated into a PowerShell/cmd shell string.

Targets are explicit:

- application/process/title with `-a`;
- stable HWND with `-w`.

Every command requests JSON output for structured observation when supported.

M4 deliberately omits raw/system input injection such as global hotkeys, send-input typing, drag, touch and pen. Those operations have different desktop/focus/security semantics and will require a separate high-risk capability in a later milestone.

## Policy

Any Windows automation requires `Capability::ComputerUse`.

Read-like operations:

- inspect
- search
- get-value
- wait-for
- screenshot

Write-like operations:

- set-value
- invoke

A future tool dispatcher will evaluate the capability/risk tuple before calling the adapter. Computer Use never bypasses the central policy engine merely because the adapter is local.

## Browser automation

The browser adapter sends one structured JSON action plan to `tools/browser/bridge.mjs`. The bridge launches one Playwright browser context and executes the plan sequentially, so a mission can perform:

`Navigate -> WaitFor -> Fill -> Click -> Snapshot`

without losing page state between actions.

Supported actions:

- navigate
- snapshot
- click
- fill
- wait_for
- screenshot

Constraints:

- 1–64 actions per request;
- only HTTP/HTTPS navigation;
- no arbitrary JavaScript evaluation;
- selectors and values are structured JSON fields;
- optional persistent profile directory belongs to OPEN AUTOPILOT;
- request timeout propagates into the runtime command;
- network navigation requires both ComputerUse and NetworkAccess policy scopes.

The bridge returns one JSON response containing per-step results and final URL.

## Why two adapters

Windows native apps expose rich UIA patterns, while websites are more robustly controlled through DOM/accessibility locators. Both adapters implement the same architectural rule: the Director chooses a typed tool, the policy engine authorizes it, and the adapter performs only the declared action.

## Prerequisites

Windows UI Automation requires Microsoft `winapp` CLI installed and available on PATH.

Browser automation requires Node.js plus dependencies in `tools/browser`. The CI workflow performs dependency installation and `node --check bridge.mjs`; browser binaries are installed separately when end-to-end browser tests are enabled.
