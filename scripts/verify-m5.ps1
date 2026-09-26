$ErrorActionPreference = "Stop"

Write-Host "== SYNERGY AI AUTOPILOT M5 verification =="

Write-Host "[1/6] Rust format"
cargo fmt --all -- --check

Write-Host "[2/6] Rust clippy"
cargo clippy --workspace --all-targets -- -D warnings

Write-Host "[3/6] Rust tests"
cargo test --workspace

Write-Host "[4/6] Browser bridge syntax"
node --check tools/browser/bridge.mjs

Write-Host "[5/6] Business runtime"
python -m pip install -e "./sidecars/business-runtime[test]"
python -m pytest sidecars/business-runtime/tests -q

Write-Host "[6/7] NEXUS bridge"
python -m pip install -e ./sidecars/nexus-bridge
python -c "from nexus_bridge.app import app; assert app.title.startswith('SYNERGY NEXUS')"

Write-Host "[7/7] Connector Gateway + capability catalog"
python -m pip install -e ./sidecars/connector-gateway
python -c "from connector_gateway.app import app; assert app.title.startswith('SYNERGY Connector')"
python -m json.tool catalog/repositories.json > $null
python -m compileall -q sidecars/business-runtime/src sidecars/nexus-bridge/src sidecars/connector-gateway/src

Write-Host "M6_M7_LOCAL_VERIFICATION_PASS"
