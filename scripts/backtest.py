"""Simple backtester for RSI+MACD strategy on 1-minute bars.

Usage examples:
  python scripts/backtest.py --months 3 --capital 10000 --symbol GOLD
  python scripts/backtest.py --csv data/GOLD_M1.csv --months 6 --capital 5000

If `--csv` is omitted the script will try to fetch data from MetaTrader5 (if installed).

Notes:
- Strategy: same as engine-rust: RSI(14) + MACD(12,26,9).
- Entry: BUY when RSI<30 and MACD histogram turns positive (hist_curr>0 and hist_prev<hist_curr).
         SELL when RSI>70 and MACD histogram turns negative.
- Exit: close on opposite signal or at end of dataset.
- Position sizing: fixed fraction `--size-pct` of current capital per trade (default 1%).
"""

import argparse
import csv
from datetime import datetime, timedelta
import math
import sys
import os
import numpy as np

try:
    import MetaTrader5 as mt5
    HAS_MT5 = True
except Exception:
    HAS_MT5 = False


def read_csv_bars(path):
    bars = []
    with open(path, 'r', newline='') as f:
        reader = csv.DictReader(f)
        for row in reader:
            # Support time as ISO or epoch seconds
            t = row.get('time') or row.get('datetime') or row.get('date')
            if t is None:
                continue
            try:
                if t.isdigit():
                    ts = int(t)
                    dt = datetime.fromtimestamp(ts)
                else:
                    dt = datetime.fromisoformat(t)
            except Exception:
                try:
                    dt = datetime.strptime(t, '%Y.%m.%d %H:%M:%S')
                except Exception:
                    dt = None

            if dt is None:
                continue

            o = float(row.get('open') or row.get('o') or 0)
            h = float(row.get('high') or row.get('h') or 0)
            l = float(row.get('low') or row.get('l') or 0)
            c = float(row.get('close') or row.get('c') or 0)
            v = int(float(row['volume']) if 'volume' in row else 0)
            bars.append({'time': dt, 'open': o, 'high': h, 'low': l, 'close': c, 'volume': v})
    return sorted(bars, key=lambda x: x['time'])


def fetch_mt5_bars(symbol, months, timeframe='M1'):
    if not HAS_MT5:
        raise RuntimeError('MetaTrader5 module not available')
    if not mt5.initialize():
        raise RuntimeError('MT5 initialize failed')

    to_dt = datetime.now()
    from_dt = to_dt - timedelta(days=months * 30)

    # Map timeframe string to MT5 constant
    tf = timeframe.upper()
    tf_map = {
        'M1': getattr(mt5, 'TIMEFRAME_M1', mt5.TIMEFRAME_M1),
        'D1': getattr(mt5, 'TIMEFRAME_D1', mt5.TIMEFRAME_D1),
        'W1': getattr(mt5, 'TIMEFRAME_W1', mt5.TIMEFRAME_W1)
    }
    mt5_tf = tf_map.get(tf, mt5.TIMEFRAME_M1)

    # Ensure symbol is selected in MarketWatch (can affect copy_rates_range)
    try:
        sel = mt5.symbol_select(symbol, True)
        print('MT5 symbol_select', symbol, sel)
    except Exception:
        pass

    rates = mt5.copy_rates_range(symbol, mt5_tf, from_dt, to_dt)
    if rates is None or len(rates) == 0:
        print('No rates for', symbol, 'in full range; attempting chunked fetch...')
        # Try fetching in smaller chunks (some brokers/terminals limit large range requests)
        try:
            chunk_days = 7
            cur_from = from_dt
            all_rates = []
            while cur_from < to_dt:
                cur_to = min(cur_from + timedelta(days=chunk_days), to_dt)
                r = mt5.copy_rates_range(symbol, mt5_tf, cur_from, cur_to)
                if r is None:
                    print('Chunk fetch returned None for', cur_from, cur_to)
                    break
                all_rates.extend(list(r))
                cur_from = cur_to

            if len(all_rates) > 0:
                rates = np.array(all_rates)
        except Exception as e:
            print('Chunked fetch exception:', e)

    if rates is None or len(rates) == 0:
        # Try a common alternative name for gold (broker-dependent)
        alt = 'XAUUSD'
        try:
            print('No rates for', symbol, '— trying', alt)
            mt5.symbol_select(alt, True)
            rates = mt5.copy_rates_range(alt, mt5_tf, from_dt, to_dt)
            if rates is None:
                # try chunked for alt
                chunk_days = 7
                cur_from = from_dt
                all_rates = []
                while cur_from < to_dt:
                    cur_to = min(cur_from + timedelta(days=chunk_days), to_dt)
                    r = mt5.copy_rates_range(alt, mt5_tf, cur_from, cur_to)
                    if r is None:
                        break
                    all_rates.extend(list(r))
                    cur_from = cur_to
                if len(all_rates) > 0:
                    rates = np.array(all_rates)
        except Exception:
            rates = None

    if rates is None or len(rates) == 0:
        raise RuntimeError('No rates returned from MT5')

    bars = []
    for r in rates:
        t = datetime.fromtimestamp(int(r['time']))
        # numpy.void (structured) doesn't support .get(); access fields directly and handle missing names
        vol = 0
        try:
            names = r.dtype.names
        except Exception:
            names = None

        if names:
            if 'tick_volume' in names:
                vol = int(r['tick_volume'])
            elif 'real_volume' in names:
                vol = int(r['real_volume'])
            elif 'volume' in names:
                vol = int(r['volume'])
            else:
                vol = 0
        else:
            # fallback
            try:
                vol = int(r['tick_volume'])
            except Exception:
                vol = 0

        bars.append({'time': t, 'open': float(r['open']), 'high': float(r['high']), 'low': float(r['low']), 'close': float(r['close']), 'volume': vol})

    return bars


