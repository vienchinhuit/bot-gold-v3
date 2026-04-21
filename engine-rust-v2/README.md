# Advanced Scalping Engine v2.0

## Overview

Production-ready high-frequency scalping engine for XAUUSD/GOLD on M1 timeframe. Built with Rust for ultra-low latency and optimized performance.
**Key Philosophy**: Trade only high-probability setups, not every signal. The engine uses a multi-filter pipeline with a scoring system to avoid buying at tops and selling at bottoms.


---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        MARKET DATA (ZMQ PUB)                         │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        CANDLE BUILDER (M1)                           │
│  • Aggregate ticks into 1-minute candles                             │
│  • Track OHLCV in real-time                                          │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     FILTER PIPELINE (12 Filters)                     │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │  1.SIDEWAY  │  │  2.TREND    │  │  3.STRUCTURE│  │  4.PULLBACK│  │
│  │  EMA check  │  │  Strength   │  │  HH/HL/LH/LL│  │  Near EMA?  │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │  5.RSI      │  │  6.VOLATILITY│  │ 7.ANTI-FOMO│  │  8.CONFIRM │  │
│  │  Momentum   │  │  ATR-based  │  │  Distance   │  │  Candle    │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
│  │  9.COOLDOWN │  │ 10.NO-TRADE │  │ 11.SCORE   │  │ 12.RISK    │  │
│  │  Loss-based │  │  Zone check│  │  >= 5/10   │  │  SL/TP ATR │  │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘  │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      SCORING SYSTEM (0-10 pts)                         │
│                                                                     │
│   Trend Score      │  0-2 pts  │ EMA alignment with direction      │
│   Strength Score   │  0-2 pts  │ EMA distance magnitude           │
│   Structure Score  │  0-2 pts  │ HH/HL/LH/LL alignment           │
│   Pullback Score   │  0-1 pts  │ Price within pullback zone       │
│   RSI Score        │  0-1 pts  │ RSI momentum confirmation       │
│   Volatility       │  0-1 pts  │ No abnormal candle spikes        │
│   Confirmation     │  0-1 pts  │ Candle close validation         │
│  ─────────────────────────────────                                 │
│   TOTAL           │  0-10 pts │ MINIMUM 5 pts to enter          │
└─────────────────────────────────┬───────────────────────────────────┘
                                  │
                                  ▼
                    ┌─────────────────────────────────┐
                    │         SIGNAL / ORDER            │
                    │   BUY or SELL with SL/TP          │
                    └─────────────────────────────────┘
```

---


## Filter Details

### 1. Sideway Filter (Bắt buộc)
```
IF |EMA20 - EMA50| < threshold (0.30)
THEN NO TRADE (market is ranging)
```
**Tại sao**: Sideway market = whipsaws = thua liên tiếp

### 2. Trend Strength
```
IF |EMA20 - EMA50| < min_strength (0.20)
THEN NO TRADE (trend too weak)
```
**Tại sao**: Weak trend = reversible = dễ thua

### 3. Market Structure
```
BUY only when: Higher High (HH) OR Higher Low (HL) detected
SELL only when: Lower High (LH) OR Lower Low (LL) detected
```
**Tại sao**: Không BUY khi đang tạo Lower High, không SELL khi đang tạo Higher Low

### 4. Pullback Entry (Cực quan trọng)
```
IF price is NOT within pullback zone (15 pips from EMA)
THEN NO TRADE
```
**Tại sao**: Không vào lệnh khi giá đã chạy mạnh (impulse) → tránh Buy đỉnh/Sell đáy

### 5. RSI Filter
```
BUY:  RSI must rise from <40 to >50 (not just neutral zone)
SELL: RSI must drop from >60 to <50 (not just neutral zone)


NO BUY when RSI > 70 (overbought)
NO SELL when RSI < 30 (oversold)
```
**Tại sao**: Momentum confirmation, không chase overbought/oversold

### 6. Volatility Filter
```
IF candle_range > 1.5 * ATR
THEN NO TRADE (abnormal volatility)

