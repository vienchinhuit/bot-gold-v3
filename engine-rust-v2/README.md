engine-rust-v2
================

Tick-breakout + momentum + volume-spike Rust engine (no lagging indicators).

Build

```powershell
cd engine-rust-v2
cargo build --release
```

Run (example)

```powershell
# dry-run (no trade sends) - default
target\release\engine-rust-v2.exe --symbol GOLD --breakout-window 60 --momentum-ticks 3 --vol-avg-ticks 20 --vol-spike-mult 2.0 --tick-verbose

# dry-run (recommended, more sensitive)
target\release\engine-rust-v2.exe --symbol GOLD --breakout-window 10 --momentum-ticks 3 --vol-avg-ticks 8 --vol-spike-mult 1.3 --momentum-min-delta 0.02 --tick-verbose

# enable live sends
target\release\engine-rust-v2.exe --trade --symbol GOLD --breakout-window 10 --momentum-ticks 3 --vol-avg-ticks 8 --vol-spike-mult 1.3 --momentum-min-delta 0.02 --log-level info
```

Notes

- Strategy triggers on tick when price breaks the recent high/low (window), momentum (recent tick deltas) aligns, and current tick volume is a spike vs recent average.
- No moving averages or lagging indicators are used.
- The engine now excludes the current tick when computing the recent average volume used for spike detection (this prevents the current tick from diluting the spike test).
- Tune `--breakout-window`, `--vol-avg-ticks`, and `--vol-spike-mult` to fit your market/data. Quick suggestion for live testing: `--breakout-window 10 --vol-avg-ticks 8 --vol-spike-mult 1.3 --momentum-min-delta 0.02`.