def aggregate_bars(bars, timeframe='D1'):
    """Aggregate smaller timeframe bars into daily or weekly bars.

    bars: list of {'time': datetime, 'open','high','low','close','volume'}
    timeframe: 'D1' or 'W1'
    """
    tf = timeframe.upper()
    if tf == 'M1' or tf == 'm1':
        return bars

    groups = {}
    if tf == 'D1':
        for b in bars:
            key = b['time'].date()
            if key not in groups:
                groups[key] = {'time': datetime.combine(key, datetime.min.time()), 'open': b['open'], 'high': b['high'], 'low': b['low'], 'close': b['close'], 'volume': b['volume']}
            else:
                groups[key]['high'] = max(groups[key]['high'], b['high'])
                groups[key]['low'] = min(groups[key]['low'], b['low'])
                groups[key]['close'] = b['close']
                groups[key]['volume'] += b['volume']

    elif tf == 'W1':
        for b in bars:
            iso = b['time'].isocalendar()
            year, week = iso[0], iso[1]
            key = (year, week)
            week_start = datetime.fromisocalendar(year, week, 1)
            if key not in groups:
                groups[key] = {'time': datetime.combine(week_start.date(), datetime.min.time()), 'open': b['open'], 'high': b['high'], 'low': b['low'], 'close': b['close'], 'volume': b['volume']}
            else:
                groups[key]['high'] = max(groups[key]['high'], b['high'])
                groups[key]['low'] = min(groups[key]['low'], b['low'])
                groups[key]['close'] = b['close']
                groups[key]['volume'] += b['volume']
    else:
        raise ValueError('Unsupported timeframe for aggregation: ' + str(timeframe))

    # Convert groups to sorted list
    items = sorted(groups.values(), key=lambda x: x['time'])
    return items


def ema_series(prices, period):
    n = len(prices)
    ema = [None] * n
    if n < period:
        return ema
    sma = sum(prices[:period]) / period
    ema[period - 1] = sma
    k = 2.0 / (period + 1)
    for i in range(period, n):
        ema[i] = prices[i] * k + ema[i - 1] * (1 - k)
    return ema


def rsi_series(prices, period=14):
    n = len(prices)
    rsi = [None] * n
    if n < period + 1:
        return rsi
    gains = [0.0] * n
    losses = [0.0] * n
    for i in range(1, n):
        diff = prices[i] - prices[i - 1]
        gains[i] = max(diff, 0.0)
        losses[i] = max(-diff, 0.0)

    avg_gain = sum(gains[1: period + 1]) / period
    avg_loss = sum(losses[1: period + 1]) / period

    if avg_loss == 0:
        rsi[period] = 100.0
    else:
        rs = avg_gain / avg_loss
        rsi[period] = 100.0 - (100.0 / (1.0 + rs))

    for i in range(period + 1, n):
        avg_gain = (avg_gain * (period - 1) + gains[i]) / period
        avg_loss = (avg_loss * (period - 1) + losses[i]) / period
        if avg_loss == 0:
            rsi[i] = 100.0
        else:
            rs = avg_gain / avg_loss
            rsi[i] = 100.0 - (100.0 / (1.0 + rs))
    return rsi


