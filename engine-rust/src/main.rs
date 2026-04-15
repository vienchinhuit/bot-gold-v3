use clap::Parser;
use serde_json::Value;
use chrono::{DateTime, NaiveDateTime, Utc, Local};
use uuid::Uuid;
use std::time::Duration;
use std::thread;
use std::env;
use std::io::{self, Write};
use atty::Stream;

use log::{info, warn, debug};
use env_logger::Env;
use env_logger::fmt::Target;

/// Simple Rust strategy engine: subscribe market ticks from python_bridge via ZMQ,
/// aggregate 1-minute bars, compute RSI & MACD, and optionally send orders back
/// to the bridge using a DEALER socket.
#[derive(Parser, Debug)]
#[command(author, version, about = "Rust strategy engine: 1-min RSI+MACD", long_about = None)]
struct Args {
    /// Market ZMQ address (PUB) to connect to
    #[arg(long, default_value = "tcp://127.0.0.1:5555")]
    market_addr: String,

    /// Order ZMQ address (ROUTER) to connect to when sending orders
    #[arg(long, default_value = "tcp://127.0.0.1:5556")]
    order_addr: String,

    /// If set, engine will send ORDER_SEND messages to the bridge
    #[arg(long, default_value_t = false)]
    trade: bool,

        /// Minimum seconds between placing two trades (cooldown)
        #[arg(long, default_value_t = 5)]
    cooldown: u64,

    /// Trade volume (lots)
    #[arg(long, default_value_t = 0.01)]
    volume: f64,

