from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path
from typing import Any

OPENAPI_NAMES = {
    "openapi.json", "swagger.json", "api.json",
}
MCP_NAMES = {
    "mcp.json", "mcp.config.json", "mcp-server.json",
}

def safe_read(path: Path, limit: int = 2_000_000) -> bytes:
    data = path.read_bytes()
    if len(data) > limit:
        raise ValueError(f"file too large: {path}")
    return data

def discover_openapi(path: Path) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    if path.name.lower() not in OPENAPI_NAMES:
        return out
    try:
        doc = json.loads(safe_read(path).decode("utf-8"))
    except Exception:
        return out
    if not isinstance(doc, dict) or not ("openapi" in doc or "swagger" in doc):
        return out
    servers = doc.get("servers") or []
    base_url = ""
    if isinstance(servers, list) and servers and isinstance(servers[0], dict):
        base_url = str(servers[0].get("url") or "")
    for route, item in (doc.get("paths") or {}).items():
        if not isinstance(item, dict):
            continue
        for method, op in item.items():
            if method.lower() not in {"get","post","put","patch","delete"} or not isinstance(op, dict):
                continue
            operation_id = op.get("operationId")
            if operation_id:
                out.append({
                    "surface": "openapi",
                    "file": str(path),
                    "method": method.upper(),
                    "path": route,
                    "operation_id": str(operation_id),
                    "base_url": base_url,
                    "mutating": method.lower() != "get",
                })
    return out

def discover_mcp(path: Path) -> list[dict[str, Any]]:
    if path.name.lower() not in MCP_NAMES:
        return []
    try:
        doc = json.loads(safe_read(path).decode("utf-8"))
    except Exception:
        return []
    out: list[dict[str, Any]] = []
    servers = doc.get("mcpServers") if isinstance(doc, dict) else None
    if isinstance(servers, dict):
        for name, spec in servers.items():
            if not isinstance(spec, dict):
                continue
            command = spec.get("command")
            args = spec.get("args") or []
            if isinstance(command, str) and isinstance(args, list) and all(isinstance(v, str) for v in args):
                out.append({
                    "surface": "mcp_stdio",
                    "file": str(path),
                    "name": str(name),
                    "command": command,
                    "args": args,
                    "requires_tool_discovery": True,
                })
    return out

def discover_cargo(path: Path) -> list[dict[str, Any]]:
    if path.name != "Cargo.toml":
        return []
    try:
        doc = tomllib.loads(safe_read(path).decode("utf-8"))
    except Exception:
        return []
    out = []
    for item in doc.get("bin") or []:
        if isinstance(item, dict) and item.get("name"):
            out.append({
                "surface":"cli",
                "file":str(path),
                "program":str(item["name"]),
                "source":"cargo-bin",
                "probe_args":["--help"],
            })
    package = doc.get("package") or {}
    if isinstance(package, dict) and package.get("name") and (path.parent / "src" / "main.rs").exists():
        out.append({
            "surface":"cli",
            "file":str(path),
            "program":str(package["name"]),
            "source":"cargo-main",
            "probe_args":["--help"],
        })
    return out

def discover_pyproject(path: Path) -> list[dict[str, Any]]:
    if path.name != "pyproject.toml":
        return []
    try:
        doc = tomllib.loads(safe_read(path).decode("utf-8"))
    except Exception:
        return []
    scripts = ((doc.get("project") or {}).get("scripts") or {})
    if not isinstance(scripts, dict):
        return []
    return [{
        "surface":"cli",
        "file":str(path),
        "program":str(name),
        "entrypoint":str(target),
        "source":"python-project-script",
        "probe_args":["--help"],
    } for name, target in scripts.items()]

def discover_package_json(path: Path) -> list[dict[str, Any]]:
    if path.name != "package.json":
        return []
    try:
        doc = json.loads(safe_read(path).decode("utf-8"))
    except Exception:
        return []
    value = doc.get("bin")
    if isinstance(value, str):
        return [{
            "surface":"cli","file":str(path),
            "program":str(doc.get("name") or path.parent.name),
            "entrypoint":value,"source":"node-bin","probe_args":["--help"],
        }]
    if isinstance(value, dict):
        return [{
            "surface":"cli","file":str(path),
            "program":str(name),"entrypoint":str(target),
            "source":"node-bin","probe_args":["--help"],
        } for name, target in value.items()]
    return []

def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("root")
    parser.add_argument("--out")
    parser.add_argument("--max-files", type=int, default=5000)
    args = parser.parse_args()

    root = Path(args.root).resolve()
    if not root.is_dir():
        raise SystemExit(f"not a directory: {root}")

    surfaces: list[dict[str, Any]] = []
    scanned = 0
    for path in root.rglob("*"):
        if scanned >= args.max_files:
            break
        if not path.is_file():
            continue
        if any(part in {".git","node_modules","target",".venv","venv","dist","build"} for part in path.parts):
            continue
        scanned += 1
        surfaces.extend(discover_openapi(path))
        surfaces.extend(discover_mcp(path))
        surfaces.extend(discover_cargo(path))
        surfaces.extend(discover_pyproject(path))
        surfaces.extend(discover_package_json(path))

    result = {
        "schema":"synergy.plugin-discovery.v1",
        "root":str(root),
        "scanned_files":scanned,
        "surfaces":surfaces,
        "security":{
            "executed_code":False,
            "network_access":False,
            "note":"Discovery is static-only. Qualification/execution is a separate gate."
        }
    }
    raw = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if args.out:
        Path(args.out).write_text(raw, encoding="utf-8")
    else:
        print(raw, end="")

if __name__ == "__main__":
    main()