def macd_series(prices, fast=12, slow=26, signal=9):
    n = len(prices)
    ema_fast = ema_series(prices, fast)
    ema_slow = ema_series(prices, slow)
    macd = [None] * n
    for i in range(n):
        if ema_fast[i] is not None and ema_slow[i] is not None:
            macd[i] = ema_fast[i] - ema_slow[i]

    # build signal on macd values
    macd_vals = [v for v in macd if v is not None]
    signal_vals = [None] * len(macd_vals)
    if len(macd_vals) >= signal:
        sma = sum(macd_vals[:signal]) / signal
        signal_vals[signal - 1] = sma
        k = 2.0 / (signal + 1)
        for j in range(signal, len(macd_vals)):
            signal_vals[j] = macd_vals[j] * k + signal_vals[j - 1] * (1 - k)

    # map signal_vals back to original index
    signal = [None] * n
    idx = 0
    for i in range(n):
        if macd[i] is not None:
            if idx < len(signal_vals):
                signal[i] = signal_vals[idx]
            idx += 1

    hist = [None] * n
    for i in range(n):
        if macd[i] is not None and signal[i] is not None:
            hist[i] = macd[i] - signal[i]

    return macd, signal, hist


def atr_series(bars, period=14):
    """Compute ATR series from bars (list of dicts with high, low, close)."""
    n = len(bars)
    tr = [None] * n
    for i in range(n):
        high = bars[i]['high']
        low = bars[i]['low']
        if i == 0:
            prev_close = bars[i]['close']
        else:
            prev_close = bars[i - 1]['close']
        tr_i = max(high - low, abs(high - prev_close), abs(low - prev_close))
        tr[i] = tr_i

    atr = [None] * n
    if n < period:
        return atr

    # Wilder's smoothing
    first_atr = sum(tr[1: period + 1]) / period
    atr[period] = first_atr
    for i in range(period + 1, n):
        atr[i] = (atr[i - 1] * (period - 1) + tr[i]) / period
    return atr


