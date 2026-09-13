from __future__ import annotations
import os, smtplib
from email.message import EmailMessage
from .http import request_json

class BusinessIntegrations:
    """Clean-room wrappers derived from the verified legacy integration inventory.
    Secrets are read only from environment; no uploaded key/session files are imported.
    """
    def status(self)->dict:
        spec={
          "telegram":{"any_of":["TG_BOT_TOKEN","TELEGRAM_BOT_TOKEN"]},
          "vk":{"any_of":["VK_TOKEN","VK_API_TOKEN"],"all_of":["VK_GROUP_ID"]},
          "facebook":{"any_of":["FB_TOKEN","FACEBOOK_TOKEN"],"all_of":["FB_PAGE_ID"]},
          "instagram":{"any_of":["INSTAGRAM_TOKEN","FB_TOKEN"],"all_of":["IG_BUSINESS_ID"]},
          "whatsapp":{"all_of":["WHATSAPP_TOKEN","WHATSAPP_PHONE_ID"]},
          "discord":{"all_of":["DISCORD_WEBHOOK"]},
          "yandex_direct":{"all_of":["YANDEX_DIRECT_TOKEN"]},
          "bitrix24":{"all_of":["B24_URL","B24_TOKEN"]},
          "amocrm":{"all_of":["AMOCRM_URL","AMOCRM_TOKEN"]},
          "retailcrm":{"all_of":["RETAILCRM_URL","RETAILCRM_KEY"]},
          "onec":{"all_of":["ONEC_URL","ONEC_USER","ONEC_PASS"]},
          "yandex_business":{"all_of":["YANDEX_BIZ_TOKEN"]},
          "youtube":{"all_of":["YOUTUBE_API_KEY"]},
          "email":{"all_of":["SMTP_HOST","SMTP_USER","SMTP_PASS"]},
          "facebook_ads":{"all_of":["FB_ADS_TOKEN","FB_ADS_ACCOUNT_ID"]},
        }
        out={}
        for name,rules in spec.items():
            any_of=rules.get("any_of",[]); all_of=rules.get("all_of",[])
            out[name]=(not any_of or any(bool(os.getenv(k)) for k in any_of)) and all(bool(os.getenv(k)) for k in all_of)
        return out

    def telegram_send(self,p):
        token=os.getenv("TG_BOT_TOKEN") or os.getenv("TELEGRAM_BOT_TOKEN") or ""
        if not token: raise RuntimeError("Telegram token not configured")
        return request_json("POST",f"https://api.telegram.org/bot{token}/sendMessage",json_body={"chat_id":p["chat_id"],"text":p["text"],"parse_mode":p.get("parse_mode","HTML")})

    def telegram_video(self,p):
        token=os.getenv("TG_BOT_TOKEN") or os.getenv("TELEGRAM_BOT_TOKEN") or ""
        if not token: raise RuntimeError("Telegram token not configured")
        return request_json("POST",f"https://api.telegram.org/bot{token}/sendVideo",json_body={"chat_id":p["chat_id"],"video":p["video_url"],"caption":p.get("caption","")})

    def vk_wall_post(self,p):
        token=os.getenv("VK_TOKEN") or os.getenv("VK_API_TOKEN") or ""; gid=os.getenv("VK_GROUP_ID","")
        if not token or not gid: raise RuntimeError("VK not configured")
        return request_json("POST","https://api.vk.com/method/wall.post",form={"access_token":token,"v":"5.199","owner_id":f"-{gid.lstrip('-')}","from_group":1,"message":p["message"],"attachments":p.get("attachments","")})

    def vk_stats(self,p):
        token=os.getenv("VK_TOKEN") or os.getenv("VK_API_TOKEN") or ""; gid=os.getenv("VK_GROUP_ID","")
        if not token or not gid: raise RuntimeError("VK not configured")
        return request_json("GET","https://api.vk.com/method/stats.get",params={"access_token":token,"v":"5.199","group_id":gid,"interval":p.get("interval","day")})

    def facebook_page_post(self,p):
        token=os.getenv("FB_TOKEN") or os.getenv("FACEBOOK_TOKEN") or ""; page=p.get("page_id") or os.getenv("FB_PAGE_ID","")
        if not token or not page: raise RuntimeError("Facebook Page not configured")
        return request_json("POST",f"https://graph.facebook.com/v22.0/{page}/feed",form={"access_token":token,"message":p["message"]})

    def instagram_publish(self,p):
        token=os.getenv("INSTAGRAM_TOKEN") or os.getenv("FB_TOKEN") or ""; ig=p.get("account_id") or os.getenv("IG_BUSINESS_ID","")
        if not token or not ig: raise RuntimeError("Instagram Business not configured")
        body={"access_token":token,"caption":p.get("caption","")}
        media_type=p.get("media_type","IMAGE").upper()
        if media_type in {"VIDEO","REELS","STORIES"}: body.update({"video_url":p["media_url"],"media_type":media_type})
        else: body["image_url"]=p["media_url"]
        created=request_json("POST",f"https://graph.facebook.com/v22.0/{ig}/media",form=body)
        cid=created.get("id")
        if not cid: return created
        return request_json("POST",f"https://graph.facebook.com/v22.0/{ig}/media_publish",form={"access_token":token,"creation_id":cid})

    def whatsapp_text(self,p):
        token=os.getenv("WHATSAPP_TOKEN",""); phone=os.getenv("WHATSAPP_PHONE_ID","")
        if not token or not phone: raise RuntimeError("WhatsApp not configured")
        return request_json("POST",f"https://graph.facebook.com/v22.0/{phone}/messages",headers={"Authorization":f"Bearer {token}"},json_body={"messaging_product":"whatsapp","to":p["to"],"type":"text","text":{"body":p["text"]}})

    def discord_send(self,p):
        url=os.getenv("DISCORD_WEBHOOK","")
        if not url: raise RuntimeError("Discord webhook not configured")
        return request_json("POST",url,json_body={"content":p["text"]})

    def bitrix_leads(self,p):
        base=os.getenv("B24_URL","").rstrip("/"); token=os.getenv("B24_TOKEN","")
        if not base or not token: raise RuntimeError("Bitrix24 not configured")
        return request_json("POST",f"{base}/rest/{token}/crm.lead.list.json",json_body={"filter":p.get("filter",{}),"select":p.get("select",["ID","TITLE","NAME","STATUS_ID"])})

    def bitrix_lead_add(self,p):
        base=os.getenv("B24_URL","").rstrip("/"); token=os.getenv("B24_TOKEN","")
        if not base or not token: raise RuntimeError("Bitrix24 not configured")
        return request_json("POST",f"{base}/rest/{token}/crm.lead.add.json",json_body={"fields":p["fields"]})

    def amocrm_leads(self,p):
        base=os.getenv("AMOCRM_URL","").rstrip("/"); token=os.getenv("AMOCRM_TOKEN","")
        if not base or not token: raise RuntimeError("amoCRM not configured")
        return request_json("GET",f"{base}/api/v4/leads",headers={"Authorization":f"Bearer {token}"},params={"limit":int(p.get("limit",50))})

    def retail_orders(self,p):
        base=os.getenv("RETAILCRM_URL","").rstrip("/"); key=os.getenv("RETAILCRM_KEY","")
        if not base or not key: raise RuntimeError("RetailCRM not configured")
        return request_json("GET",f"{base}/api/v5/orders",params={"apiKey":key,"limit":int(p.get("limit",50))})

    def onec_orders(self,p):
        base=os.getenv("ONEC_URL","").rstrip("/"); user=os.getenv("ONEC_USER",""); pwd=os.getenv("ONEC_PASS","")
        if not base or not user or not pwd: raise RuntimeError("1C HTTP service not configured")
        import base64
        auth=base64.b64encode(f"{user}:{pwd}".encode()).decode()
        path=str(p.get("path","orders")).lstrip("/")
        return request_json("GET",f"{base}/{path}",headers={"Authorization":f"Basic {auth}"},params=p.get("params",{}))

    def yandex_direct_campaigns(self,p):
        token=os.getenv("YANDEX_DIRECT_TOKEN","")
        if not token: raise RuntimeError("Yandex Direct not configured")
        headers={"Authorization":f"Bearer {token}","Accept-Language":"ru","Content-Type":"application/json; charset=utf-8"}
        login=os.getenv("YANDEX_DIRECT_LOGIN","")
        if login: headers["Client-Login"]=login
        return request_json("POST","https://api.direct.yandex.com/json/v5/campaigns",headers=headers,json_body={"method":"get","params":{"SelectionCriteria":{},"FieldNames":["Id","Name","State","Status"]}})

    def facebook_ads_campaigns(self,p):
        token=os.getenv("FB_ADS_TOKEN",""); account=os.getenv("FB_ADS_ACCOUNT_ID","")
        if not token or not account: raise RuntimeError("Facebook Ads not configured")
        account=account if account.startswith("act_") else f"act_{account}"
        return request_json("GET",f"https://graph.facebook.com/v22.0/{account}/campaigns",params={"access_token":token,"fields":"id,name,status,objective,effective_status","limit":min(100,int(p.get("limit",50)))})

    def youtube_search(self,p):
        key=os.getenv("YOUTUBE_API_KEY","")
        if not key: raise RuntimeError("YouTube API key not configured")
        return request_json("GET","https://www.googleapis.com/youtube/v3/search",params={"part":"snippet","type":"video","maxResults":min(25,int(p.get("limit",10))),"q":p["query"],"key":key})

    def email_send(self,p):
        host=os.getenv("SMTP_HOST",""); user=os.getenv("SMTP_USER",""); pwd=os.getenv("SMTP_PASS",""); port=int(os.getenv("SMTP_PORT","465"))
        if not host or not user or not pwd: raise RuntimeError("SMTP not configured")
        m=EmailMessage(); m["From"]=user; m["To"]=p["to"]; m["Subject"]=p["subject"]; m.set_content(p["body"])
        with smtplib.SMTP_SSL(host,port,timeout=25) as s: s.login(user,pwd); s.send_message(m)
        return {"ok":True,"to":p["to"]}
