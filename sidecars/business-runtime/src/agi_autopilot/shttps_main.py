from __future__ import annotations
import asyncio
from shttps.bootstrap import rpc_server_from_env
from .runtime import AutopilotRuntime
from .shttps_rpc import build_dispatcher

def build_server():
    runtime=AutopilotRuntime()
    return rpc_server_from_env("AUTOPILOT",build_dispatcher(runtime),host_default="127.0.0.1",port_default=8120,roles=("AUTOPILOT",))
async def amain(): await build_server().serve_forever()
def main(): asyncio.run(amain())
if __name__=="__main__": main()
