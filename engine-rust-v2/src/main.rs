// ============================================================
// ADVANCED SCALPING ENGINE v2.0 - Production-Ready System
// ============================================================

use clap::Parser;
use chrono::{DateTime, NaiveDateTime, Utc, Local};
use std::collections::VecDeque;
use std::time::Duration;
use std::thread;
use uuid::Uuid;
use log::{info, warn, debug, error};
use env_logger::Env;

mod strategy_new;
mod slack;
use slack::SlackClient;
use strategy_new::{
    Config, State, Candle, SignalAction, Direction,
    should_trade, calc_ema, calc_rsi, calc_atr,
    is_pullback, detect_structure,
    SwingType,
};
use serde_json::Value;

/// Advanced Scalping Engine v2.0
/// Strategy: Market Structure + Pullback + Scoring System
#[derive(Parser, Debug)]
#[command(author, version, about = "Advanced Scalping Engine v2.0", long_about = None)]
struct Args {
    /// Market ZMQ address (PUB)
    #[arg(long, default_value = "tcp://127.0.0.1:5555")]
    market_addr: String,
    
    /// Order ZMQ address (ROUTER)
    #[arg(long, default_value = "tcp://127.0.0.1:5556")]
    order_addr: String,
    
    /// Enable actual trading
    #[arg(long, default_value_t = false)]
    trade: bool,
    
    /// Symbol to trade
    #[arg(long, default_value = "GOLD")]
    symbol: String,
    
    /// Minimum cooldown between trades (seconds)
    #[arg(long, default_value_t = 5)]
    cooldown_sec: u64,
    
        /// Trade volume (lots)
    #[arg(long, default_value_t = 0.01)]
    volume: f64,
    
    /// Maximum volume per trade (lots)
    #[arg(long, default_value_t = 0.10)]
    max_volume_per_trade: f64,
    
    /// Maximum total volume across all positions (lots)
    #[arg(long, default_value_t = 0.50)]
    max_total_volume: f64,
    
    /// Minimum score to enter trade
    #[arg(long, default_value_t = 5)]
    min_score: i32,
    
    /// Minimum confidence (0.0-1.0)
    #[arg(long, default_value_t = 0.5)]
    min_confidence: f64,
    
    /// Sideway EMA threshold
    #[arg(long, default_value_t = 0.30)]
    sideway_threshold: f64,
    
    /// Min trend strength (EMA distance)
    #[arg(long, default_value_t = 0.20)]
    min_trend_strength: f64,
    
    /// Max pullback distance from EMA (pips)
    #[arg(long, default_value_t = 15.0)]
    max_pullback_pips: f64,
    
    /// Max FOMO distance (pips)
    #[arg(long, default_value_t = 25.0)]
    max_fomo_pips: f64,
    
    /// Cooldown candles after loss
    #[arg(long, default_value_t = 15)]
    cooldown_candles: usize,
    
    /// Max consecutive losses before pause
    #[arg(long, default_value_t = 3)]
    max_losses: usize,
    
    /// Pause duration after max losses (minutes)
    #[arg(long, default_value_t = 30)]
    pause_minutes: i64,
    
    /// Max candle size multiplier (vs ATR)
    #[arg(long, default_value_t = 1.5)]
    max_candle_mult: f64,
    
    /// SL multiplier (ATR * this)
    #[arg(long, default_value_t = 1.2)]
    sl_mult: f64,
    
    /// TP multiplier (ATR * this)
    #[arg(long, default_value_t = 2.0)]
    tp_mult: f64,
    
    /// Verbose tick logging
    #[arg(long, default_value_t = false)]
    verbose: bool,
    
        /// Logging level
    #[arg(long, default_value = "info")]
    log_level: String,
    
    /// Enable Slack notifications
    #[arg(long, default_value_t = false)]
    slack_enabled: bool,
    
    /// Slack webhook URL
    #[arg(long, default_value = "")]
    slack_webhook: String,
    
        /// Slack channel (e.g., #trading, #alerts)
    #[arg(long, default_value = "#trading")]
    slack_channel: String,
    
