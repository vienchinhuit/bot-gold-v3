#!/usr/bin/env bash
set -euo pipefail

# Dry-run: collect history, warmup, optimizer, but DO NOT execute real trades (--trade false)
# Edit variables below as needed before running.

# Replace with your actual Slack webhook if desired
SLACK_WEBHOOK="https://hooks.slack.com/services/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"
SLACK_CHANNEL="ai-trading"

export RUST_LOG="debug"

cargo run --release -- \
  --trade false \
  --symbol GOLD \
  --min-score 0 \
  --volume 1 \
  --max-volume-per-trade 50 \
  --max-total-volume 100 \
  --sl-mult 15 \
  --tp-mult 30 \
  --cooldown-sec 0 \
  --slack-enabled \
  --slack-webhook "$SLACK_WEBHOOK" \
  --slack-channel "$SLACK_CHANNEL" \
  --slack-notify-port 5557 \
  --auto-optimize \
  --auto-reload-optimized-config \
  --optimizer-output-file "optimizer_result.json" \
  --history-count 500 \
  --use-mt5-bridge true \
  --mt5-bridge-script "python_bridge/main.py" \
  --mt5-symbol "GOLD" \
  --log-level debug
