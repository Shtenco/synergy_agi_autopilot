import os,tempfile
from agi_autopilot.runtime import AutopilotRuntime

def rt():
    f=tempfile.NamedTemporaryFile(suffix='.db',delete=False); f.close(); return AutopilotRuntime(f.name)

def test_catalog_contains_legacy_business_agents():
    r=rt(); names={x['name'] for x in r.registry.list()}
    for n in ['cfo.analyze_budget','smm.content_plan','sales.qualify','video.storyboard','crm.bitrix24.leads','instagram.publish','ads.yandex.campaigns']:
        assert n in names

def test_cfo_is_deterministic_and_non_authoritative():
    r=rt(); t=r.submit('cfo.analyze_budget',{'revenue_usd':1000,'costs':{'ads':200,'ops':300},'ad_spend_usd':200}); assert t['status']=='ready'
    out=r.execute(t['id']); assert out['status']=='succeeded'; assert out['result']['operating_margin_usd']==500; assert out['result']['authority']=='ANALYTICAL_PROPOSAL_ONLY'

def test_external_write_requires_approval_and_effects_switch(monkeypatch):
    monkeypatch.delenv('AUTOPILOT_EXTERNAL_EFFECTS',raising=False); r=rt(); t=r.submit('telegram.send_message',{'chat_id':'1','text':'x'}); assert t['status']=='needs_approval'
    r.approve(t['id'],'owner'); e=r.execute(t['id']); assert e['status']=='denied'; assert 'EXTERNAL_EFFECTS' in e['error']

def test_authority_capabilities_are_hard_denied():
    r=rt()
    for cap in ['payment.execute','accounting.book','blockchain.submit','host.shell']:
        assert r.submit(cap,{})['status']=='denied'

def test_video_is_storyboard_not_fake_renderer():
    r=rt(); out=r.execute(r.submit('video.storyboard',{'topic':'product demo','platform':'reels','duration_sec':30})['id']); assert out['result']['render_authority']=='NONE'

def test_sales_objection_and_refusal():
    r=rt(); out=r.execute(r.submit('sales.qualify',{'message':'Спасибо, не интересно'})['id']); assert out['result']['stage']=='lost'

def test_integration_status_accepts_alternative_token_names(monkeypatch):
    monkeypatch.setenv('TG_BOT_TOKEN','x'); r=rt(); assert r.biz.status()['telegram'] is True

def test_video_render_is_real_configured_boundary_and_fails_closed(monkeypatch):
    monkeypatch.delenv('AUTOPILOT_VIDEO_PROVIDER_URL',raising=False); monkeypatch.setenv('AUTOPILOT_EXTERNAL_EFFECTS','1'); r=rt(); t=r.submit('video.render',{'prompt':'demo'}); r.approve(t['id'],'owner'); out=r.execute(t['id']); assert out['status']=='failed'

def test_new_business_read_capabilities_are_registered():
    r=rt(); names={x['name'] for x in r.registry.list()}; assert {'crm.onec.orders','ads.facebook.campaigns','video.render'} <= names

def test_ceo_plan_only_returns_registered_capabilities():
    r=rt(); registered={x['name'] for x in r.registry.list()}
    for goal in ['запусти smm продажи cfo видео рекламу','проанализируй неизвестную бизнес задачу','разработай код API']:
        out=r.execute(r.submit('ceo.plan',{'goal':goal})['id']); assert set(out['result']['capabilities']) <= registered

def test_revenue_validation_matches_auto_company_contract():
    r=rt(); out=r.execute(r.submit('research.validate_revenue_opportunity',{'opportunity':{'title':'AI SMM contract','source':'market','url':'https://example.test/task','expected_revenue_usd':500,'estimated_cost_usd':100,'probability':0.6,'evidence':['buyer-posted']}})['id']); assert out['result']['expected_margin_usd']==400

def test_coder_and_verifier_restore_safe_legacy_roles():
    r=rt(); names={x['name'] for x in r.registry.list()}; assert {'coder.change_plan','verifier.check_evidence'} <= names
    c=r.execute(r.submit('coder.change_plan',{'repository':'Shtenco/x','objective':'add API'})['id']); assert c['result']['shell_authority']=='NONE'
    v=r.execute(r.submit('verifier.check_evidence',{'required':['buyer','budget'],'evidence':{'buyer':'x'}})['id']); assert v['result']['verified'] is False; assert v['result']['missing']==['budget']
