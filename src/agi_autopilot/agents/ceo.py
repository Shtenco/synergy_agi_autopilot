from __future__ import annotations

class CEOAgent:
    def plan(self,p:dict)->dict:
        goal=str(p.get("goal","")).lower(); caps=[]
        if any(x in goal for x in ["smm","контент","соц"]): caps += ["smm.content_plan","smm.post_brief"]
        if any(x in goal for x in ["продаж","sales","лид"]): caps += ["sales.qualify","sales.compose_offer"]
        if any(x in goal for x in ["cfo","финанс","бюджет","roi"]): caps += ["cfo.analyze_budget"]
        if any(x in goal for x in ["video","видео","reels","shorts"]): caps += ["video.storyboard"]
        if any(x in goal for x in ["реклам","ads"]): caps += ["ads.yandex.campaigns"]
        if not caps: caps=["integration.status"]
        return {"schema":"synergy.autopilot-plan/v1","goal":p.get("goal",""),"capabilities":list(dict.fromkeys(caps)),"execution_authority":"NONE"}
