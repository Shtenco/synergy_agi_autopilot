$ErrorActionPreference = "Stop"

Write-Host "== SYNERGY AI AUTOPILOT M5-M10 verification =="

Write-Host "[1/11] Rust format"
cargo fmt --all -- --check

Write-Host "[2/11] Rust clippy"
cargo clippy --workspace --all-targets -- -D warnings

Write-Host "[3/11] Rust tests"
cargo test --workspace

Write-Host "[4/11] Browser bridge syntax"
node --check tools/browser/bridge.mjs

Write-Host "[5/11] Business runtime"
python -m pip install -e "./sidecars/business-runtime[test]"
python -m pytest sidecars/business-runtime/tests -q

Write-Host "[6/11] NEXUS bridge"
python -m pip install -e ./sidecars/nexus-bridge
python -c "from nexus_bridge.app import app; assert app.title.startswith('SYNERGY NEXUS')"

Write-Host "[7/11] Connector Gateway"
python -m pip install -e ./sidecars/connector-gateway
python -c "from connector_gateway.app import app; assert app.title.startswith('SYNERGY Connector')"

Write-Host "[8/11] Catalog + schemas + Python compile"
python -m json.tool catalog/repositories.json > $null
python -m json.tool schemas/synergy.plugin.v1.schema.json > $null
python -m json.tool schemas/synergy.cli-capabilities.v1.schema.json > $null
python -m json.tool schemas/synergy.cognitive-plan.v1.schema.json > $null
python -m json.tool examples/m10/sales-to-file-cognitive-plan.json > $null
Get-ChildItem plugins -Recurse -Filter plugin.json | ForEach-Object { python -m json.tool $_.FullName > $null }
python -m py_compile scripts/discover-plugin-surfaces.py scripts/propose-plugin-build.py
python -m compileall -q sidecars/business-runtime/src sidecars/nexus-bridge/src sidecars/connector-gateway/src

Write-Host "[9/11] M8 static self-build smoke"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("synergy-m8-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
try {
    $openapi = @'
{
  "openapi": "3.0.0",
  "servers": [{"url": "https://api.example.com"}],
  "paths": {
    "/status": {
      "get": {"operationId": "getStatus"}
    }
  }
}
'@
    Set-Content -Path (Join-Path $tmp "openapi.json") -Value $openapi -Encoding UTF8

    $discovery = Join-Path $tmp "discovery.json"
    $proposals = Join-Path $tmp "proposals.json"

    python scripts/discover-plugin-surfaces.py $tmp --out $discovery
    python scripts/propose-plugin-build.py $discovery --capability custom.status.read --plugin-id generated.smoke.status --name "Smoke Status" --base-url https://api.example.com --out $proposals
    python -c "import json,sys; d=json.load(open(sys.argv[1], encoding='utf-8')); assert any(p.get('ready_to_build') for p in d['proposals'])" $proposals
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

Write-Host "[10/11] M9 declarative CLI smoke"
$tmpCli = Join-Path ([System.IO.Path]::GetTempPath()) ("synergy-m9-cli-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $tmpCli | Out-Null
try {
    $descriptor = @'
{
  "schema":"synergy.cli-capabilities.v1",
  "commands":[{
    "program":"lead-tool",
    "capability":"crm.read",
    "args":["--json"],
    "risk":"Medium",
    "input_schema":{
      "type":"object",
      "properties":{"query":{"type":"string"}},
      "required":["query"],
      "additionalProperties":false
    },
    "output_schema":{
      "type":"object",
      "properties":{"items":{"type":"array","items":{"type":"object"}}},
      "required":["items"],
      "additionalProperties":false
    }
  }]
}
'@
    Set-Content -Path (Join-Path $tmpCli "synergy.cli.json") -Value $descriptor -Encoding UTF8
    $discoveryCli = Join-Path $tmpCli "discovery.json"
    $proposalsCli = Join-Path $tmpCli "proposals.json"
    python scripts/discover-plugin-surfaces.py $tmpCli --out $discoveryCli
    python scripts/propose-plugin-build.py $discoveryCli --capability crm.read --plugin-id generated.smoke.crm --name "Smoke CRM" --out $proposalsCli
    python -c "import json,sys; d=json.load(open(sys.argv[1],encoding='utf-8')); p=[x for x in d['proposals'] if x.get('ready_to_build')][0]; assert p['build_request']['input_contract']['required']==['query']; assert p['build_request']['output_contract']['required']==['items']" $proposalsCli
}
finally {
    Remove-Item -Recurse -Force $tmpCli -ErrorAction SilentlyContinue
}

Write-Host "[11/11] M10 verified cognitive DAG smoke"
cargo run -p open-autopilot -- cognitive-plan-verify --proposal examples/m10/sales-to-file-cognitive-plan.json

Write-Host "SYNERGY_AUTOPILOT_M5_M10_VERIFICATION_PASS"
