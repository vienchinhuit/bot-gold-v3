# PowerShell script to start engine in LIVE mode (trading enabled) with safer defaults
# Location: scripts/engine-start-live.ps1

param(
    [string]$SlackWebhook = "https://hooks.slack.com/services/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",
    [string]$SlackChannel = "ai-trading"
)

# Resolve paths
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$EngineDir = Join-Path $ScriptDir "..\engine-rust-v2" | Resolve-Path -ErrorAction Stop
$Mt5BridgeScript = "..\python_bridge\main.py"  # relative to engine folder after Set-Location

Write-Host "Starting engine (LIVE) from: $EngineDir"
Push-Location $EngineDir

try {
    $env:RUST_LOG = "info"

    & cargo run --release -- `
        --trade true `
        --symbol GOLD `
        --min-score 1 `
        --volume 0.1 `
        --max-volume-per-trade 10 `
        --max-total-volume 50 `
        --sl-mult 6 `
        --tp-mult 12 `
        --cooldown-sec 5 `
        --slack-enabled `
        --slack-webhook $SlackWebhook `
        --slack-channel $SlackChannel `
        --slack-notify-port 5557 `
        --auto-optimize `
        --auto-reload-optimized-config `
        --optimizer-output-file "optimizer_result.json" `
        --optimizer-reload-sec 120 `
        --history-count 500 `
        --use-mt5-bridge true `
        --mt5-bridge-script $Mt5BridgeScript `
        --mt5-symbol GOLD `
        --log-level info
}
finally {
    Pop-Location
}
