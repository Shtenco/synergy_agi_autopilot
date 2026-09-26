param(
    [Parameter(Mandatory=$true)][string]$BlockchainDir,
    [string]$AutoCompanyDir = "",
    [switch]$WithRemote,
    [string]$ShttpsDir = ""
)

$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..").Path

Write-Host "== Build P7 Node Commander + Agent Fabric =="
cargo build --manifest-path "$BlockchainDir\node-runtime\Cargo.toml" --release --bin synergy-node --bin agent-fabric-stdio
$p7Bin = Join-Path $BlockchainDir "node-runtime\target\release"
$env:PATH = "$p7Bin;$env:PATH"

Write-Host "== Install local sidecars =="
python -m pip install -e "$repo\sidecars\business-runtime[test]"
python -m pip install -e "$repo\sidecars\nexus-bridge"
python -m pip install -e "$repo\sidecars\connector-gateway"

Write-Host "== Start Business Runtime :8120 =="
$business = Start-Process python -ArgumentList @(
    "-m","uvicorn","agi_autopilot.app:app",
    "--app-dir","$repo\sidecars\business-runtime\src",
    "--host","127.0.0.1","--port","8120"
) -PassThru

Write-Host "== Start NEXUS bridge :8130 =="
$nexus = Start-Process python -ArgumentList @(
    "-m","uvicorn","nexus_bridge.app:app",
    "--app-dir","$repo\sidecars\nexus-bridge\src",
    "--host","127.0.0.1","--port","8130"
) -PassThru

Write-Host "== Start Connector Gateway :8150 =="
$connector = Start-Process python -ArgumentList @(
    "-m","uvicorn","connector_gateway.app:app",
    "--app-dir","$repo\sidecars\connector-gateway\src",
    "--host","127.0.0.1","--port","8150"
) -PassThru

$company = $null
if ($AutoCompanyDir) {
    Write-Host "== Install + start Auto-Company :8788 =="
    python -m pip install -e "$AutoCompanyDir\auto_company_v2"
    $company = Start-Process python -ArgumentList @(
        "$AutoCompanyDir\auto_company_v2\scripts\run_api.py"
    ) -WorkingDirectory "$AutoCompanyDir\auto_company_v2" -PassThru
} else {
    Write-Warning "Auto-Company not started. Pass -AutoCompanyDir for full E2E."
}

if ($WithRemote) {
    if (-not $ShttpsDir) { throw "-ShttpsDir is required with -WithRemote" }
    Write-Host "== Install SHTTPS Remote MCP =="
    python -m pip install -e $ShttpsDir
    Write-Host "Remote MCP gateway will be invoked by Autopilot through synergy-mcp-gateway."
}

Start-Sleep -Seconds 2

Write-Host ""
Write-Host "M5 stack started."
Write-Host "Business PID: $($business.Id)"
Write-Host "NEXUS PID: $($nexus.Id)"
Write-Host "Connector Gateway PID: $($connector.Id)"
if ($company) { Write-Host "Auto-Company PID: $($company.Id)" }
Write-Host ""
Write-Host "Example:"
Write-Host 'cargo run -p open-autopilot -- e2e-sales --lead-url "https://example.com" --lead-title "Example lead" --product "SYNERGY AI AUTOPILOT" --audience "business owners" --workspace "C:\work"'
Write-Host ""
Write-Host "Stop sidecars with Stop-Process -Id <PID>."