IF upper_wick > 50% of body
THEN NO TRADE (possible liquidity sweep)
```
**Tại sao**: Không trade khi có news/spikes có thể quét stop loss

### 7. Anti-FOMO
```
IF price_distance_from_EMA > max_fomo_pips (25)
THEN NO TRADE
```
**Tại sao**: Không chase khi giá đã đi quá xa

### 8. Confirmation Candle
```
BUY: Wait for candle close ABOVE previous candle high
SELL: Wait for candle close BELOW previous candle low
```
**Tại sao**: Tránh false breakouts

### 9. Cooldown System
```
After LOSS: Wait 15 candles before next trade
After 3 consecutive losses: Pause 30 minutes
```
**Tại sao**: Không overtrade khi đang "on tilt"


### 10. No-Trade Zone
```
IF price is within 10 pips of recent trade price
THEN NO TRADE
```
**Tại sao**: Tránh vùng giá đã có nhiều orders

### 11. Scoring System
```
Total Score >= 5 AND Confidence >= 0.5
Otherwise NO TRADE
```
**Tại sao**: Multi-factor confirmation thay vì binary AND/OR

### 12. Risk Management
```
Stop Loss: 1.2 * ATR
Take Profit: 2.0 * ATR (TP >= 1.5 * SL ✓)
Max positions per direction: 5

Volume Management:
- Base volume per trade: 0.01 lots
- Max volume per direction: 0.10 lots
- Max total volume: 0.50 lots
```

---

## Build


```powershell
cd engine-rust-v2
cargo build --release
```


---


## Run


```powershell
# Signal only (test mode)
target\release\engine-rust-v2.exe --symbol GOLD

# Signal with verbose logging
target\release\engine-rust-v2.exe --symbol GOLD --verbose

# Real trading with default volume
target\release\engine-rust-v2.exe --trade --symbol GOLD

# Real trading with custom parameters
target\release\engine-rust-v2.exe --trade --symbol GOLD --min-score 6 --min-confidence 0.6

# Real trading with custom volume settings
target\release\engine-rust-v2.exe --trade --symbol GOLD --volume 0.02 --max-volume-per-trade 0.05 --max-total-volume 0.20

# Custom scoring threshold
target\release\engine-rust-v2.exe --trade --symbol GOLD --min-score 7 --max-pullback-pips 10
```

### Command Line Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `--symbol` | GOLD | Symbol to trade |
| `--trade` | false | Enable actual trading |
| `--verbose` | false | Show all skip reasons |
| `--sideway-threshold` | 0.30 | EMA distance for sideway detection |
| `--min-trend-strength` | 0.20 | Minimum EMA distance for valid trend |
| `--max-pullback-pips` | 15 | Max distance from EMA for pullback |
| `--max-fomo-pips` | 25 | Max distance from EMA (anti-FOMO) |
| `--cooldown-candles` | 15 | Cooldown after loss |
| `--max-losses` | 3 | Max consecutive losses before pause |
| `--pause-minutes` | 30 | Pause duration after max losses |
| `--max-candle-mult` | 1.5 | Max candle size vs ATR |
| `--min-score` | 5 | Minimum score to enter (0-10) |
| `--min-confidence` | 0.5 | Minimum confidence (0.0-1.0) |
| `--sl-mult` | 1.2 | Stop Loss = ATR * this |
| `--tp-mult` | 2.0 | Take Profit = ATR * this |
| `--cooldown-sec` | 5 | Minimum seconds between trades |
| `--volume` | 0.01 | Base volume per trade (lots) |
| `--max-volume-per-trade` | 0.10 | Max volume per trade direction (lots) |
| `--max-total-volume` | 0.50 | Max total volume across all positions (lots) |


---

## Expected Output

```
============================================================
  ADVANCED SCALPING ENGINE v2.0
============================================================
Symbol: XAUUSD (input: GOLD) | Trading: ENABLED
Filters: Sideway=0.3 | Trend=0.2 | Pullback=15pips | FOMO=25pips
Risk: SL=1.2*ATR | TP=2*ATR | MinScore=5 | MinConf=0.50
Volume: 0.01-0.1 lots/trade | MaxTotal=0.5 lots
Cooldown: 15 candles | MaxLosses=3 | Pause=30min
============================================================

