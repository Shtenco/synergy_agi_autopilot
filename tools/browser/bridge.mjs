import { chromium } from "playwright";

function fail(message, code = 2) {
  process.stderr.write(`${message}\n`);
  process.exit(code);
}

function parseRequest() {
  const raw = process.argv[2];
  if (!raw) fail("missing browser request JSON");
  let request;
  try {
    request = JSON.parse(raw);
  } catch (error) {
    fail(`invalid browser request JSON: ${error.message}`);
  }
  if (!Array.isArray(request.actions) || request.actions.length === 0 || request.actions.length > 64) {
    fail("actions must contain between 1 and 64 typed steps");
  }
  return request;
}

function requireString(value, name) {
  if (typeof value !== "string" || value.length === 0) fail(`${name} must be a non-empty string`);
  return value;
}

function validateUrl(value) {
  const url = new URL(requireString(value, "url"));
  if (url.protocol !== "http:" && url.protocol !== "https:") fail("only http/https browser navigation is allowed");
  return url.toString();
}

const request = parseRequest();
const timeout = Number.isFinite(request.timeout_ms) ? Math.max(1, Number(request.timeout_ms)) : 30000;
const headless = request.headless !== false;
let browser;
let context;

try {
  if (request.profile_dir) {
    context = await chromium.launchPersistentContext(requireString(request.profile_dir, "profile_dir"), { headless });
  } else {
    browser = await chromium.launch({ headless });
    context = await browser.newContext();
  }
  context.setDefaultTimeout(timeout);
  let pages = context.pages();
  let page = pages[0] ?? await context.newPage();
  const results = [];

  for (const [index, action] of request.actions.entries()) {
    switch (action.action) {
      case "navigate": {
        const url = validateUrl(action.url);
        const response = await page.goto(url, { waitUntil: "domcontentloaded", timeout });
        results.push({ index, action: action.action, url: page.url(), status: response?.status() ?? null });
        break;
      }
      case "snapshot": {
        results.push({
          index,
          action: action.action,
          url: page.url(),
          title: await page.title(),
          text: (await page.locator("body").innerText({ timeout })).slice(0, 20000),
        });
        break;
      }
      case "click": {
        const selector = requireString(action.selector, "selector");
        await page.locator(selector).click({ timeout });
        results.push({ index, action: action.action, selector, url: page.url() });
        break;
      }
      case "fill": {
        const selector = requireString(action.selector, "selector");
        const value = typeof action.value === "string" ? action.value : fail("value must be a string");
        await page.locator(selector).fill(value, { timeout });
        results.push({ index, action: action.action, selector });
        break;
      }
      case "wait_for": {
        const selector = requireString(action.selector, "selector");
        const actionTimeout = Number.isFinite(action.timeout_ms) ? Math.max(1, Number(action.timeout_ms)) : timeout;
        await page.locator(selector).waitFor({ state: "visible", timeout: actionTimeout });
        results.push({ index, action: action.action, selector });
        break;
      }
      case "screenshot": {
        const output = requireString(action.output, "output");
        await page.screenshot({ path: output, fullPage: true });
        results.push({ index, action: action.action, output });
        break;
      }
      default:
        fail(`unsupported browser action: ${String(action.action)}`);
    }
  }

  process.stdout.write(`${JSON.stringify({ ok: true, final_url: page.url(), results })}\n`);
} catch (error) {
  process.stderr.write(`${error?.stack ?? error}\n`);
  process.exitCode = 1;
} finally {
  try { await context?.close(); } catch {}
  try { await browser?.close(); } catch {}
}
