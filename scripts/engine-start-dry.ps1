# PowerShell script to start engine in DRY-RUN (no trading)
# Location: scripts/engine-start-dry.ps1
# Assumes repository layout:
#  - engine-rust-v2/   (engine project)
#  - python_bridge/    (existing python bridge)
#  - scripts/          (this script)

param(
    [string]$SlackWebhook = "https://hooks.slack.com/services/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",
    [string]$SlackChannel = "ai-trading"
)

# Resolve paths
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$EngineDir = Join-Path $ScriptDir "..\engine-rust-v2" | Resolve-Path -ErrorAction Stop
$Mt5BridgeScript = "..\python_bridge\main.py"  # relative to engine folder after Set-Location

Write-Host "Starting engine (DRY RUN) from: $EngineDir"
Push-Location $EngineDir

try {
    $env:RUST_LOG = "debug"

    & cargo run --release -- `
        --trade false `
        --symbol GOLD `
        --min-score 0 `
        --volume 1 `
        --max-volume-per-trade 50 `
        --max-total-volume 100 `
        --sl-mult 15 `
        --tp-mult 30 `
        --cooldown-sec 0 `
        --slack-enabled `
        --slack-webhook $SlackWebhook `
        --slack-channel $SlackChannel `
        --slack-notify-port 5557 `
        --auto-optimize `
        --auto-reload-optimized-config `
        --optimizer-output-file "optimizer_result.json" `
        --history-count 500 `
        --use-mt5-bridge true `
        --mt5-bridge-script $Mt5BridgeScript `
        --mt5-symbol GOLD `
        --log-level debug
}
finally {
    Pop-Location
}