def run_backtest(bars, start_capital=10000.0, size_pct=0.01,
                 rsi_buy=30.0, rsi_sell=70.0, hist_thresh=0.0,
                 ema_period=None, use_trend=False,
                 sl_pct=None, tp_pct=None, trailing=False, atr_mult=0.0,
                 start_hour=0, end_hour=23,
                 rsi_period=14, macd_fast=12, macd_slow=26, macd_signal=9,
                 spread=0.0, slippage=0.0, max_hold_bars=None):
    closes = [b['close'] for b in bars]
    times = [b['time'] for b in bars]
    n = len(closes)
    rsi = rsi_series(closes, rsi_period)
    macd, sig, hist = macd_series(closes, fast=macd_fast, slow=macd_slow, signal=macd_signal)

    # advanced series
    ema = None
    if use_trend and ema_period and ema_period > 0:
        ema = ema_series(closes, ema_period)
    atr = None
    if atr_mult and atr_mult > 0:
        atr = atr_series(bars, period=14)

    capital = start_capital
    trades = []
    position = None  # dict: side, entry_price, units, entry_idx, entry_time

    for i in range(1, n):
        # only evaluate when macd hist and rsi available
        if rsi[i] is None or hist[i] is None or hist[i - 1] is None:
            continue

        # time of day filter
        h = times[i].hour
        if not (start_hour <= h <= end_hour):
            continue

        price = closes[i]
        # signals
        rsi_val = rsi[i]
        hist_curr = hist[i]
        hist_prev = hist[i - 1]

        buy_signal = (rsi_val < rsi_buy) and (hist_curr > hist_thresh) and (hist_prev < hist_curr)
        sell_signal = (rsi_val > rsi_sell) and (hist_curr < -hist_thresh) and (hist_prev > hist_curr)

        # trend filter
        if use_trend and ema and ema[i] is not None:
            if buy_signal and closes[i] <= ema[i]:
                buy_signal = False
            if sell_signal and closes[i] >= ema[i]:
                sell_signal = False

        if position is None:
            if buy_signal or sell_signal:
                # open position with SL/TP
                risk_amount = capital * size_pct
                side = 'LONG' if buy_signal else 'SHORT'
                # determine sl/tp based on atr_mult or percent
                if atr and atr[i] is not None and atr_mult and atr_mult > 0:
                    sl_dist = atr[i] * atr_mult
                    tp_dist = atr[i] * atr_mult * 2.0 if tp_pct is None else price * tp_pct
                else:
                    sl_dist = price * sl_pct if sl_pct is not None else None
                    tp_dist = price * tp_pct if tp_pct is not None else None

                # compute units sized by risk_amount and sl_dist when available
                if sl_dist is not None and sl_dist > 0:
                    units = risk_amount / sl_dist
                else:
                    # fallback: allocate fraction of capital / price
                    units = (capital * size_pct) / price

                entry_price = price
                # compute stop and tp prices
                if side == 'LONG':
                    stop_price = entry_price - sl_dist if sl_dist is not None else None
                    tp_price = entry_price + tp_dist if tp_dist is not None else None
                    peak = entry_price
                else:
                    stop_price = entry_price + sl_dist if sl_dist is not None else None
                    tp_price = entry_price - tp_dist if tp_dist is not None else None
                    peak = entry_price

                # track max hold
                max_hold = (i + max_hold_bars) if (max_hold_bars is not None) else None

                position = {'side': side, 'entry_price': entry_price, 'units': units, 'entry_idx': i, 'entry_time': times[i], 'stop_price': stop_price, 'tp_price': tp_price, 'peak': peak, 'max_hold': max_hold}
                trades.append({'entry_time': times[i], 'entry_price': entry_price, 'side': side, 'exit_time': None, 'exit_price': None, 'pnl': None})
        else:
            # Manage open position: check SL/TP and opposite signal
            # Update trailing stop using peak
            if position is not None:
                side = position['side']
                # update peak
                if side == 'LONG':
                    position['peak'] = max(position.get('peak', position['entry_price']), bars[i]['high'])
                    if trailing and sl_pct is not None:
                        # trail stop to peak*(1-sl_pct)
                        new_stop = position['peak'] * (1.0 - sl_pct)
                        if position['stop_price'] is None or new_stop > position['stop_price']:
                            position['stop_price'] = new_stop
                else:
                    position['peak'] = min(position.get('peak', position['entry_price']), bars[i]['low'])
                    if trailing and sl_pct is not None:
                        new_stop = position['peak'] * (1.0 + sl_pct)
                        if position['stop_price'] is None or new_stop < position['stop_price']:
                            position['stop_price'] = new_stop

                exited = False
                # check TP/SL on this bar
                if side == 'LONG':
                    low = bars[i]['low']
                    high = bars[i]['high']
                    sp = position.get('stop_price')
                    tp = position.get('tp_price')
                    if tp is not None and high >= tp:
                        exit_price = tp
                        exited = True
                    elif sp is not None and low <= sp:
                        exit_price = sp
                        exited = True
                else:
                    low = bars[i]['low']
                    high = bars[i]['high']
                    sp = position.get('stop_price')
                    tp = position.get('tp_price')
                    if tp is not None and low <= tp:
                        exit_price = tp
                        exited = True
                    elif sp is not None and high >= sp:
                        exit_price = sp
                        exited = True

                if exited:
                    if side == 'LONG':
                        pnl = (exit_price - position['entry_price']) * position['units']
                    else:
                        pnl = (position['entry_price'] - exit_price) * position['units']
                    # subtract round-trip costs (spread + slippage both sides)
                    cost_per_round = spread + 2.0 * slippage
                    pnl -= cost_per_round * position['units']
                    capital += pnl
                    trades[-1].update({'exit_time': times[i], 'exit_price': exit_price, 'pnl': pnl})
                    position = None
                    continue

            # if opposite signal, close at market price
            if position is not None:
                # max hold close
                if position.get('max_hold') is not None and i >= position.get('max_hold'):
                    exit_price = price
                    if position['side'] == 'LONG':
                        pnl = (exit_price - position['entry_price']) * position['units']
                    else:
                        pnl = (position['entry_price'] - exit_price) * position['units']
                    pnl -= (spread + 2.0 * slippage) * position['units']
                    capital += pnl
                    trades[-1].update({'exit_time': times[i], 'exit_price': exit_price, 'pnl': pnl})
                    position = None
                elif position['side'] == 'LONG' and sell_signal:
                    exit_price = price
                    pnl = (exit_price - position['entry_price']) * position['units']
                    pnl -= (spread + 2.0 * slippage) * position['units']
                    capital += pnl
                    trades[-1].update({'exit_time': times[i], 'exit_price': exit_price, 'pnl': pnl})
                    position = None
                elif position['side'] == 'SHORT' and buy_signal:
                    exit_price = price
                    pnl = (position['entry_price'] - exit_price) * position['units']
                    pnl -= (spread + 2.0 * slippage) * position['units']
                    capital += pnl
                    trades[-1].update({'exit_time': times[i], 'exit_price': exit_price, 'pnl': pnl})
                    position = None

    # close any open position at last price
    if position is not None:
        exit_price = closes[-1]
        if position['side'] == 'LONG':
            pnl = (exit_price - position['entry_price']) * position['units']
        else:
            pnl = (position['entry_price'] - exit_price) * position['units']
        # subtract trading costs
        pnl -= (spread + 2.0 * slippage) * position['units']
        capital += pnl
        trades[-1].update({'exit_time': times[-1], 'exit_price': exit_price, 'pnl': pnl})

    # metrics
    gross_pnl = sum(t['pnl'] for t in trades if t['pnl'] is not None)
    wins = [t for t in trades if t['pnl'] is not None and t['pnl'] > 0]
    losses = [t for t in trades if t['pnl'] is not None and t['pnl'] <= 0]
    total_trades = len([t for t in trades if t['pnl'] is not None])
    win_rate = (len(wins) / total_trades * 100.0) if total_trades > 0 else 0.0

    # equity curve
    equity = [start_capital]
    cur = start_capital
    for t in trades:
        if t['pnl'] is not None:
            cur += t['pnl']
            equity.append(cur)

    peak = -1e18
    max_dd = 0.0
    for e in equity:
        if e > peak:
            peak = e
        dd = (peak - e)
        if dd > max_dd:
            max_dd = dd

    return {
        'start_capital': start_capital,
        'end_capital': capital,
        'gross_pnl': gross_pnl,
        'trades': trades,
        'total_trades': total_trades,
        'wins': len(wins),
        'losses': len(losses),
        'win_rate': win_rate,
        'max_drawdown': max_dd,
    }


