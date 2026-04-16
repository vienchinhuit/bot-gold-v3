engine-rust-v3
=================

Purpose: tick-breakout OR volume-spike engine (no lagging indicators).

Quick run (after building):

```powershell
cd engine-rust-v3
cargo build --release
.\target\release\engine-rust-v3.exe --symbol GOLD --tick-verbose --log-level info
```

Defaults tuned for sensitivity: small breakout window, short volume average, lower vol multiplier.