    /// Port to receive position close notifications (for Slack)
    #[arg(long, default_value_t = 0)]
    slack_notify_port: u16,
}

/// Candle builder for M1 timeframe
struct CandleBuilder {
    open: Option<f64>,
    high: f64,
    low: f64,
    close: f64,
    volume: i64,
    start_time: i64,
    current_time: i64,
}


impl CandleBuilder {
    fn new() -> Self {
        Self {
            open: None,
            high: f64::NEG_INFINITY,
            low: f64::INFINITY,
            close: 0.0,
            volume: 0,
            start_time: 0,
            current_time: 0,
        }
    }
    
    fn update(&mut self, price: f64, volume: i64, timestamp: i64) {
        let minute = timestamp / 60;
        let current_minute = self.current_time / 60;
        
        if minute != current_minute || self.start_time == 0 {
            self.new_candle(timestamp);
        }
        
        if self.open.is_none() {
            self.open = Some(price);
            self.high = price;
            self.low = price;
        }
        
        self.high = self.high.max(price);
        self.low = self.low.min(price);
        self.close = price;
        self.volume += volume;
        self.current_time = timestamp;
    }
    
    fn new_candle(&mut self, _timestamp: i64) {
        self.open = None;
        self.high = f64::NEG_INFINITY;
        self.low = f64::INFINITY;
        self.close = 0.0;
        self.volume = 0;
        self.start_time = self.current_time;
    }
    
    fn build(&self, timestamp: i64) -> Option<Candle> {
        self.open.map(|open| Candle {
            time: timestamp,
            open,
            high: self.high,
            low: self.low,
            close: self.close,
            volume: self.volume,
        })
    }
    
    fn is_complete(&self) -> bool {
        self.open.is_some() && self.current_time > self.start_time
    }
}

fn parse_iso_datetime(s: &str) -> Option<DateTime<Utc>> {
    if s.is_empty() { return None; }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) { return Some(dt.with_timezone(&Utc)); }
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
    }
    None
}

fn resolve_symbol_alias(raw: &str) -> String {
    // Don't convert - keep original symbol
    // Your MT5 broker might use different symbol names
    // Test with both if needed
    raw.to_uppercase()
}

/// Create visual bar string
fn make_bar(filled: usize, total: usize) -> String {
    format!("{}{}", "█".repeat(filled), "-".repeat(total - filled))
}