    /// Symbol to monitor (must match python_bridge symbols)
    #[arg(long, default_value = "GOLD")]
    symbol: String,
        /// Strategy mode: normal or scalp
        #[arg(long, default_value = "scalp")]
    mode: String,
    /// RSI period
    #[arg(long, default_value_t = 14usize)]
    rsi_period: usize,
    /// MACD fast period
    #[arg(long, default_value_t = 12usize)]
    macd_fast: usize,
    /// MACD slow period
    #[arg(long, default_value_t = 26usize)]
    macd_slow: usize,
    /// MACD signal period
    #[arg(long, default_value_t = 9usize)]
    macd_signal: usize,
    /// RSI buy threshold
    #[arg(long, default_value_t = 30.0)]
    rsi_buy: f64,
    /// RSI sell threshold
    #[arg(long, default_value_t = 70.0)]
    rsi_sell: f64,
    /// MACD histogram absolute threshold to consider
    #[arg(long, default_value_t = 0.0)]
    hist_threshold: f64,
    /// ATR period (used for spike detection)
    #[arg(long, default_value_t = 14usize)]
    atr_period: usize,
    /// Spike detection multiplier (bar_range > atr * spike_k)
    #[arg(long, default_value_t = 2.0)]
    spike_k: f64,
    /// Spike body ratio (body / range) threshold
    #[arg(long, default_value_t = 0.6)]
    spike_body_ratio: f64,
    /// Volume multiplier compared to average to confirm spike
    #[arg(long, default_value_t = 2.0)]
    spike_volume_factor: f64,
    /// Number of bars to average volume for spike detection
    #[arg(long, default_value_t = 20usize)]
    spike_volume_avg_period: usize,
    /// Scale trade volume when spike detected (e.g. 0.5 reduces size)
    #[arg(long, default_value_t = 0.5)]
    spike_volume_scale: f64,
    /// Multiply cooldown when spike detected
    #[arg(long, default_value_t = 2u64)]
    spike_cooldown_mult: u64,
    /// Spike handling mode: "strict" (require AND), "filter" (suppress), "none"
    #[arg(long, default_value = "strict")]
    spike_mode: String,
    /// Number of bars to wait for follow-through confirmation
    #[arg(long, default_value_t = 2usize)]
    follow_through_bars: usize,
    /// Lookback for simple divergence check (price vs RSI)
    #[arg(long, default_value_t = 5usize)]
    divergence_lookback: usize,
    /// Wick ratio to detect rejection candles (wick/ range)
    #[arg(long, default_value_t = 0.6)]
    spike_wick_ratio: f64,
    /// Require BOTH RSI and MACD to trigger (AND). Default false (uses OR)
    #[arg(long, default_value_t = false)]
    require_both: bool,
    /// Logging level (error,warn,info,debug,trace)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[derive(Debug, Clone)]
struct Bar {
    start_minute: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: i64,
}

fn compute_atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Option<f64> {
    if highs.len() < period + 1 || lows.len() < period + 1 || closes.len() < period + 1 {
        return None;
    }

    let n = highs.len();
    let mut sum = 0.0f64;
    for i in (n - period)..n {
        let prev_close = closes[i - 1];
        let tr1 = highs[i] - lows[i];
        let tr2 = (highs[i] - prev_close).abs();
        let tr3 = (lows[i] - prev_close).abs();
        let tr = tr1.max(tr2).max(tr3);
        sum += tr;
    }

    Some(sum / (period as f64))
}

fn avg_volume(vols: &[i64], period: usize) -> Option<f64> {
    if vols.len() < 1 {
        return None;
    }
    let n = vols.len();
    let start = if vols.len() > period { n - period } else { 0 };
    let slice = &vols[start..n];
    if slice.is_empty() {
        return None;
    }
    let sum: i64 = slice.iter().sum();
    Some(sum as f64 / (slice.len() as f64))
}

#[derive(Debug, Clone, PartialEq)]
enum SpikeClass {
    Pending,
    Continuation,
    Reversal,
    Uncertain,
}

#[derive(Debug, Clone)]
struct PendingSpike {
    extreme: f64,
    dir: i32,
    open: f64,
    high: f64,
    low: f64,
    created_idx: usize,
    age: usize,
    class: SpikeClass,
    init_vol: i64,
}

fn detect_divergence(closes: &[f64], rsi_period: usize, lookback: usize) -> Option<i32> {
    // returns Some(1) for bullish divergence, Some(-1) for bearish, None for none
    if closes.len() < rsi_period + 1 + lookback {
        return None;
    }

    let n = closes.len();
    let price_now = closes[n - 1];
    let price_prev = closes[n - 1 - lookback];

    // compute current RSI
    let rsi_now = compute_rsi(&closes, rsi_period)?;
    // compute previous RSI at earlier slice
    let prev_slice = &closes[..(n - lookback)];
    let rsi_prev = compute_rsi(prev_slice, rsi_period)?;

    // bearish divergence: price makes higher high but RSI lower high
    if price_now > price_prev && rsi_now < rsi_prev {
        return Some(-1);
    }
    // bullish divergence: price makes lower low but RSI higher low
    if price_now < price_prev && rsi_now > rsi_prev {
        return Some(1);
    }

    None
}

fn is_rejection_candle(bar: &Bar, dir: i32, wick_ratio: f64) -> bool {
    let range = bar.high - bar.low;
    if range <= 0.0 {
        return false;
    }
    let upper_wick = bar.high - bar.close;
    let lower_wick = bar.open - bar.low;

    if dir == 1 {
        // bullish spike — check for long upper wick (rejection)
        return upper_wick > range * wick_ratio;
    } else {
        // bearish spike — check for long lower wick (rejection)
        return lower_wick > range * wick_ratio;
    }
}

fn parse_iso_datetime(s: &str) -> Option<DateTime<Utc>> {
    if s.is_empty() {
        return None;
    }

    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try parsing naive ISO without timezone
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }

    None
}

fn ema_on_series(series: &[f64], period: usize) -> Option<f64> {
    if series.len() < period {
        return None;
    }
    let k = 2.0 / (period as f64 + 1.0);
    let mut ema = series[..period].iter().sum::<f64>() / (period as f64);
    for price in &series[period..] {
        ema = price * k + ema * (1.0 - k);
    }
    Some(ema)
}

