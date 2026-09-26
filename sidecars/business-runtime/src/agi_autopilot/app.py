from __future__ import annotations
import os
from fastapi import FastAPI,Header,HTTPException
from pydantic import BaseModel,Field
from .runtime import AutopilotRuntime

runtime=AutopilotRuntime()
app=FastAPI(title="SYNERGY AUTOPILOT Capability Runtime",version="1.0.0a1")

def guard(auth:str|None):
    token=os.getenv("AUTOPILOT_API_TOKEN","")
    if token and auth!=f"Bearer {token}": raise HTTPException(401,"invalid token")

class TaskIn(BaseModel):
    capability:str|None=None
    params:dict=Field(default_factory=dict)
    goal:str|None=None
    context:dict=Field(default_factory=dict)
    metadata:dict=Field(default_factory=dict)
    idempotency_key:str|None=None
class ApprovalIn(BaseModel): actor:str

@app.get("/health")
def health(): return {"ok":True,"service":"agi_autopilot","schema":"synergy.capability.v1","external_effects":runtime.policy.external_effects_enabled()}
@app.get("/v1/capabilities")
def capabilities(authorization:str|None=Header(default=None)):
    guard(authorization); return {"schema":"synergy.capability-catalog/v1","capabilities":runtime.registry.list(),"integrations":runtime.biz.status(),"models":runtime.models.status(),"video_provider":runtime.video_provider.status()}
@app.post("/v1/tasks")
def create_task(body:TaskIn,authorization:str|None=Header(default=None)):
    guard(authorization)
    try:
        if body.capability: t=runtime.submit(body.capability,body.params,body.idempotency_key)
        elif body.goal: t=runtime.submit_goal(body.goal,body.context,body.metadata)
        else: raise HTTPException(422,"capability or goal required")
        return t
    except KeyError as e: raise HTTPException(404,f"unknown capability: {e.args[0]}")
@app.get("/v1/tasks/{task_id}")
def get_task(task_id:str,authorization:str|None=Header(default=None)):
    guard(authorization)
    try:return runtime.store.get(task_id)
    except KeyError:raise HTTPException(404,"task not found")
@app.post("/v1/tasks/{task_id}/approve")
def approve(task_id:str,body:ApprovalIn,authorization:str|None=Header(default=None)):
    guard(authorization)
    try:return runtime.approve(task_id,body.actor)
    except KeyError:raise HTTPException(404,"task not found")
    except ValueError as e:raise HTTPException(409,str(e))
@app.post("/v1/tasks/{task_id}/execute")
def execute(task_id:str,authorization:str|None=Header(default=None)):
    guard(authorization)
    try:return runtime.execute(task_id)
    except KeyError:raise HTTPException(404,"task not found")