fn main() {
    let args = Args::parse();
    let resolved_symbol = resolve_symbol_alias(&args.symbol);
    
    // Initialize logging
    let log_env = Env::default().filter_or("RUST_LOG", &args.log_level);
    env_logger::Builder::from_env(log_env)
        .format(|buf, record| {
            use std::io::Write;
            let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            writeln!(buf, "{} {:<5} [{}] {}",
                ts, record.level(), thread::current().name().unwrap_or("main"), record.args())
        })
        .init();
    
        info!("============================================================");
    info!("  ADVANCED SCALPING ENGINE v2.0");
    info!("============================================================");
    info!("Symbol: {} (input: {}) | Trading: {}", resolved_symbol, args.symbol, if args.trade { "ENABLED" } else { "DISABLED" });
    info!("Filters: Sideway={} | Trend={} | Pullback={}pips | FOMO={}pips",
        args.sideway_threshold, args.min_trend_strength, args.max_pullback_pips, args.max_fomo_pips);
        info!("Risk: SL={}*ATR | TP={}*ATR | MinScore={} | MinConf={:.2}",
        args.sl_mult, args.tp_mult, args.min_score, args.min_confidence);
    info!("Volume: {}-{} lots/trade | MaxTotal={} lots",
        args.volume, args.max_volume_per_trade, args.max_total_volume);
        info!("Cooldown: {} candles | MaxLosses={} | Pause={}min",
        args.cooldown_candles, args.max_losses, args.pause_minutes);
        info!("Slack: {} | Channel: {} | Webhook: {}",
        if args.slack_enabled { "ENABLED" } else { "DISABLED" },
        args.slack_channel,
        if args.slack_webhook.is_empty() { "NOT SET" } else { "CONFIGURED" }
    );
    if args.slack_notify_port > 0 {
        info!("Position Close Notify: tcp://*:{} (ZMQ SUB)", args.slack_notify_port);
    } else {
        info!("Position Close Notify: DISABLED");
    }
    info!("============================================================");
    
    // Initialize configuration
    let config = Config {
        ema_fast: 20,
        ema_slow: 50,
        rsi_period: 14,
        atr_period: 14,
        sideway_ema_threshold: args.sideway_threshold,
        min_trend_strength: args.min_trend_strength,
        max_pullback_pips: args.max_pullback_pips,
        max_fomo_pips: args.max_fomo_pips,
        rsi_oversold: 30.0,
        rsi_overbought: 70.0,
        rsi_sell_confirm_low: 50.0,
        rsi_sell_confirm_high: 60.0,
        rsi_buy_confirm_low: 40.0,
        rsi_buy_confirm_high: 50.0,
        max_candle_mult: args.max_candle_mult,
        max_wick_ratio: 0.5,
        min_score: args.min_score,
        min_confidence: args.min_confidence,
        sl_mult: args.sl_mult,
        tp_mult: args.tp_mult,
        pip_value: 0.01,  // XAUUSD
        cooldown_after_loss: args.cooldown_candles,
        max_consecutive_losses: args.max_losses,
        pause_duration_minutes: args.pause_minutes,
        max_positions_per_direction: 10,
        no_trade_zone_pips: 100.0,
    };
    
        // Initialize state
    let mut state = State::new();
    
    // Reset positions on startup (MT5 tracks positions separately)
    state.long_positions = 0;
    state.short_positions = 0;
    info!("Position tracking reset for fresh start");
    
    // Candle builder
    let mut candle_builder = CandleBuilder::new();
    let mut last_completed_candle: Option<Candle> = None;
    
    // Price history
    let mut price_history: VecDeque<f64> = VecDeque::with_capacity(100);
    let mut high_history: VecDeque<f64> = VecDeque::with_capacity(100);
    let mut low_history: VecDeque<f64> = VecDeque::with_capacity(100);
    
        // Cooldown tracking
        let mut last_action_time: Option<DateTime<Utc>> = None;
        let mut last_status_time: Option<DateTime<Utc>> = None;
        let mut last_heartbeat_time: Option<DateTime<Utc>> = None;
    
    // ZMQ setup
    let ctx = zmq::Context::new();
    
    // Market data subscriber
    let sub = ctx.socket(zmq::SUB).expect("Failed to create SUB socket");
    sub.connect(&args.market_addr).expect("Failed to connect to market publisher");
    sub.set_subscribe(b"").expect("Failed to subscribe");
    info!("Connected to market feed: {}", args.market_addr);
    
        // Order dealer (optional)
    let dealer = if args.trade {
        let s = ctx.socket(zmq::DEALER).expect("Failed to create DEALER socket");
        s.connect(&args.order_addr).expect("Failed to connect to order router");
        s.set_rcvtimeo(5000).ok();
        info!("Connected to order router: {}", args.order_addr);
        Some(s)
    } else {
        info!("Trading DISABLED - signals only");
        None
    };
    
        // Slack client initialization
    let slack = SlackClient::new(args.slack_enabled, args.slack_webhook.clone(), args.slack_channel.clone());
    if slack.is_enabled() {
        if slack.is_configured() {
            info!("✅ Slack ENABLED - Channel: {} | Webhook: configured", slack.get_channel());
            match slack.send_status("GOLD Scalping Bot v2.0 started") {
                Ok(_) => info!("✅ Slack test message sent successfully"),
                Err(e) => warn!("⚠️ Slack test message failed: {}", e),
            }
        } else {
            warn!("⚠️ Slack ENABLED but no webhook URL configured - messages will be skipped");
        }
    } else {
        info!("⏸️ Slack notifications DISABLED");
    }
    
    // ZMQ subscriber for position close notifications (from order_monitor.py)
    let notify_socket = if args.slack_notify_port > 0 {
        let ctx = zmq::Context::new();
                let sub = ctx.socket(zmq::SUB).expect("Failed to create SUB socket");
        let addr = format!("tcp://localhost:{}", args.slack_notify_port);
        sub.connect(&addr).expect("Failed to connect notify socket");
        // Subscribe to all messages (publisher does not add a topic prefix)
        sub.set_subscribe(b"").expect("Failed to subscribe");
        info!("✅ Position close notifications subscriber connected to {} (for Slack)", addr);
        Some(sub)
    } else {
        info!("⏸️ Position close notifications DISABLED");
        None
    };
    
    info!("Starting main trading loop...");
    
    loop {
        match sub.recv_string(0) {
            Ok(Ok(msg)) => {
                if args.verbose { debug!("RAW: {}", msg); }
                
                let v: Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(e) => { warn!("JSON parse error: {}", e); continue; }
                };
                
                let msg_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if msg_type != "TICK" { continue; }
                
                let data = &v["data"];
                
                let symbol = data.get("symbol").and_then(|x| x.as_str()).unwrap_or("");
                let resolved_incoming = resolve_symbol_alias(symbol);
                if resolved_incoming.to_uppercase() != resolved_symbol.to_uppercase() { continue; }
                
                let price = data.get("last").and_then(|x| x.as_f64())
                    .or_else(|| {
                        let bid = data.get("bid").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        let ask = data.get("ask").and_then(|x| x.as_f64()).unwrap_or(0.0);
                        if bid > 0.0 && ask > 0.0 { Some((bid + ask) / 2.0) } else { None }
                    })
                    .unwrap_or(0.0);
                
                let bid = data.get("bid").and_then(|x| x.as_f64()).unwrap_or(price);
                let ask = data.get("ask").and_then(|x| x.as_f64()).unwrap_or(price);
                let volume = data.get("volume").and_then(|x| x.as_i64()).unwrap_or(0);
                
                let time_str = data.get("server_time").and_then(|x| x.as_str())
                    .or_else(|| data.get("time").and_then(|x| x.as_str()))
                    .unwrap_or("");
                
                let dt = match parse_iso_datetime(time_str) {
                    Some(d) => d,
                    None => Utc::now(),
                };
                let ts = dt.timestamp();
                
                // Update price history
                price_history.push_back(price);
                high_history.push_back(ask);
                low_history.push_back(bid);
                
                const MAX_HISTORY: usize = 100;
                while price_history.len() > MAX_HISTORY { price_history.pop_front(); }
                while high_history.len() > MAX_HISTORY { high_history.pop_front(); }
                while low_history.len() > MAX_HISTORY { low_history.pop_front(); }
                
                state.push_price(price);
                state.push_ohlc(ask, bid);
                
                // Candle builder update
                candle_builder.update(price, volume, ts);
                
                // Check for completed candle
                let minute_now = ts / 60;
                let minute_last = if let Some(ref c) = last_completed_candle { c.time / 60 } else { -1 };
                
                if minute_now > minute_last && candle_builder.is_complete() {
                    if let Some(candle) = candle_builder.build(ts) {
                        state.push_candle(candle);
                        last_completed_candle = Some(candle);
                        info!("CANDLE: {} O:{:.2} H:{:.2} L:{:.2} C:{:.2} V:{}",
                            candle.time, candle.open, candle.high, candle.low, candle.close, candle.volume);
                    }
                }
                
                let current_candle = candle_builder.build(ts).unwrap_or(Candle {
                    time: ts, open: price, high: price, low: price, close: price, volume,
                });
                
                // Calculate indicators
                let prices_vec: Vec<f64> = price_history.iter().copied().collect();
                let closes_vec: Vec<f64> = price_history.iter().copied().collect();
                let highs_vec: Vec<f64> = high_history.iter().copied().collect();
                let lows_vec: Vec<f64> = low_history.iter().copied().collect();
                
                state.ema_fast = calc_ema(&prices_vec, config.ema_fast);
                state.ema_slow = calc_ema(&prices_vec, config.ema_slow);
                state.rsi_prev = state.rsi;
                state.rsi = calc_rsi(&closes_vec, config.rsi_period);
                state.atr = calc_atr(&highs_vec, &lows_vec, &closes_vec, config.atr_period);
                
                                if price_history.len() < 60 {
                    info!("🔄 WARMUP: {}/{} ticks collected (need {} for indicators)", 
                        price_history.len(), 60, 60);
                    continue;
                } else if price_history.len() == 60 {
                    info!("✅ WARMUP COMPLETE: {} ticks collected, indicators ready", price_history.len());
                }
                
                                // RUN STRATEGY
                let signal = should_trade(&mut state, price, bid, ask, &current_candle, &config);
                
                // Always log signal status
                let ema_str = match (state.ema_fast, state.ema_slow) {
                    (Some(f), Some(s)) => format!("EMA20={:.2} EMA50={:.2}", f, s),
                    _ => "EMA=---".to_string(),
                };
                let rsi_str = state.rsi.map(|r| format!("RSI={:.1}", r)).unwrap_or_else(|| "RSI=---".to_string());
                let atr_str = state.atr.map(|a| format!("ATR={:.3}", a)).unwrap_or_else(|| "ATR=---".to_string());
                
                                match signal.action {
                                        SignalAction::EnterLong | SignalAction::EnterShort => {
                        info!("🚨🚨🚨 SIGNAL {} | score={}/10 conf={:.2} | {} | {} | {} | SL={:.2} TP={:.2}",
                            match signal.direction {
                                Direction::Long => "BUY ",
                                Direction::Short => "SELL",
                                Direction::None => "HOLD",
                            },
                            signal.score,
                            signal.confidence,
                            ema_str,
                            rsi_str,
                            atr_str,
                            signal.stop_loss,
                            signal.take_profit
                        );
                        
                        // Send Slack notification for signal
                        let dir_str = match signal.direction {
                            Direction::Long => "BUY",
                            Direction::Short => "SELL",
                            Direction::None => "HOLD",
                        };
                        if let Err(e) = slack.send_signal(
                            dir_str,
                            signal.entry_price,
                            signal.stop_loss,
                            signal.take_profit,
                            signal.score,
                            signal.confidence,
                            &signal.reason
                        ) {
                            warn!("⚠️ Slack signal notification failed: {}", e);
                        }
                        info!("   Reason: {}", signal.reason);
                        
                                                // Detailed score breakdown with visual bar
                        let bd = &signal.breakdown;
                        
                        info!("   📊 SCORE BREAKDOWN ({} pts):", bd.total);
                        info!("   ├ Trend      [{}] {} pts", make_bar(bd.trend as usize, 2), bd.trend);
                        info!("   ├ Strength   [{}] {} pts", make_bar(bd.strength as usize, 2), bd.strength);
                        info!("   ├ Structure  [{}] {} pts", make_bar(bd.structure.unsigned_abs() as usize, 2), bd.structure);
                        info!("   ├ Pullback   [{}] {} pts", make_bar(bd.pullback as usize, 1), bd.pullback);
                        info!("   ├ RSI        [{}] {} pts", make_bar(bd.rsi as usize, 1), bd.rsi);
                        info!("   ├ Volatility [{}] {} pts", make_bar(bd.volatility as usize, 1), bd.volatility);
                        info!("   └ Confirm   [{}] {} pts", make_bar(bd.confirmation as usize, 1), bd.confirmation);
                        
                        // Visual score bar (total out of 10)
                        let score_bar = make_bar(signal.score as usize, 10);
                        info!("   📈 SCORE BAR [{}] {}/10 (min: {})", score_bar, signal.score, config.min_score);
                        
                        // Show required remaining points
                        let remaining = config.min_score - bd.total;
                        if remaining > 0 {
                            info!("   💡 Need {} more pts to enter next time", remaining);
                        } else {
                            info!("   ✅ Score {} >= min {} - CONFIRMED", bd.total, config.min_score);
                        }
                    }
                                        SignalAction::SkipDueToFilter => { 
                        if args.verbose { 
                            info!("⬜ SKIP | {}", signal.reason); 
                        }
                    }
                    SignalAction::Hold => { 
                        // Log status every 30 seconds to show engine is alive
                        let now = Utc::now();
                        if last_status_time.map_or(true, |t| (now - t).num_seconds() >= 30) {
                            let trend_status = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                                if state.ema_fast.unwrap() > state.ema_slow.unwrap() { "📈 Uptrend" } 
                                else { "📉 Downtrend" }
                            } else { "⚪ Flat" };
                            
                            let sideway_check = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                                let diff = (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs();
                                if diff < 0.30 { "SIDEWAY" } else if diff < 0.20 { "WEAK" } else { "STRONG" }
                            } else { "N/A" };
                            
                            info!("📊 STATUS | Price={:.2} | {} | {} | {} | {} | Ticks={}",
                                price, trend_status, ema_str, rsi_str, atr_str, state.ticks_processed);
                            info!("   Trend: {} | Cooldown: {} | Losses: {} | Positions: Long={} Short={}", 
                                sideway_check, state.cooldown_counter, state.consecutive_losses,
                                state.long_positions, state.short_positions);
                            
                                                        // Show score breakdown (always visible)
                            info!("   ┌────────────────────────── SCORE BREAKDOWN ──────────────────────────┐");
                                
                                // Calculate potential score components
                                let ema_diff = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                                    (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs()
                                } else { 0.0 };
                                
                                let trend_pts = if ema_diff >= 0.20 { 2 } else if ema_diff >= 0.10 { 1 } else { 0 };
                                let strength_pts = if ema_diff >= 0.40 { 2 } else if ema_diff >= 0.20 { 1 } else { 0 };
                                
                                // Structure detection
                                let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                                let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                                let structure = detect_structure(&highs_vec, &lows_vec, 20);
                                let struct_pts = match structure {
                                    SwingType::HigherHigh => 2,
                                    SwingType::HigherLow => 1,
                                    SwingType::LowerLow => 2,
                                    SwingType::LowerHigh => 1,
                                    _ => 0,
                                };
                                
                                let pullback_ok = state.ema_fast.map_or(false, |ema| {
                                    is_pullback(price, ema, 15.0, 0.01)
                                });
                                let pullback_pts = if pullback_ok { 1 } else { 0 };
                                
                                let rsi_val = state.rsi.unwrap_or(50.0);
                                let rsi_pts = if (rsi_val > 50.0 && rsi_val < 70.0) || (rsi_val >= 30.0 && rsi_val < 50.0) { 1 } else { 0 };
                                
                                // Sideway check
                                let sideway_ok = state.ema_fast.is_some() && state.ema_slow.is_some() && {
                                    let diff = (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs();
                                    diff >= 0.30
                                };
                                
                                info!("   │ Trend    [{}] {} pts  (need >=2 for valid)", make_bar(trend_pts, 2), trend_pts);
                                info!("   │ Strength [{}] {} pts  (EMA dist: {:.3})", make_bar(strength_pts, 2), strength_pts, ema_diff);
                                info!("   │ Struct   [{}] {} pts  ({:?})", make_bar(struct_pts, 2), struct_pts, structure);
                                info!("   │ Pullback [{}] {} pts  ({})", make_bar(pullback_pts, 1), pullback_pts, if pullback_ok { "OK" } else { "NO" });
                                info!("   │ RSI      [{}] {} pts  ({:.1})", make_bar(rsi_pts, 1), rsi_pts, rsi_val);
                                info!("   │ Vol/Conf [██] 2 pts  (assumed OK)");
                                info!("   └────────────────────────────────────────────────────────────────────────────┘");
                                
                                                                let potential_total = (trend_pts + strength_pts + struct_pts + pullback_pts + rsi_pts + 2) as i32;
                                let score_bar = make_bar(potential_total.max(0).min(10) as usize, 10);
                                info!("   📈 POTENTIAL SCORE: [{}] {}/10 | Min Required: {}", score_bar, potential_total, config.min_score);
                                
                                if potential_total < config.min_score {
                                    let missing = config.min_score - potential_total;
                                    info!("   ❌ Missing {} pts - Waiting for better setup...", missing);
                                } else {
                                    info!("   ✅ Would qualify - Watching for entry confirmation...");
                                }
                                
                                                                                                                                        if !sideway_ok {
                                                                                                                                            info!("   ⚠️ WARNING: Market is SIDEWAY or weak trend - filters active");
                                                                                                                                        }
                            
                                                                last_status_time = Some(now);
                        }
                    }
                }
                
                // Slack heartbeat (every 5 minutes, independent of status log)
                if slack.is_enabled() {
                    let now = Utc::now();
                    if last_heartbeat_time.map_or(true, |t| (now - t).num_seconds() >= 300) {
                        let trend_status = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                            if state.ema_fast.unwrap() > state.ema_slow.unwrap() { "Uptrend" } 
                            else { "Downtrend" }
                        } else { "Flat" };
                        let ema_diff = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                            (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs()
                        } else { 0.0 };
                        let trend_pts = if ema_diff >= 0.20 { 2 } else if ema_diff >= 0.10 { 1 } else { 0 };
                        let strength_pts = if ema_diff >= 0.40 { 2 } else if ema_diff >= 0.20 { 1 } else { 0 };
                        let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                        let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                        let structure = detect_structure(&highs_vec, &lows_vec, 20);
                        let struct_pts = match structure {
                            SwingType::HigherHigh => 2, SwingType::HigherLow => 1,
                            SwingType::LowerLow => 2, SwingType::LowerHigh => 1, _ => 0,
                        };
                        let pullback_ok = state.ema_fast.map_or(false, |ema| is_pullback(price, ema, 15.0, 0.01));
                        let pullback_pts = if pullback_ok { 1 } else { 0 };
                        let rsi_val = state.rsi.unwrap_or(50.0);
                        let rsi_pts = if (rsi_val > 50.0 && rsi_val < 70.0) || (rsi_val >= 30.0 && rsi_val < 50.0) { 1 } else { 0 };
                        let potential_total = trend_pts + strength_pts + struct_pts + pullback_pts + rsi_pts + 2;
                        
                        let status_msg = format!(
                            "Price={:.2} | {} | Score={}/10 | Cooldown={} | Long={} Short={} | Ticks={}",
                            price, trend_status, potential_total, state.cooldown_counter,
                            state.long_positions, state.short_positions, state.ticks_processed
                        );
                        if let Err(e) = slack.send_status(&status_msg) {
                            warn!("⚠️ Slack heartbeat failed: {}", e);
                        }
                        last_heartbeat_time = Some(now);
                    }
                }
                
                // EXECUTE TRADE
                if signal.is_enter() {
                    let now = Utc::now();
                    
                    let can_send = match last_action_time {
                        Some(t) => (now - t).num_seconds() >= args.cooldown_sec as i64,
                        None => true,
                    };
                    
                    if !can_send {
                        debug!("⏳ Cooldown active, skipping signal");
                        continue;
                    }
                    
                                        if args.trade {
                        if let Some(ref sock) = dealer {
                                                        // Volume check
                            let current_positions = if signal.direction == Direction::Long { 
                                state.long_positions 
                            } else { 
                                state.short_positions 
                            };
                            let current_volume = (current_positions as f64) * args.volume;
                            
                            if current_volume >= args.max_volume_per_trade {
                                info!("⏳ VOLUME LIMIT: Already using {:.2}/{} lots for this direction",
                                    current_volume, args.max_volume_per_trade);
                                continue;
                            }
                            
                            let total_volume = ((state.long_positions + state.short_positions) as f64) * args.volume;
                            if total_volume >= args.max_total_volume {
                                info!("⏳ TOTAL VOLUME LIMIT: Using {:.2}/{} lots max",
                                    total_volume, args.max_total_volume);
                                continue;
                            }
                            
                            let order_type = match signal.direction {
                                Direction::Long => "BUY",
                                Direction::Short => "SELL",
                                Direction::None => continue,
                            };
                            
                            let request_id = Uuid::new_v4().to_string();
                            
                            // MT5 comment limit is 27 chars max - truncate to fit
                                                        let comment = format!("v2:{}+{:.0}", signal.score, signal.confidence * 100.0);
                                                        let comment = if comment.len() > 27 { &comment[..27] } else { &comment };
                            
                                                        let payload = serde_json::json!({
                                                            "type": "ORDER_SEND",
                                                            "data": {
                                                                "symbol": resolved_symbol,
                                                                "volume": args.volume,
                                                                "order_type": order_type,
                                                                "price": 0,
                                                                "stop_loss": signal.stop_loss,
                                                                "take_profit": signal.take_profit,
                                                                "comment": comment,
                                                                "magic": 2100,
                                                                "request_id": request_id
                                                            }
                                                        });
                            
                            let s = payload.to_string();
                            info!("📤 EXECUTING {} {} lots @ {} | SL={:.2} TP={:.2}",
                                order_type, args.volume, signal.entry_price, signal.stop_loss, signal.take_profit);
                            
                                                        match sock.send(s.as_bytes(), 0) {
                                Ok(_) => match sock.recv_string(0) {
                                    Ok(Ok(resp)) => {
                                        info!("📥 Order response: {}", resp);
                                        state.record_trade(signal.entry_price, true);
                                        if signal.direction == Direction::Long { state.long_positions += 1; }
                                        else if signal.direction == Direction::Short { state.short_positions += 1; }
                                        
                                        // Send Slack notification for executed order
                                        if let Err(e) = slack.send_order_executed(
                                            order_type,
                                            args.volume,
                                            signal.entry_price,
                                            &request_id
                                        ) {
                                            warn!("⚠️ Slack order notification failed: {}", e);
                                        }
                                    }
                                    Ok(Err(_)) => { warn!("⚠️ Non-UTF8 reply"); }
                                    Err(e) => { warn!("⚠️ No reply: {:?}", e); }
                                },
                                Err(e) => { error!("❌ Send failed: {:?}", e); }
                            }
                        }
                    } else {
                        info!("📋 SIGNAL {} (trade disabled) | entry={:.2} SL={:.2} TP={:.2} | score={} conf={:.2}",
                            match signal.direction {
                                Direction::Long => "BUY",
                                Direction::Short => "SELL",
                                Direction::None => "HOLD",
                            },
                            signal.entry_price, signal.stop_loss, signal.take_profit, signal.score, signal.confidence);
                    }
                    
                    last_action_time = Some(now);
                }
            }
            
                        Ok(Err(e)) => { warn!("recv_string error: {:?}", e); thread::sleep(Duration::from_millis(50)); }
            Err(e) => { warn!("ZMQ error: {:?}", e); thread::sleep(Duration::from_millis(200)); }
        }
        
        // Check for position close notifications from order_monitor.py
        if let Some(ref notify_sock) = notify_socket {
            // Try non-blocking receive
            match notify_sock.recv_string(zmq::DONTWAIT) {
                Ok(Ok(msg)) => {
                    // Message format: "CLOSE_NOTIFY|{ticket}|{direction}|{volume}|{price}|{profit}|{magic}"
                    let parts: Vec<&str> = msg.split('|').collect();
                    if parts.len() >= 7 && parts[0] == "CLOSE_NOTIFY" {
                        let ticket = parts[1];
                        let direction = parts[2];
                        let volume: f64 = parts[3].parse().unwrap_or(0.0);
                        let price: f64 = parts[4].parse().unwrap_or(0.0);
                        let profit: f64 = parts[5].parse().unwrap_or(0.0);
                        let magic: i32 = parts[6].parse().unwrap_or(0);
                        
                        info!("Position CLOSED: #{ticket} {direction} {volume} lots @ {price} | P&L: ${profit:+.2} | Magic: {magic}");
                        
                        // Send Slack notification
                        if let Err(e) = slack.send_position_closed(ticket, direction, volume, price, profit, magic) {
                            warn!("Slack close notification failed: {}", e);
                        }
                    }
                }
                _ => { /* No message or error - normal */ }
            }
        }
    }
}