🔄 WARMUP: 1/60 ticks collected (need 60 for indicators)
🔄 WARMUP: 30/60 ticks collected
✅ WARMUP COMPLETE: 60 ticks collected, indicators ready

📊 STATUS | Price=2850.50 | 📈 Uptrend | EMA20=2850.30 EMA50=2849.80 | RSI=55.3 | ATR=0.52 | Ticks=60
   Trend: STRONG | Cooldown: 0 | Losses: 0 | Positions: Long=0 Short=0
   ┌────────────────────────── SCORE BREAKDOWN ──────────────────────────┐
   │ Trend    [██] 2 pts  (need >=2 for valid)
   │ Strength [██] 2 pts  (EMA dist: 0.500)
   │ Struct   [██] 2 pts  (HigherHigh)
   │ Pullback [██] 1 pts  (OK)
   │ RSI      [██] 1 pts  (55.3)
   │ Vol/Conf [██] 2 pts  (assumed OK)
   └────────────────────────────────────────────────────────────────────────────┘
   📈 POTENTIAL SCORE: [██████████] 10/10 | Min Required: 5
   ✅ Would qualify - Watching for entry confirmation...

📊 STATUS | Price=2851.20 | 📈 Uptrend | EMA20=2850.35 EMA50=2849.85 | RSI=58.2 | ATR=0.51 | Ticks=120
   Trend: STRONG | Cooldown: 0 | Losses: 0 | Positions: Long=0 Short=0
   ┌────────────────────────── SCORE BREAKDOWN ──────────────────────────┐
   │ Trend    [██] 2 pts  (need >=2 for valid)
   │ Strength [██] 2 pts  (EMA dist: 0.500)
   │ Struct   [██] 2 pts  (HigherHigh)
   │ Pullback [██] 1 pts  (OK)
   │ RSI      [██] 1 pts  (58.2)
   │ Vol/Conf [██] 2 pts  (assumed OK)
   └────────────────────────────────────────────────────────────────────────────┘
   📈 POTENTIAL SCORE: [██████████] 10/10 | Min Required: 5
   ✅ Would qualify - Watching for entry confirmation...


🚨🚨🚨 SIGNAL BUY  | score=7/10 conf=0.70 | EMA20=2850.35 EMA50=2849.85 | RSI=52.3 | ATR=0.51 | SL=2849.74 TP=2850.57
   Reason: BUY: Uptrend + pullback + RSI rising
   📊 SCORE BREAKDOWN (7 pts):
   ├ Trend      [██] 2 pts
   ├ Strength   [██] 2 pts
   ├ Structure  [██] 2 pts
   ├ Pullback   [██] 1 pts
   ├ RSI        [██] 1 pts
   ├ Volatility [██] 1 pts
   └ Confirm   [██] 1 pts
   📈 SCORE BAR [███████---] 7/10 (min: 5)
   ✅ Score 7 >= min 5 - CONFIRMED

📤 EXECUTING BUY 0.01 lots @ 2850.60 | SL=2849.74 TP=2850.57
```

---


## Logging Levels

```powershell
# Quiet mode (only signals)
target\release\engine-rust-v2.exe --trade --symbol GOLD --log-level warn

# Normal mode (status updates)
target\release\engine-rust-v2.exe --trade --symbol GOLD --log-level info

