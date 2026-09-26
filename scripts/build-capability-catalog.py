from __future__ import annotations

import argparse
import base64
import json
import os
import re
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

API = "https://api.github.com"
PLUGIN_PATH_RE = re.compile(r"(^|/)plugin\.json$", re.I)

TAG_RULES: dict[str, tuple[str, ...]] = {
    "trading": ("trade", "trading", "market", "midas", "quant", "arbitrage", "metatrader"),
    "ai": ("ai", "agi", "nexus", "agent", "llm"),
    "blockchain": ("blockchain", "chain", "metachain", "crypto"),
    "finance": ("finance", "financial", "pay", "budget", "capital", "qe", "money"),
    "network": ("net", "shttps", "shhts", "messenger"),
    "runtime": ("os", "linux", "system", "motherboard"),
    "compression": ("compress", "gguf", "qr"),
    "apps": ("app", "super_app", "apps"),
    "science": ("science", "nuclear", "quantum"),
    "business": ("company", "business", "sales", "smm"),
}

CAPABILITY_RULES: dict[str, tuple[str, ...]] = {
    "trading.analyze": ("trade", "trading", "market", "midas", "quant"),
    "trading.execute": ("metatrader", "market_maker", "trademux"),
    "intelligence.infer": ("agi", "nexus", "llm", "ai_language"),
    "memory.search": ("nexus", "memory"),
    "computer.execute": ("meta_os", "turbo_linux", "motherboard"),
    "network.transport": ("net", "shttps", "shhts"),
    "blockchain.submit": ("blockchain", "chain", "metachain"),
    "finance.analyze": ("finance", "financial", "budget", "investment"),
    "business.operate": ("company", "autopilot", "agentic"),
    "app.interface": ("app", "apps"),
    "compression.encode": ("compress", "gguf", "qr"),
}

def api_get(path: str, token: str) -> Any:
    req = urllib.request.Request(API + path, headers={
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "synergy-ai-autopilot-capability-catalog",
    })
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.load(response)

def fetch_text(owner: str, repo: str, path: str, ref: str, token: str) -> str | None:
    try:
        value = api_get(f"/repos/{owner}/{repo}/contents/{path}?ref={urllib.parse.quote(ref)}", token)
    except Exception:
        return None
    if isinstance(value, dict) and value.get("encoding") == "base64":
        return base64.b64decode(value.get("content", "")).decode("utf-8", errors="replace")
    return None

def infer(name: str, readme: str) -> tuple[set[str], set[str], set[str]]:
    text = f"{name}\n{readme[:20000]}".lower()
    tags = {tag for tag, needles in TAG_RULES.items() if any(n in text for n in needles)}
    caps = {cap for cap, needles in CAPABILITY_RULES.items() if any(n in text for n in needles)}
    transports = set()
    for t in ("mcp", "openapi", "webhook", "grpc", "http", "shttps", "stcp", "cli"):
        if t in text:
            transports.add(t)
    return tags, caps, transports

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--owner", default="Shtenco")
    parser.add_argument("--out", default="catalog/repositories.json")
    args = parser.parse_args()
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        raise SystemExit("GITHUB_TOKEN or GH_TOKEN is required")

    repos = api_get(f"/users/{args.owner}/repos?per_page=100&type=owner&sort=full_name", token)
    catalog: list[dict[str, Any]] = []
    for repo in repos:
        name = repo["name"]
        branch = repo.get("default_branch") or "main"
        readme = ""
        for candidate in ("README.md", "README.MD", "readme.md"):
            value = fetch_text(args.owner, name, candidate, branch, token)
            if value is not None:
                readme = value
                break

        tags, capabilities, transports = infer(name, readme)
        plugin_ids: set[str] = set()
        evidence: set[str] = set()
        try:
            tree = api_get(f"/repos/{args.owner}/{name}/git/trees/{branch}?recursive=1", token)
            plugin_paths = [
                item["path"] for item in tree.get("tree", [])
                if item.get("type") == "blob" and PLUGIN_PATH_RE.search(item.get("path", ""))
            ][:50]
            for plugin_path in plugin_paths:
                raw = fetch_text(args.owner, name, plugin_path, branch, token)
                if not raw:
                    continue
                try:
                    manifest = json.loads(raw)
                except json.JSONDecodeError:
                    continue
                if manifest.get("schema") != "synergy.plugin.v1":
                    continue
                if manifest.get("id"):
                    plugin_ids.add(str(manifest["id"]))
                for cap in manifest.get("capabilities", []):
                    if isinstance(cap, dict) and cap.get("name"):
                        capabilities.add(str(cap["name"]))
                for endpoint in manifest.get("endpoints", []):
                    if isinstance(endpoint, dict) and endpoint.get("transport"):
                        transports.add(str(endpoint["transport"]))
                evidence.add(f"manifest:{plugin_path}")
        except urllib.error.HTTPError:
            pass

        if readme:
            evidence.add("README.md")
        catalog.append({
            "name": name,
            "full_name": repo["full_name"],
            "default_branch": branch,
            "visibility": repo.get("visibility", "private" if repo.get("private") else "public"),
            "archived": bool(repo.get("archived")),
            "tags": sorted(tags),
            "capabilities": sorted(capabilities),
            "transports": sorted(transports),
            "plugin_ids": sorted(plugin_ids),
            "evidence": sorted(evidence),
        })

    output = Path(args.out)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({
        "schema": "synergy.repository-capability-catalog.v1",
        "owner": args.owner,
        "repository_count": len(catalog),
        "repositories": catalog,
    }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(catalog)} repositories to {output}")

if __name__ == "__main__":
    import urllib.parse
    main()
