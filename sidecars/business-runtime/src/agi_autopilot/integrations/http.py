from __future__ import annotations
import json, urllib.parse, urllib.request


def request_json(method:str,url:str,*,headers:dict|None=None,params:dict|None=None,json_body:dict|None=None,form:dict|None=None,timeout:int=25):
    if params: url += ("&" if "?" in url else "?") + urllib.parse.urlencode(params, doseq=True)
    data=None; h=dict(headers or {})
    if json_body is not None: data=json.dumps(json_body).encode(); h.setdefault("Content-Type","application/json")
    elif form is not None: data=urllib.parse.urlencode(form,doseq=True).encode(); h.setdefault("Content-Type","application/x-www-form-urlencoded")
    req=urllib.request.Request(url,data=data,headers=h,method=method.upper())
    with urllib.request.urlopen(req,timeout=timeout) as r:
        raw=r.read().decode(); return json.loads(raw) if raw else {"status":r.status}