fn compute_rsi(closes: &[f64], period: usize) -> Option<f64> {
    if closes.len() < period + 1 {
        return None;
    }

    let mut gains = 0.0;
    let mut losses = 0.0;
    let start = closes.len() - period - 1;
    for i in (start + 1)..closes.len() {
        let diff = closes[i] - closes[i - 1];
        if diff > 0.0 {
            gains += diff;
        } else {
            losses += -diff;
        }
    }

    let avg_gain = gains / (period as f64);
    let avg_loss = losses / (period as f64);

    if avg_loss == 0.0 {
        return Some(100.0);
    }

    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

/// Compute MACD line, signal line and histogram (current and previous histogram values).
fn compute_macd_signal(closes: &[f64], fast: usize, slow: usize, signal: usize) -> Option<(f64, f64, f64, f64)> {
    if closes.len() < slow + signal {
        return None;
    }

    let mut macd_vals: Vec<f64> = Vec::new();

    for i in (slow - 1)..closes.len() {
        let sub = &closes[..=i];
        if let (Some(f), Some(s)) = (ema_on_series(sub, fast), ema_on_series(sub, slow)) {
            macd_vals.push(f - s);
        }
    }

    if macd_vals.len() < signal + 1 {
        return None;
    }

    // Build signal EMA over macd_vals
    let mut signal_ema: Vec<f64> = Vec::new();
    let sma = macd_vals[..signal].iter().sum::<f64>() / (signal as f64);
    signal_ema.push(sma);
    let k = 2.0 / (signal as f64 + 1.0);
    for j in signal..macd_vals.len() {
        let prev = *signal_ema.last().unwrap();
        let next = macd_vals[j] * k + prev * (1.0 - k);
        signal_ema.push(next);
    }

    if signal_ema.len() < 2 || macd_vals.len() < 2 {
        return None;
    }

    let macd_curr = macd_vals[macd_vals.len() - 1];
    let macd_prev = macd_vals[macd_vals.len() - 2];
    let sig_curr = signal_ema[signal_ema.len() - 1];
    let sig_prev = signal_ema[signal_ema.len() - 2];

    let hist_curr = macd_curr - sig_curr;
    let hist_prev = macd_prev - sig_prev;

    Some((macd_curr, sig_curr, hist_curr, hist_prev))
}

fn main() {
    let args = Args::parse();

    // Initialize console logger with simple timestamped format. Respect RUST_LOG or --log-level.
    let log_env = Env::default().filter_or("RUST_LOG", &args.log_level);
    env_logger::Builder::from_env(log_env)
        .format(|buf, record| {
            use std::io::Write;
            let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
            writeln!(buf, "{} {:<5} {}", ts, record.level(), record.args())
        })
        .target(Target::Stdout)
        .init();

    // Allow overriding addresses via environment variables
    let market_addr = env::var("ZMQ_MARKET_ADDR").unwrap_or_else(|_| args.market_addr.clone());
    let order_addr = env::var("ZMQ_ORDER_ADDR").unwrap_or_else(|_| args.order_addr.clone());

    info!("Starting Rust strategy engine (symbol={})", args.symbol);

    // ZeroMQ context and sockets
    let ctx = zmq::Context::new();
    let sub = ctx.socket(zmq::SUB).expect("failed to create SUB socket");
    sub.connect(&market_addr).expect("failed to connect SUB");
    info!("Connected to market publisher at {}", market_addr);
    sub.set_subscribe(b"").expect("failed to subscribe");
    info!("Subscribed to market feed (all topics)");

    let mut dealer = if args.trade {
        let s = ctx.socket(zmq::DEALER).expect("failed to create DEALER socket");
        s.connect(&order_addr).expect("failed to connect DEALER");
        info!("Connected to order router at {}", order_addr);
        // Short receive timeout so strategy doesn't block forever waiting for broker reply
        s.set_rcvtimeo(5000).ok();
        Some(s)
    } else {
        None
    };

    let mut current_bar: Option<Bar> = None;
    let mut closes: Vec<f64> = Vec::new();
    // Series for ATR/volume/spike checks
    let mut highs: Vec<f64> = Vec::new();
    let mut lows: Vec<f64> = Vec::new();
    let mut bar_vols: Vec<i64> = Vec::new();
    let mut pending_spike: Option<PendingSpike> = None;
    let mut last_action_time: Option<DateTime<Utc>> = None;

    // Strategy parameters (can be overridden by mode presets)
    let mut rsi_period = args.rsi_period;
    let mut macd_fast = args.macd_fast;
    let mut macd_slow = args.macd_slow;
    let mut macd_signal = args.macd_signal;
    let mut rsi_buy = args.rsi_buy;
    let mut rsi_sell = args.rsi_sell;
    let mut hist_threshold = args.hist_threshold;
    let mut cooldown_secs = args.cooldown;
    let mut trade_volume = args.volume;

    if args.mode.to_lowercase() == "scalp" {
        // scalping presets (adjust for live trading as needed)
        rsi_period = 7;
        macd_fast = 6;
        macd_slow = 13;
        macd_signal = 5;
        rsi_buy = 20.0;
        rsi_sell = 80.0;
        hist_threshold = 0.01;
        cooldown_secs = 5; // seconds between signals
        // keep trade_volume as provided by user or default
        info!("Scalp mode applied: RSI={} MACD={}|{}|{} rsi_buy={} rsi_sell={} hist_thresh={} cooldown={}s", rsi_period, macd_fast, macd_slow, macd_signal, rsi_buy, rsi_sell, hist_threshold, cooldown_secs);
    }

    // Interactive test order prompt (default No). Only prompt if stdin is a TTY.
    if atty::is(Stream::Stdin) {
        print!("Send a test ORDER to python_bridge now? (y/N): ");
        io::stdout().flush().ok();

        let mut answer = String::new();
        if let Ok(_) = io::stdin().read_line(&mut answer) {
            let a = answer.trim().to_lowercase();
            if a == "y" || a == "yes" {
                // Ensure dealer socket exists (create if not)
                if dealer.is_none() {
                    let s = ctx.socket(zmq::DEALER).expect("failed to create DEALER socket");
                    s.connect(&order_addr).expect("failed to connect DEALER");
                    s.set_rcvtimeo(5000).ok();
                    info!("Connected to order router at {}", order_addr);
                    dealer = Some(s);
                }

                if let Some(ref sock) = dealer {
                    let request_id = Uuid::new_v4().to_string();
                    let payload = serde_json::json!({
                        "type": "ORDER_SEND",
                        "data": {
                            "symbol": args.symbol,
                            "volume": trade_volume,
                            "order_type": "BUY",
                            "price": 0,
                            "stop_loss": serde_json::Value::Null,
                            "take_profit": serde_json::Value::Null,
                            "comment": "rust-test-order",
                            "magic": 9999,
                            "request_id": request_id
                        }
                    });

                    let s = payload.to_string();
                    info!("Sending test ORDER to bridge: {}", s);
                    match sock.send(s.as_bytes(), 0) {
                        Ok(_) => {
                            match sock.recv_string(0) {
                                Ok(Ok(resp)) => info!("Test order response: {}", resp),
                                Ok(Err(_)) => warn!("Test order: non-utf8 reply from bridge"),
                                Err(e) => warn!("Test order: no reply or recv error: {:?}", e),
                            }
                        }
                        Err(e) => warn!("Failed to send test order: {:?}", e),
                    }
                }
            } else {
                info!("Skipping test ORDER (default No)");
            }
        }
    } else {
        debug!("Stdin not a TTY; skipping interactive test-order prompt");
    }

    loop {
        match sub.recv_string(0) {
            Ok(Ok(msg)) => {
                debug!("raw message: {}", msg);

                let v: Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("invalid json from publisher: {}", e);
                        continue;
                    }
                };

                let msg_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

                if msg_type != "TICK" {
                    // ignore HEARTBEAT / other types for now
                    continue;
                }

                let data = &v["data"];
                let symbol = data.get("symbol").and_then(|x| x.as_str()).unwrap_or("");
                if symbol != args.symbol {
                    continue;
                }

                let last_price = data.get("last").and_then(|x| x.as_f64())
                    .or_else(|| data.get("bid").and_then(|x| x.as_f64()))
                    .unwrap_or(0.0);
                let volume = data.get("volume").and_then(|x| x.as_i64()).unwrap_or(0);
                let time_str = data.get("server_time").and_then(|x| x.as_str())
                    .or_else(|| data.get("time").and_then(|x| x.as_str()))
                    .unwrap_or("");

                let dt = match parse_iso_datetime(time_str) {
                    Some(d) => d,
                    None => {
                        warn!("failed parse timestamp: {}", time_str);
                        continue;
                    }
                };

                let minute = dt.timestamp() / 60;

                debug!("Received TICK {} price={:.5} time={}", symbol, last_price, dt);

                if let Some(bar) = current_bar.as_mut() {
                    if bar.start_minute == minute {
                        // update current bar
                        bar.close = last_price;
                        if last_price > bar.high { bar.high = last_price; }
                        if last_price < bar.low { bar.low = last_price; }
                        bar.volume += volume;

                        // --- Per-tick evaluation (use temporary series including current bar) ---
                        let mut temp_closes = closes.clone();
                        temp_closes.push(bar.close);
                        let mut temp_highs = highs.clone();
                        temp_highs.push(bar.high);
                        let mut temp_lows = lows.clone();
                        temp_lows.push(bar.low);
                        let mut temp_vols = bar_vols.clone();
                        temp_vols.push(bar.volume);

                        let temp_len = temp_closes.len();
                        let rsi_required = rsi_period + 1;
                        let macd_required = macd_slow + macd_signal;

                        if temp_len >= macd_required {
                            if let Some((macd_curr, sig_curr, hist_curr, hist_prev)) = compute_macd_signal(&temp_closes, macd_fast, macd_slow, macd_signal) {
                                // compute RSI on temp series
                                if let Some(rsi) = compute_rsi(&temp_closes, rsi_period) {
                                    // detect spike using temp series
                                    let mut is_spike = false;
                                    let mut spike_dir = 0i32;
                                    if let Some(atr) = compute_atr(&temp_highs, &temp_lows, &temp_closes, args.atr_period) {
                                        let bar_range = bar.high - bar.low;
                                        let body = (bar.close - bar.open).abs();
                                        let vol_avg = avg_volume(&temp_vols, args.spike_volume_avg_period);
                                        let vol_ok = vol_avg.map_or(true, |avg| (bar.volume as f64) > avg * args.spike_volume_factor);
                                        if (bar_range > atr * args.spike_k || body > bar_range * args.spike_body_ratio) && vol_ok {
                                            is_spike = true;
                                            spike_dir = if bar.close > bar.open { 1 } else { -1 };
                                            debug!("Per-tick spike detected: range={:.5} atr={:.5} body={:.5} vol={} avg_vol={:?} dir={}", bar_range, atr, body, bar.volume, vol_avg, spike_dir);
                                        }
                                    }

                                    let rsi_buy_cond = rsi < rsi_buy;
                                    let rsi_sell_cond = rsi > rsi_sell;
                                    let macd_buy = hist_curr > hist_threshold && hist_prev < hist_curr;
                                    let macd_sell = hist_curr < -hist_threshold && hist_prev > hist_curr;

                                    let mut buy_cond: bool;
                                    let mut sell_cond: bool;
                                    if args.require_both {
                                        buy_cond = rsi_buy_cond && macd_buy;
                                        sell_cond = rsi_sell_cond && macd_sell;
                                    } else {
                                        buy_cond = rsi_buy_cond || macd_buy;
                                        sell_cond = rsi_sell_cond || macd_sell;
                                    }

                                    // apply spike filter if requested
                                    if args.spike_mode.to_lowercase().as_str() == "filter" && is_spike {
                                        buy_cond = false;
                                        sell_cond = false;
                                    }

                                    let mut eff_volume = trade_volume;
                                    let mut eff_cooldown = cooldown_secs;
                                    if let Some(ps) = pending_spike.as_ref() {
                                        match ps.class {
                                            SpikeClass::Continuation => {}
                                            SpikeClass::Reversal => {
                                                if ps.dir == 1 { buy_cond = false; } else { sell_cond = false; }
                                            }
                                            SpikeClass::Uncertain | SpikeClass::Pending => {
                                                eff_volume = trade_volume * args.spike_volume_scale;
                                                eff_cooldown = cooldown_secs * args.spike_cooldown_mult;
                                            }
                                        }
                                    } else if is_spike {
                                        eff_volume = trade_volume * args.spike_volume_scale;
                                        eff_cooldown = cooldown_secs * args.spike_cooldown_mult;
                                    }

                                    let mut action = "HOLD";
                                    if buy_cond { action = "BUY"; } else if sell_cond { action = "SELL"; }

                                    if action != "HOLD" {
                                        let now = Utc::now();
                                        let do_send = match last_action_time {
                                            Some(t) => (now - t).num_seconds() >= eff_cooldown as i64,
                                            None => true,
                                        };

                                        if do_send {
                                            if args.trade {
                                                if let Some(ref sock) = dealer {
                                                    let order_type = if action == "BUY" { "BUY" } else { "SELL" };
                                                    let request_id = Uuid::new_v4().to_string();
                                                    let payload = serde_json::json!({
                                                        "type": "ORDER_SEND",
                                                        "data": {
                                                            "symbol": args.symbol,
                                                            "volume": eff_volume,
                                                            "order_type": order_type,
                                                            "price": 0,
                                                            "stop_loss": serde_json::Value::Null,
                                                            "take_profit": serde_json::Value::Null,
                                                            "comment": format!("rust-tick:{}", action),
                                                            "magic": 2000,
                                                            "request_id": request_id
                                                        }
                                                    });
                                                    let s = payload.to_string();
                                                    info!("Tick ORDER {} {} lots (comment={})", order_type, eff_volume, format!("rust-tick:{}", action));
                                                    debug!("tick order payload: {}", s);
                                                    match sock.send(s.as_bytes(), 0) {
                                                        Ok(_) => {
                                                            match sock.recv_string(0) {
                                                                Ok(Ok(resp)) => info!("tick order response: {}", resp),
                                                                Ok(Err(_)) => warn!("tick order: non-utf8 reply"),
                                                                Err(e) => warn!("tick order: no reply or recv error: {:?}", e),
                                                            }
                                                        }
                                                        Err(e) => warn!("tick order send failed: {:?}", e),
                                                    }
                                                }
                                            } else {
                                                info!("Tick signal {} generated (trade disabled).", action);
                                            }
                                            last_action_time = Some(now);
                                        }
                                    }

                                    // register pending spike on tick if detected
                                    if is_spike {
                                        let create_new = match pending_spike.as_ref() {
                                            Some(ps) => ps.class != SpikeClass::Pending,
                                            None => true,
                                        };
                                        if create_new {
                                            pending_spike = Some(PendingSpike {
                                                extreme: if spike_dir == 1 { bar.high } else { bar.low },
                                                dir: spike_dir,
                                                open: bar.open,
                                                high: bar.high,
                                                low: bar.low,
                                                created_idx: temp_len - 1,
                                                age: 0,
                                                class: SpikeClass::Pending,
                                                init_vol: bar.volume,
                                            });
                                            info!("Pending spike (tick) registered dir={} extreme={:.5}", spike_dir, if spike_dir==1 { bar.high } else { bar.low });
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        // finalize previous bar
                        let finished = current_bar.take().unwrap();
                        // push to series
                        highs.push(finished.high);
                        lows.push(finished.low);
                        bar_vols.push(finished.volume);
                        closes.push(finished.close);

                        // Diagnostics: print finalized bar summary (include open)
                        info!("Finalized bar: start_minute={} open={:.5} high={:.5} low={:.5} close={:.5} vol={}",
                              finished.start_minute, finished.open, finished.high, finished.low, finished.close, finished.volume);

                        // Show closes summary (debug)
                        let closes_len = closes.len();
                        let tail = if closes_len > 20 { 20 } else { closes_len };
                        if tail > 0 {
                            debug!("Closes (last {}): {:?}", tail, &closes[closes_len - tail..]);
                        }

                        // If there is a pending spike from a previous bar, evaluate follow-through / reversal
                        if let Some(ps) = pending_spike.as_mut() {
                            ps.age += 1;
                            if ps.class == SpikeClass::Pending {
                                // check simple follow-through
                                if (ps.dir == 1 && finished.close > ps.extreme) || (ps.dir == -1 && finished.close < ps.extreme) {
                                    ps.class = SpikeClass::Continuation;
                                    info!("Pending spike confirmed continuation (dir={})", ps.dir);
                                } else if is_rejection_candle(&finished, ps.dir, args.spike_wick_ratio) {
                                    ps.class = SpikeClass::Reversal;
                                    info!("Pending spike classified Reversal by rejection wick (dir={})", ps.dir);
                                } else if let Some(div) = detect_divergence(&closes, rsi_period, args.divergence_lookback) {
                                    if div == -ps.dir {
                                        ps.class = SpikeClass::Reversal;
                                        info!("Pending spike classified Reversal by divergence (div={})", div);
                                    }
                                } else if ps.age >= args.follow_through_bars {
                                    ps.class = SpikeClass::Uncertain;
                                    info!("Pending spike marked Uncertain after {} bars", ps.age);
                                }
                            }
                        }

                        // Compute RSI and MACD with readiness reporting using configured periods
                        let rsi_required = rsi_period + 1; // need period+1 closes
                        if closes_len >= rsi_required {
                            if let Some(rsi) = compute_rsi(&closes, rsi_period) {
                                info!("RSI({}) = {:.4}", rsi_period, rsi);
                            }
                        } else {
                            let need = rsi_required - closes_len;
                            info!("RSI not ready: need {} more closes (have {})", need, closes_len);
                        }

                        let macd_required = macd_slow + macd_signal; // slow + signal
                        if closes_len >= macd_required {
                            if let Some((macd_curr, sig_curr, hist_curr, hist_prev)) = compute_macd_signal(&closes, macd_fast, macd_slow, macd_signal) {
                                info!("MACD: macd={:.6} sig={:.6} hist={:.6} prev_hist={:.6}", macd_curr, sig_curr, hist_curr, hist_prev);

                                // Evaluate entry conditions and report booleans
                                if let Some(rsi) = compute_rsi(&closes, rsi_period) {
                                    // detect spike on the finished bar (current bar)
                                    let mut is_spike = false;
                                    let mut spike_dir = 0i32;
                                    if let Some(atr) = compute_atr(&highs, &lows, &closes, args.atr_period) {
                                        let bar_range = finished.high - finished.low;
                                        let body = (finished.close - finished.open).abs();
                                        let vol_avg = avg_volume(&bar_vols, args.spike_volume_avg_period);
                                        let vol_ok = vol_avg.map_or(true, |avg| (finished.volume as f64) > avg * args.spike_volume_factor);
                                        if (bar_range > atr * args.spike_k || body > bar_range * args.spike_body_ratio) && vol_ok {
                                            is_spike = true;
                                            spike_dir = if finished.close > finished.open { 1 } else { -1 };
                                            info!("Spike detected: range={:.5} atr={:.5} body={:.5} vol={} avg_vol={:?} dir={}", bar_range, atr, body, finished.volume, vol_avg, spike_dir);
                                        }
                                    }

                                    let rsi_buy_cond = rsi < rsi_buy;
                                    let rsi_sell_cond = rsi > rsi_sell;
                                    let macd_buy = hist_curr > hist_threshold && hist_prev < hist_curr;
                                    let macd_sell = hist_curr < -hist_threshold && hist_prev > hist_curr;

                                    let mut buy_cond: bool;
                                    let mut sell_cond: bool;
                                    if args.require_both {
                                        buy_cond = rsi_buy_cond && macd_buy;
                                        sell_cond = rsi_sell_cond && macd_sell;
                                    } else {
                                        buy_cond = rsi_buy_cond || macd_buy;
                                        sell_cond = rsi_sell_cond || macd_sell;
                                    }

                                    // apply spike filter if requested
                                    if args.spike_mode.to_lowercase().as_str() == "filter" && is_spike {
                                        buy_cond = false;
                                        sell_cond = false;
                                    }

                                    // Adjust behavior based on any existing pending spike
                                    let mut eff_volume = trade_volume;
                                    let mut eff_cooldown = cooldown_secs;
                                    if let Some(ps) = pending_spike.as_ref() {
                                        match ps.class {
                                            SpikeClass::Continuation => {
                                                // allow entries aligned with spike, no special penalty
                                            }
                                            SpikeClass::Reversal => {
                                                // suppress signals aligned with spike direction
                                                if ps.dir == 1 { buy_cond = false; }
                                                else { sell_cond = false; }
                                            }
                                            SpikeClass::Uncertain | SpikeClass::Pending => {
                                                eff_volume = trade_volume * args.spike_volume_scale;
                                                eff_cooldown = cooldown_secs * args.spike_cooldown_mult;
                                            }
                                        }
                                    } else if is_spike {
                                        // conservative immediate handling for a newly detected spike
                                        eff_volume = trade_volume * args.spike_volume_scale;
                                        eff_cooldown = cooldown_secs * args.spike_cooldown_mult;
                                    }

                                    info!("Conditions: rsi={:.2} rsi_buy={} rsi_sell={} macd_buy={} macd_sell={} spike={} pending={:?}",
                                          rsi, rsi_buy_cond, rsi_sell_cond, macd_buy, macd_sell, is_spike, pending_spike.as_ref().map(|p| &p.class));

                                    let mut action = "HOLD";
                                    if buy_cond {
                                        action = "BUY";
                                    } else if sell_cond {
                                        action = "SELL";
                                    }

                                    info!("Decision => {}", action);

                                    if action != "HOLD" {
                                        let now = Utc::now();
                                        let do_send = match last_action_time {
                                            Some(t) => (now - t).num_seconds() >= eff_cooldown as i64,
                                            None => true,
                                        };

                                        if do_send {
                                            if args.trade {
                                                if let Some(ref sock) = dealer {
                                                    let order_type = if action == "BUY" { "BUY" } else { "SELL" };
                                                    let request_id = Uuid::new_v4().to_string();

                                                    let payload = serde_json::json!({
                                                        "type": "ORDER_SEND",
                                                        "data": {
                                                            "symbol": args.symbol,
                                                            "volume": eff_volume,
                                                            "order_type": order_type,
                                                            "price": 0,
                                                            "stop_loss": serde_json::Value::Null,
                                                            "take_profit": serde_json::Value::Null,
                                                            "comment": format!("rust-strategy:{}", action),
                                                            "magic": 2000,
                                                            "request_id": request_id
                                                        }
                                                    });

                                                    let s = payload.to_string();
                                                    info!("Sending ORDER {} {} lots (comment={})", order_type, eff_volume, format!("rust-strategy:{}", action));
                                                    debug!("order payload: {}", s);
                                                    match sock.send(s.as_bytes(), 0) {
                                                        Ok(_) => {
                                                            match sock.recv_string(0) {
                                                                Ok(Ok(resp)) => info!("order response: {}", resp),
                                                                Ok(Err(_)) => warn!("non-utf8 reply from bridge"),
                                                                Err(e) => warn!("no reply or error receiving reply: {:?}", e),
                                                            }
                                                        }
                                                        Err(e) => warn!("failed to send order: {:?}", e),
                                                    }
                                                }
                                            } else {
                                                info!("Signal {} generated (trade disabled).", action);
                                            }

                                            last_action_time = Some(now);
                                        } else {
                                            debug!("Signal {} suppressed due cooldown (effective_cooldown={}s)", action, eff_cooldown);
                                        }
                                    }

                                    // If this finished bar is a spike, create pending_spike (unless one is already pending)
                                    if is_spike {
                                        let create_new = match pending_spike.as_ref() {
                                            Some(ps) => ps.class != SpikeClass::Pending,
                                            None => true,
                                        };
                                        if create_new {
                                            pending_spike = Some(PendingSpike {
                                                extreme: if spike_dir == 1 { finished.high } else { finished.low },
                                                dir: spike_dir,
                                                open: finished.open,
                                                high: finished.high,
                                                low: finished.low,
                                                created_idx: closes_len,
                                                age: 0,
                                                class: SpikeClass::Pending,
                                                init_vol: finished.volume,
                                            });
                                            info!("Pending spike registered (dir={} extreme={:.5})", spike_dir, if spike_dir==1 { finished.high } else { finished.low });
                                        }
                                    }
                                } else {
                                    info!("RSI unexpectedly not ready despite macd_ready");
                                }
                            }
                        } else {
                            let need = macd_required - closes_len;
                            info!("MACD not ready: need {} more closes (have {})", need, closes_len);
                        }

                        // start new bar from current tick (record open)
                        current_bar = Some(Bar {
                            start_minute: minute,
                            open: last_price,
                            high: last_price,
                            low: last_price,
                            close: last_price,
                            volume: volume,
                        });
                    }
                } else {
                    // initialize first bar
                    current_bar = Some(Bar {
                        start_minute: minute,
                        open: last_price,
                        high: last_price,
                        low: last_price,
                        close: last_price,
                        volume: volume,
                    });
                }
            }
            Ok(Err(e)) => {
                warn!("recv_string error: {:?}", e);
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                warn!("zmq recv error: {:?}", e);
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
}