def parse_args():
    p = argparse.ArgumentParser()
    p.add_argument('--months', type=int, default=3, help='Months of history to use')
    p.add_argument('--capital', type=float, default=10000.0, help='Starting capital')
    p.add_argument('--symbol', type=str, default='GOLD', help='Symbol')
    p.add_argument('--csv', type=str, default=None, help='Path to CSV of minute bars (time,open,high,low,close,volume)')
    p.add_argument('--timeframe', type=str, default='M1', choices=['M1','D1','W1'], help='Timeframe to backtest: M1 (minute), D1 (daily), W1 (weekly)')
    p.add_argument('--size-pct', type=float, default=0.01, help='Fraction of capital to allocate per trade (0-1)')
    # Strategy tuning params
    p.add_argument('--rsi-buy', type=float, default=30.0, help='RSI threshold to consider buy')
    p.add_argument('--rsi-sell', type=float, default=70.0, help='RSI threshold to consider sell')
    p.add_argument('--hist-threshold', type=float, default=0.0, help='MACD histogram threshold (absolute)')
    p.add_argument('--ema-period', type=int, default=200, help='EMA period for trend filter (0 to disable)')
    p.add_argument('--use-trend', action='store_true', help='Enable EMA trend filter')
    p.add_argument('--sl-pct', type=float, default=0.02, help='Stop-loss as fraction of price (e.g., 0.02=2%)')
    p.add_argument('--tp-pct', type=float, default=0.04, help='Take-profit as fraction of price')
    p.add_argument('--trailing', action='store_true', help='Enable trailing stop (uses sl-pct)')
    p.add_argument('--atr-mult', type=float, default=0.0, help='Use ATR * mult as SL distance (overrides sl-pct when >0)')
    p.add_argument('--start-hour', type=int, default=0, help='Start hour (0-23) to allow trading')
    p.add_argument('--end-hour', type=int, default=23, help='End hour (0-23) to allow trading')
    p.add_argument('--mode', type=str, default='baseline', choices=['baseline','scalp'], help='Preset mode: scalp for scalping defaults')
    p.add_argument('--rsi-period', type=int, default=14, help='RSI period')
    p.add_argument('--macd-fast', type=int, default=12, help='MACD fast period')
    p.add_argument('--macd-slow', type=int, default=26, help='MACD slow period')
    p.add_argument('--macd-signal', type=int, default=9, help='MACD signal period')
    p.add_argument('--spread', type=float, default=0.0, help='Round-trip spread in price units (applied per trade)')
    p.add_argument('--slippage', type=float, default=0.0, help='Per-side slippage in price units')
    p.add_argument('--max-hold', type=int, default=None, help='Max bars to hold a trade; None disables')
    return p.parse_args()


