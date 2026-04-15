"""Grid search without EMA trend filter (to find parameters that increase win rate).
"""
import csv
import time
import itertools

import backtest as bt

# Config
SYMBOL = 'GOLD'
MONTHS = 3
START_CAPITAL = 10000.0
SIZE_PCT = 0.01
TIMEFRAME = 'M1'

# Parameter grid
RSI_BUY = [25.0, 30.0, 35.0]
RSI_SELL = [65.0, 70.0, 75.0]
HIST_THRESH = [0.0, 0.02]
SL_PCT = [0.01, 0.02]
TP_PCT = [0.02, 0.04]
TRAILING = [False]

grid = list(itertools.product(RSI_BUY, RSI_SELL, HIST_THRESH, SL_PCT, TP_PCT, TRAILING))
print(f"Grid size: {len(grid)}")

print('Fetching bars from MT5...')
bars = bt.fetch_mt5_bars(SYMBOL, MONTHS, timeframe=TIMEFRAME)
print('Bars fetched:', len(bars))

out_rows = []
start = time.time()
for idx, (rsi_b, rsi_s, hist_t, sl_p, tp_p, trail) in enumerate(grid, 1):
    try:
        res = bt.run_backtest(
            bars,
            start_capital=START_CAPITAL,
            size_pct=SIZE_PCT,
            rsi_buy=rsi_b,
            rsi_sell=rsi_s,
            hist_thresh=hist_t,
            ema_period=None,
            use_trend=False,
            sl_pct=sl_p,
            tp_pct=tp_p,
            trailing=trail,
            atr_mult=0.0,
            start_hour=0,
            end_hour=23,
        )
    except Exception as e:
        print('Error on combo', idx, e)
        continue

    out_rows.append({
        'rsi_buy': rsi_b,
        'rsi_sell': rsi_s,
        'hist_thresh': hist_t,
        'sl_pct': sl_p,
        'tp_pct': tp_p,
        'trailing': trail,
        'end_capital': res['end_capital'],
        'gross_pnl': res['gross_pnl'],
        'trades': res['total_trades'],
        'win_rate': res['win_rate'],
        'max_dd': res['max_drawdown'],
    })

    if idx % 10 == 0:
        print(f"{idx}/{len(grid)} done")

elapsed = time.time() - start
print('Grid search finished in', elapsed, 'seconds. Results:', len(out_rows))

# Sort by win_rate then end_capital
out_rows.sort(key=lambda r: (r['win_rate'], r['end_capital']), reverse=True)

# write CSV
with open('opt_no_trend_results.csv', 'w', newline='') as f:
    writer = csv.DictWriter(f, fieldnames=list(out_rows[0].keys()) if out_rows else [])
    if out_rows:
        writer.writeheader()
        writer.writerows(out_rows)

print('\nTop 10 configs:')
for r in out_rows[:10]:
    print(r)

print('Saved opt_no_trend_results.csv')
