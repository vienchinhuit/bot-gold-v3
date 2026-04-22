#!/usr/bin/env bash
set -euo pipefail

# Live trading (safer defaults). Review and adjust volumes, SL/TP and limits before running.
# Replace with your actual Slack webhook if desired
SLACK_WEBHOOK="https://hooks.slack.com/services/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"
SLACK_CHANNEL="ai-trading"

export RUST_LOG="info"

cargo run --release -- \
  --trade true \
  --symbol GOLD \
  --min-score 1 \
  --volume 0.1 \
  --max-volume-per-trade 10 \
  --max-total-volume 50 \
  --sl-mult 6 \
  --tp-mult 12 \
  --cooldown-sec 5 \
  --slack-enabled \
  --slack-webhook "$SLACK_WEBHOOK" \
  --slack-channel "$SLACK_CHANNEL" \
  --slack-notify-port 5557 \
  --auto-optimize \
  --auto-reload-optimized-config \
  --optimizer-output-file "optimizer_result.json" \
  --optimizer-reload-sec 120 \
  --history-count 500 \
  --use-mt5-bridge true \
  --mt5-bridge-script "python_bridge/main.py" \
  --mt5-symbol "GOLD" \
  --log-level info