def main():
    args = parse_args()

    print('Backtest RSI+MACD strategy')
    print(f"Symbol: {args.symbol}, months={args.months}, start_capital={args.capital}")

    if args.csv:
        if not os.path.exists(args.csv):
            print('CSV file not found:', args.csv)
            sys.exit(1)
        bars = read_csv_bars(args.csv)
    else:
        if not HAS_MT5:
            print('MetaTrader5 not available and no CSV supplied. Please provide --csv')
            sys.exit(1)
        print('Fetching bars from MT5...')
        bars = fetch_mt5_bars(args.symbol, args.months, timeframe=args.timeframe)

    if len(bars) < 100:
        print('Not enough bars for meaningful backtest:', len(bars))
        sys.exit(1)

    # Apply mode presets
    if getattr(args, 'mode', None) == 'scalp':
        print('Applying scalping presets')
        args.timeframe = 'M1'
        args.size_pct = 0.001
        args.rsi_period = 7
        args.rsi_buy = 20.0
        args.rsi_sell = 80.0
        args.hist_threshold = 0.01
        args.macd_fast = 6
        args.macd_slow = 13
        args.macd_signal = 5
        args.sl_pct = 0.002
        args.tp_pct = 0.004
        args.trailing = True
        args.atr_mult = 0.0
        args.max_hold = 5
        args.spread = 0.2
        args.slippage = 0.0

    res = run_backtest(
        bars,
        start_capital=args.capital,
        size_pct=args.size_pct,
        rsi_buy=args.rsi_buy,
        rsi_sell=args.rsi_sell,
        hist_thresh=args.hist_threshold,
        ema_period=args.ema_period,
        use_trend=args.use_trend,
        sl_pct=args.sl_pct,
        tp_pct=args.tp_pct,
        trailing=args.trailing,
        atr_mult=args.atr_mult,
        start_hour=args.start_hour,
        end_hour=args.end_hour,
        rsi_period=args.rsi_period,
        macd_fast=args.macd_fast,
        macd_slow=args.macd_slow,
        macd_signal=args.macd_signal,
        spread=args.spread,
        slippage=args.slippage,
        max_hold_bars=args.max_hold,
    )

    print('\nBacktest result:')
    print(f" Start capital: ${res['start_capital']:.2f}")
    print(f" End capital:   ${res['end_capital']:.2f}")
    print(f" Gross P&L:     ${res['gross_pnl']:.2f}")
    print(f" Trades:         {res['total_trades']}")
    print(f" Wins:           {res['wins']}")
    print(f" Losses:         {res['losses']}")
    print(f" Win rate:       {res['win_rate']:.2f}%")
    print(f" Max drawdown:   ${res['max_drawdown']:.2f}")

    # print trades sample
    if res['trades']:
        print('\nFirst 10 trades:')
        for t in res['trades'][:10]:
            if t['pnl'] is not None:
                print(f" {t['entry_time']} {t['side']} entry={t['entry_price']:.5f} exit={t['exit_price']:.5f} pnl={t['pnl']:+.2f}")


if __name__ == '__main__':
    main()