# Verbose mode (all filters and reasons)
target\release\engine-rust-v2.exe --trade --symbol GOLD --verbose
```


---

## Scoring Breakdown

Each component contributes to the final score:

```
┌────────────────────────────────────────────────────────────────────┐
│  COMPONENT        │  POINTS  │  CONDITION                           │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Trend           │  0-2     │  2 = EMA aligns with direction        │
│                  │          │  0 = Flat or opposite trend          │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Strength        │  0-2     │  2 = EMA distance > 2x min           │
│                  │          │  1 = EMA distance > 1x min           │
│                  │          │  0 = Weak trend                      │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Structure       │  0-2     │  2 = Perfect structure (HH/HL/LH/LL) │
│                  │          │  1 = Partial structure match          │
│                  │          │ -1 = Anti-pattern detected           │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Pullback        │  0-1     │  1 = Price within pullback zone      │
│                  │          │  0 = Not in pullback                 │
├──────────────────┼──────────┼───────────────────────────────────────│
│  RSI             │  0-1     │  1 = RSI momentum confirmed          │
│                  │          │  0 = No momentum or wrong zone       │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Volatility      │  0-1     │  1 = No abnormal volatility           │
│                  │          │  0 = Spike detected                 │
├──────────────────┼──────────┼───────────────────────────────────────│
│  Confirmation   │  0-1     │  1 = Candle close confirmed          │
│                  │          │  0 = No confirmation                │
├──────────────────┼──────────┼───────────────────────────────────────│
│  TOTAL           │  0-10    │  Minimum 5 to enter                  │
└──────────────────┴──────────┴───────────────────────────────────────┘
```


---


## Why This Strategy Works

### Before (v1 - Problematic)
```rust
if ema_fast > ema_slow && rsi < 70 {
    BUY  // ❌ Buys at resistance, catches falling knife
}
if ema_fast < ema_slow && rsi > 30 {
    SELL  // ❌ Sells at support, misses the bounce
}
```

**Problems:**
- ❌ Trades in sideway market
- ❌ Buys when RSI already overbought
- ❌ No structure awareness (buys into LH!)
- ❌ Chases impulses (no pullback check)
- ❌ No cooldown after losses
- ❌ Binary logic = binary failures

### After (v2 - Professional)
```rust
// Must pass ALL filters first
if is_sideway() { return NO_TRADE }
if !structure_aligned() { return NO_TRADE }
if !in_pullback_zone() { return NO_TRADE }
if volatility_spike() { return NO_TRADE }

// Then accumulate score
if trend_correct() { score += 2 }
if structure_perfect() { score += 2 }
if pullback_fresh() { score += 1 }
if rsi_confirmed() { score += 1 }
if volatility_ok() { score += 1 }
if candle_confirmed() { score += 1 }

if score >= 5 && confidence >= 0.5 {
    ENTER_TRADE  // ✅ High probability only
}
```


**Improvements:**
- ✅ Filters reject bad setups BEFORE scoring
- ✅ Structure prevents buying into reversals
- ✅ Pullback = better entry price
- ✅ RSI momentum = confirmation, not just zone
- ✅ Cooldown = protection from tilt
- ✅ Scoring = graduated probability, not binary

---


## ZMQ Connection

The engine connects to two ZMQ endpoints:

```
Market Feed (SUB):  tcp://127.0.0.1:5555  (receives TICK messages)
Order Router (DEALER): tcp://127.0.0.1:5556 (sends ORDER_SEND messages)
```

### Expected TICK Message Format
```json
{
  "type": "TICK",
  "data": {
    "symbol": "XAUUSD",
    "last": 2850.50,
    "bid": 2850.40,
    "ask": 2850.60,
    "volume": 100,
    "server_time": "2025-01-01T12:00:00.000Z"
  }
}
```

### Order Response Format
```json
{
  "type": "ORDER_RESPONSE",
  "data": {
    "retcode": 10009,
    "deal": 123456,
    "order": 789012,
    "volume": 0.01
  }
}
```


---

## Performance Optimizations

- **Zero-copy data structures**: Candle and Tick use copy semantics
- **Pre-allocated collections**: VecDeque with capacity hints
- **Inline calculations**: `#[inline]` attributes on hot paths
- **Release optimizations**: LTO enabled, codegen-units=1
- **Minimal heap allocations**: Stack-based calculations where possible


---

## Disclaimer

⚠️ **FOR EDUCATIONAL PURPOSES ONLY**


This engine is a demonstration of professional trading system design. Always:
- Backtest thoroughly before live trading
- Use proper risk management
- Understand that past performance does not guarantee future results
- Never risk more than you can afford to lose


---


## Author

Senior Quantitative Developer
XAUUSD/GOLD M1 Scalping Strategy
