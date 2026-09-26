from __future__ import annotations
from typing import Any
from shttps.rpc import RPCDispatcher
from .runtime import AutopilotRuntime

def build_dispatcher(runtime: AutopilotRuntime) -> RPCDispatcher:
    d=RPCDispatcher()
    def submit(p:dict[str,Any],ctx:dict[str,Any]):
        capability=str(p.get("capability") or "")
        if not capability: raise ValueError("capability required")
        return runtime.submit(capability,dict(p.get("params") or {}),p.get("idempotency_key"))
    def status(p,ctx): return runtime.store.get(str(p["task_id"]))
    def approve(p,ctx): return runtime.approve(str(p["task_id"]),str(p.get("actor") or ctx["peer_node_id"]))
    def execute(p,ctx): return runtime.execute(str(p["task_id"]))
    def catalog(p,ctx): return {"schema":"synergy.capability-catalog/v1","capabilities":runtime.registry.list(),"transport":"SHTTPS/1"}
    d.register("autopilot","submit",submit); d.register("autopilot","status",status); d.register("autopilot","approve",approve); d.register("autopilot","execute",execute); d.register("autopilot","capabilities",catalog)
    return d
