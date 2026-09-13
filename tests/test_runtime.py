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
    a=r.approve(t['id'],'owner'); assert a['status']=='approved'; e=r.execute(t['id']); assert e['status']=='denied'; assert 'EXTERNAL_EFFECTS' in e['error']

def test_authority_capabilities_are_hard_denied():
    r=rt()
    for cap in ['payment.execute','accounting.book','blockchain.submit','host.shell']:
        t=r.submit(cap,{}); assert t['status']=='denied'

def test_video_is_storyboard_not_fake_renderer():
    r=rt(); t=r.submit('video.storyboard',{'topic':'product demo','platform':'reels','duration_sec':30}); out=r.execute(t['id']); assert out['result']['render_authority']=='NONE'; assert len(out['result']['scenes'])>=3

def test_sales_objection_and_refusal():
    r=rt(); t=r.submit('sales.qualify',{'message':'Спасибо, не интересно'}); out=r.execute(t['id']); assert out['result']['stage']=='lost'; assert out['result']['recommended_next_action']=='stop'

def test_integration_status_accepts_alternative_token_names(monkeypatch):
    monkeypatch.setenv('TG_BOT_TOKEN','x'); r=rt(); assert r.biz.status()['telegram'] is True
    monkeypatch.delenv('TG_BOT_TOKEN'); monkeypatch.setenv('TELEGRAM_BOT_TOKEN','y'); assert r.biz.status()['telegram'] is True

def test_video_render_is_real_configured_boundary_and_fails_closed(monkeypatch):
    monkeypatch.delenv('AUTOPILOT_VIDEO_PROVIDER_URL',raising=False); monkeypatch.setenv('AUTOPILOT_EXTERNAL_EFFECTS','1')
    r=rt(); t=r.submit('video.render',{'prompt':'demo'}); assert t['status']=='needs_approval'; r.approve(t['id'],'owner'); out=r.execute(t['id']); assert out['status']=='failed'; assert 'not configured' in out['error']

def test_new_business_read_capabilities_are_registered():
    r=rt(); names={x['name'] for x in r.registry.list()}; assert {'crm.onec.orders','ads.facebook.campaigns','video.render'} <= names

def test_ceo_plan_only_returns_registered_capabilities():
    r=rt()
    for goal in ["запусти smm продажи cfo видео рекламу","проанализируй неизвестную бизнес задачу"]:
        t=r.submit('ceo.plan',{'goal':goal}); out=r.execute(t['id']); assert out['status']=='succeeded'; registered={x['name'] for x in r.registry.list()}; assert set(out['result']['capabilities']) <= registered

def test_revenue_validation_matches_auto_company_contract():
    r=rt(); t=r.submit('research.validate_revenue_opportunity',{'opportunity':{'title':'AI SMM contract','source':'market','url':'https://example.test/task','expected_revenue_usd':500,'estimated_cost_usd':100,'probability':0.6,'evidence':['buyer-posted']}})
    out=r.execute(t['id']); assert out['status']=='succeeded'; v=out['result']; assert v['next_action']=='prepare_bounded_proposal'; assert v['expected_margin_usd']==400; assert v['revenue_authority']=='NONE'
