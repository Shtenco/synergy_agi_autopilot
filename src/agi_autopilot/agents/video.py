from __future__ import annotations

class VideoAgent:
    """Legacy sources prove video/reels/story publishing, not generative rendering.
    This agent creates production plans; rendering is a separately configured paid capability.
    """
    def storyboard(self,p:dict)->dict:
        topic=str(p.get("topic","продукт")); platform=str(p.get("platform","reels")); duration=max(5,min(180,int(p.get("duration_sec",30))))
        scenes=max(3,min(10,int(p.get("scenes",5))))
        per=max(1,duration//scenes)
        out=[]
        for i in range(scenes):
            role="hook" if i==0 else "cta" if i==scenes-1 else "value"
            out.append({"scene":i+1,"seconds":per,"role":role,"visual":f"{role} visual for {topic}","voiceover":f"{role}: {topic}","on_screen_text":topic if i==0 else role})
        return {"schema":"synergy.ai-video-storyboard/v1","platform":platform,"duration_sec":duration,"scenes":out,"render_authority":"NONE"}

    def render_request(self,p:dict)->dict:
        return {"schema":"synergy.ai-video-render-request/v1","provider":p.get("provider"),"storyboard":p.get("storyboard"),"estimated_cost_usd":float(p.get("estimated_cost_usd",0) or 0),"note":"Provider execution is external/cost-bearing and must be explicitly configured and approved."}
