$ErrorActionPreference = "Stop"

Write-Host "== SYNERGY AI AUTOPILOT M5-M8 verification =="

Write-Host "[1/9] Rust format"
cargo fmt --all -- --check

Write-Host "[2/9] Rust clippy"
cargo clippy --workspace --all-targets -- -D warnings

Write-Host "[3/9] Rust tests"
cargo test --workspace

Write-Host "[4/9] Browser bridge syntax"
node --check tools/browser/bridge.mjs

Write-Host "[5/9] Business runtime"
python -m pip install -e "./sidecars/business-runtime[test]"
python -m pytest sidecars/business-runtime/tests -q

Write-Host "[6/9] NEXUS bridge"
python -m pip install -e ./sidecars/nexus-bridge
python -c "from nexus_bridge.app import app; assert app.title.startswith('SYNERGY NEXUS')"

Write-Host "[7/9] Connector Gateway"
python -m pip install -e ./sidecars/connector-gateway
python -c "from connector_gateway.app import app; assert app.title.startswith('SYNERGY Connector')"

Write-Host "[8/9] Catalog + schemas + Python compile"
python -m json.tool catalog/repositories.json > $null
python -m json.tool schemas/synergy.plugin.v1.schema.json > $null
python -m py_compile scripts/discover-plugin-surfaces.py scripts/propose-plugin-build.py
python -m compileall -q sidecars/business-runtime/src sidecars/nexus-bridge/src sidecars/connector-gateway/src

Write-Host "[9/9] M8 static self-build smoke"
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

Write-Host "SYNERGY_AUTOPILOT_M5_M8_VERIFICATION_PASS"
