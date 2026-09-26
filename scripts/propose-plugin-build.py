from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

READ_WORDS = {"read","get","list","search","find","query","fetch","inspect","status","health","analyze"}
WRITE_WORDS = {"create","update","delete","send","publish","write","execute","run","post","put","patch"}

def tokens(value: str) -> set[str]:
    return {part for part in re.split(r"[^a-zA-Z0-9_]+", value.lower()) if len(part) >= 2}

def score(surface: dict[str, Any], capability: str) -> int:
    wanted=tokens(capability)
    hay=" ".join(str(surface.get(k,"")) for k in ("operation_id","path","program","name","source","surface")).lower()
    found=tokens(hay)
    result=10*len(wanted & found)
    if surface.get("surface")=="openapi":
        result+=20
        if not surface.get("mutating"): result+=10
    elif surface.get("surface")=="mcp_stdio":
        result+=12
    elif surface.get("surface")=="cli":
        result+=5
    return result

def risk_for(surface: dict[str,Any]) -> tuple[str,bool]:
    if surface.get("surface")=="openapi":
        if surface.get("mutating"):
            return "High", True
        return "Medium", False
    return "Low", False

def main()->None:
    p=argparse.ArgumentParser()
    p.add_argument("discovery")
    p.add_argument("--capability",required=True)
    p.add_argument("--plugin-id",required=True)
    p.add_argument("--name",required=True)
    p.add_argument("--base-url")
    p.add_argument("--tool")
    p.add_argument("--out")
    args=p.parse_args()

    discovery=json.loads(Path(args.discovery).read_text(encoding="utf-8"))
    surfaces=discovery.get("surfaces") or []
    ranked=sorted(surfaces,key=lambda item:(-score(item,args.capability),str(item.get("file",""))))
    proposals=[]
    root=Path(discovery.get("root") or ".")
    for surface in ranked[:20]:
        kind=surface.get("surface")
        risk,approval=risk_for(surface)
        proposal={
            "score":score(surface,args.capability),
            "surface":surface,
            "ready_to_build":False,
            "build_request":None,
            "reason":None,
        }
        if kind=="openapi":
            base=args.base_url or surface.get("base_url") or ""
            doc_path=Path(str(surface.get("file","")))
            if not doc_path.is_absolute(): doc_path=root/doc_path
            try:
                document=json.loads(doc_path.read_text(encoding="utf-8"))
            except Exception as exc:
                proposal["reason"]=f"cannot read OpenAPI document: {exc}"
            else:
                if not base:
                    proposal["reason"]="OpenAPI base URL is missing; pass --base-url"
                else:
                    proposal["ready_to_build"]=True
                    proposal["build_request"]={
                        "plugin_id":args.plugin_id,
                        "display_name":args.name,
                        "capability":args.capability,
                        "source":{
                            "type":"open_api",
                            "base_url":base,
                            "operation_id":surface["operation_id"],
                            "document":document,
                        },
                        "risk":risk,
                        "requires_approval":approval,
                        "tags":["generated","openapi"],
                    }
        elif kind=="mcp_stdio":
            tool=args.tool
            if not tool:
                proposal["reason"]="MCP tool must be resolved from sandbox tools/list; pass --tool after discovery"
            else:
                proposal["ready_to_build"]=True
                proposal["build_request"]={
                    "plugin_id":args.plugin_id,
                    "display_name":args.name,
                    "capability":args.capability,
                    "source":{
                        "type":"mcp_stdio",
                        "command":surface["command"],
                        "args":surface.get("args") or [],
                        "tool":tool,
                    },
                    "risk":risk,
                    "requires_approval":approval,
                    "tags":["generated","mcp"],
                }
        elif kind=="cli":
            proposal["reason"]="CLI entrypoint discovered, but capability-to-argv/stdin semantics require explicit mapping before build"
        proposals.append(proposal)

    result={
        "schema":"synergy.plugin-proposals.v1",
        "capability":args.capability,
        "plugin_id":args.plugin_id,
        "proposals":proposals,
        "security":{
            "executed_code":False,
            "note":"Proposal generation is static-only. MCP tool discovery and all candidate execution require sandbox qualification."
        }
    }
    raw=json.dumps(result,ensure_ascii=False,indent=2)+"\n"
    if args.out: Path(args.out).write_text(raw,encoding="utf-8")
    else: print(raw,end="")

if __name__=="__main__":
    main()
