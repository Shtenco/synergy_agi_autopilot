from agi_autopilot.runtime import AutopilotRuntime
from agi_autopilot.shttps_rpc import build_dispatcher
from shttps.rpc import RPCRequest

def test_shttps_dispatcher_preserves_autopilot_policy(tmp_path):
    r=AutopilotRuntime(str(tmp_path/"a.db")); d=build_dispatcher(r); response=d.dispatch(RPCRequest("autopilot","submit",{"capability":"cfo.analyze_budget","params":{"revenue":100,"cost":40}}),{"peer_node_id":"company","application_authority":False}); assert response.ok and response.result["status"]=="ready"; tid=response.result["id"]; executed=d.dispatch(RPCRequest("autopilot","execute",{"task_id":tid}),{"peer_node_id":"company","application_authority":False}); assert executed.ok and executed.result["status"]=="succeeded"
def test_shttps_unknown_method_denied(tmp_path):
    d=build_dispatcher(AutopilotRuntime(str(tmp_path/"a.db"))); response=d.dispatch(RPCRequest("autopilot","shell",{}),{"peer_node_id":"company"}); assert not response.ok and response.error=="RPC_METHOD_NOT_ALLOWED"
