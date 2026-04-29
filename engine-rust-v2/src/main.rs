// ============================================================
// ADVANCED SCALPING ENGINE v2.0 - Production-Ready System
// ============================================================

use clap::Parser;
use chrono::{DateTime, NaiveDateTime, Utc, Local};
use std::collections::{VecDeque, HashMap};
use std::time::Duration;
use std::thread;
use std::fs;
use uuid::Uuid;
use log::{info, warn, debug, error};
use env_logger::Env;

mod strategy_new;
mod slack;
mod optimizer;
mod log_writer;


use slack::SlackClient;
use strategy_new::{
    Config, State, Candle, SignalAction, Direction,
    should_trade, calc_ema, calc_rsi, calc_atr,
    is_pullback, detect_structure,
    SwingType,
};
use optimizer::{OptimizationResult, optimize, save_optimization_result, load_trade_logs, load_optimization_result};

use log_writer::{make_strategy_row, append_strategy_log};


use serde_json::Value;
use std::time::Instant;

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
        #[arg(long, default_value_t = 1)]
        min_score: i32,
    
    /// Minimum confidence (0.0-1.0)
        #[arg(long, default_value_t = 0.30)]
        min_confidence: f64,
    
    /// Sideway EMA threshold
    #[arg(long, default_value_t = 0.30)]
    sideway_threshold: f64,
    
    /// Min trend strength (EMA distance)
        #[arg(long, default_value_t = 0.02)]
        min_trend_strength: f64,
    
    /// Max pullback distance from EMA (pips)
        #[arg(long, default_value_t = 60.0)]
        max_pullback_pips: f64,
    
    /// Max FOMO distance (pips)
        #[arg(long, default_value_t = 80.0)]
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
        #[arg(long, default_value_t = 3.0)]
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

        /// Enable auto optimization from log file at startup
    #[arg(long, default_value_t = false)]
    auto_optimize: bool,

    /// Path to strategy log JSON file
    #[arg(long, default_value = "strategy_logs.json")]
    strategy_log_file: String,

    /// Path to save optimization result JSON
        #[arg(long, default_value = "optimizer_result.json")]
        optimizer_output_file: String,

        /// Path to append live strategy JSON logs
        #[arg(long, default_value = "strategy_logs.json")]
        live_log_file: String,

        /// Auto reload optimized config during runtime
        #[arg(long, default_value_t = true)]
        auto_reload_optimized_config: bool,

                /// Seconds between optimizer reload checks
                #[arg(long, default_value_t = 60)]
                optimizer_reload_sec: u64,

                /// Seconds between status messages sent to Slack (recommended 10-15s)
                #[arg(long, default_value_t = 15)]
                status_interval_sec: u64,

        /// Force apply a loose starter config at launch (useful for demo/testing)
                #[arg(long, default_value_t = false)]
                loose_start: bool,

                /// Enable scalping preset: apply aggressive scalping-friendly parameters
                #[arg(long, default_value_t = false)]
                scalp_mode: bool,

                /// Log per-tick analysis (debug-level) each tick
        #[arg(long, default_value_t = false)]
        per_tick_log: bool,

                /// Require one-candle entry confirmation (true = wait one candle to confirm entry)
        #[arg(long, default_value_t = false)]
        require_confirmation: bool,



        /// Path to load historical M1 candles (JSON array) - optional
        #[arg(long, default_value = "mt5_history.json")]
        history_file: String,

        /// Number of historical candles to load (most recent N)
        #[arg(long, default_value_t = 500)]
        history_count: usize,

        /// If true, require history to be loaded at startup (exit if not loaded)
        #[arg(long, default_value_t = false)]
        require_history: bool,

        /// Seconds to wait for history load when using MT5 bridge
        #[arg(long, default_value_t = 30)]
        history_wait_sec: u64,

            /// Use local Python MT5 bridge to fetch history at startup (requires python MetaTrader5 package)
        #[arg(long, default_value_t = false)]
        use_mt5_bridge: bool,

        /// Path to the python bridge script to call (default: python_bridge/mt5_bridge.py)
        #[arg(long, default_value = "python_bridge/mt5_bridge.py")]
        mt5_bridge_script: String,

        /// Symbol name to request from MT5 bridge (if use_mt5_bridge=true)
        #[arg(long, default_value = "GOLD")]
        mt5_symbol: String,
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

                let mut config = Config {
        ema_fast: 20,
        ema_slow: 50,
        rsi_period: 14,
        atr_period: 14,
        sideway_ema_threshold: args.sideway_threshold,
        min_trend_strength: args.min_trend_strength,
        max_pullback_pips: args.max_pullback_pips,
        max_fomo_pips: args.max_fomo_pips,
        rsi_oversold: 15.0,
        rsi_overbought: 85.0,
        rsi_sell_confirm_low: 50.0,
        rsi_sell_confirm_high: 60.0,
        rsi_buy_confirm_low: 40.0,
        rsi_buy_confirm_high: 50.0,
                max_candle_mult: args.max_candle_mult,
        max_wick_ratio: 2.0,
        min_score: args.min_score,
        min_confidence: args.min_confidence,
        sl_mult: args.sl_mult,
        tp_mult: args.tp_mult,
        pip_value: 0.01,
        cooldown_after_loss: args.cooldown_candles,
        max_consecutive_losses: args.max_losses,
        pause_duration_minutes: args.pause_minutes,
        max_positions_per_direction: 10,
        no_trade_zone_pips: if args.scalp_mode { 0.0 } else { 100.0 },
        require_confirmation: args.require_confirmation,
        momentum_override_enabled: true,
        momentum_override_mult: 0.6,
        scalp_mode: args.scalp_mode,
        };

    // Apply 'scalp mode' preset if requested (overrides many conservative defaults)
    if args.scalp_mode {
        config.min_score = 1;
        config.min_confidence = 0.30;
        config.min_trend_strength = 0.02;
        config.max_pullback_pips = 60.0;
        config.max_fomo_pips = 80.0;
        config.max_candle_mult = 3.0;
        config.require_confirmation = false;
        config.momentum_override_mult = 0.6;
        info!("SCALP MODE ENABLED: applied scalping-friendly presets");
    }

        // Initialize Slack client early so optimizer can send updates
        let slack = SlackClient::new(args.slack_enabled, args.slack_webhook.clone(), args.slack_channel.clone());

        // Send immediate startup status to Slack
        if slack.is_enabled() {
            let startup_msg = format!("Engine STARTED | Symbol={} | Trading={} | LooseStart={} | ScalpMode={} | LogLevel={}",
                            resolved_symbol,
                            if args.trade { "ENABLED" } else { "DISABLED" },
                            args.loose_start,
                            args.scalp_mode,
                            args.log_level);
            match slack.send_status(&startup_msg) {
                Ok(_) => info!("Startup notification sent to Slack"),
                Err(e) => warn!("Failed to send startup Slack notification: {}", e),
            }
        }


    // Ensure strategy log files exist so optimizer and logger can append safely
    if !std::path::Path::new(&args.strategy_log_file).exists() {
        match fs::write(&args.strategy_log_file, "[]") {
            Ok(_) => info!("Created empty strategy log file: {}", args.strategy_log_file),
            Err(e) => warn!("Failed to create strategy log file {}: {}", args.strategy_log_file, e),
        }
    }
    if !std::path::Path::new(&args.live_log_file).exists() {
        match fs::write(&args.live_log_file, "[]") {
            Ok(_) => info!("Created empty live log file: {}", args.live_log_file),
            Err(e) => warn!("Failed to create live log file {}: {}", args.live_log_file, e),
        }
    }

                if args.auto_optimize {
        let logs = load_trade_logs(&args.strategy_log_file);
        let base_cfg = Config {
                    ema_fast: 20,
                    ema_slow: 50,
                    rsi_period: 14,
                    atr_period: 14,
                    sideway_ema_threshold: args.sideway_threshold,
                    min_trend_strength: args.min_trend_strength,
                    max_pullback_pips: args.max_pullback_pips,
                    max_fomo_pips: args.max_fomo_pips,
                    rsi_oversold: 15.0,
                    rsi_overbought: 85.0,
                    rsi_sell_confirm_low: 50.0,
                    rsi_sell_confirm_high: 60.0,
                    rsi_buy_confirm_low: 40.0,
                    rsi_buy_confirm_high: 50.0,
                    max_candle_mult: args.max_candle_mult,
                    max_wick_ratio: 2.0,
                    min_score: args.min_score,
                    min_confidence: args.min_confidence,
                    sl_mult: args.sl_mult,
                    tp_mult: args.tp_mult,
                    pip_value: 0.01,
                    cooldown_after_loss: args.cooldown_candles,
                    max_consecutive_losses: args.max_losses,
                    pause_duration_minutes: args.pause_minutes,
                    max_positions_per_direction: 10,
                    no_trade_zone_pips: if args.scalp_mode { 0.0 } else { 100.0 },
                    require_confirmation: args.require_confirmation,
        momentum_override_enabled: true,
        momentum_override_mult: 0.6,
        scalp_mode: args.scalp_mode,
                };

        // If user requested a loose start, apply loose config regardless of logs
        if args.loose_start {
                    let mut loose = base_cfg.clone();
        loose.min_score = 0; // very low required score for demo (loose start)
        loose.min_confidence = 0.10; // very low confidence
        loose.max_pullback_pips = (loose.max_pullback_pips + 25.0).min(120.0);
        loose.max_fomo_pips = (loose.max_fomo_pips + 40.0).min(200.0);
        loose.max_candle_mult = (loose.max_candle_mult + 2.0).min(6.0);


        let mut metrics = HashMap::new();
                metrics.insert("note".to_string(), 1.0);
                // Build a minimal BacktestResult placeholder
                let empty_metrics = optimizer::BacktestResult { total_pnl: 0.0, total_trades: 0, winrate: 0.0, expectancy: 0.0, max_drawdown: 0.0, sharpe_ratio: 0.0 };
                let result = OptimizationResult { best_config: loose.clone(), train_metrics: empty_metrics.clone(), test_metrics: empty_metrics.clone() };

                if let Err(e) = save_optimization_result(&args.optimizer_output_file, &result) {
                    warn!("Failed to save loose starter optimizer result: {}", e);
                } else {
                    info!("Loose starter optimizer config saved to {}", args.optimizer_output_file);
                }

                // Apply loose config
                config.min_score = loose.min_score;
                config.min_confidence = loose.min_confidence;
                config.sideway_ema_threshold = loose.sideway_ema_threshold;
                config.min_trend_strength = loose.min_trend_strength;
                config.max_pullback_pips = loose.max_pullback_pips;
                config.max_fomo_pips = loose.max_fomo_pips;
                config.max_candle_mult = loose.max_candle_mult;
                config.sl_mult = loose.sl_mult;
                config.tp_mult = loose.tp_mult;
        // Reduce confirmation requirement for loose start to increase trade frequency
        config.require_confirmation = false;

                    info!("LOOSE STARTER FORCED: min_score={} min_conf={:.2} pullback={} fomo={} candle_mult={:.2}",
            config.min_score, config.min_confidence, config.max_pullback_pips, config.max_fomo_pips, config.max_candle_mult);

                // Notify Slack about applied loose optimizer (if enabled)
        if slack.is_enabled() {
            let summary = format!(
                "Loose starter applied at startup: min_score={} min_conf={:.2} pullback={} fomo={} candle_mult={:.2}",
                config.min_score, config.min_confidence, config.max_pullback_pips, config.max_fomo_pips, config.max_candle_mult
            );
            // Build list of changed params (compare base_cfg -> loose)
            let mut changes: Vec<(String,String,String)> = Vec::new();
            if base_cfg.min_score != loose.min_score { changes.push(("min_score".to_string(), base_cfg.min_score.to_string(), loose.min_score.to_string())); }
            if (base_cfg.min_confidence - loose.min_confidence).abs() > std::f64::EPSILON { changes.push(("min_confidence".to_string(), format!("{:.2}", base_cfg.min_confidence), format!("{:.2}", loose.min_confidence))); }
            if (base_cfg.max_pullback_pips - loose.max_pullback_pips).abs() > std::f64::EPSILON { changes.push(("max_pullback_pips".to_string(), format!("{:.1}", base_cfg.max_pullback_pips), format!("{:.1}", loose.max_pullback_pips))); }
            if (base_cfg.max_fomo_pips - loose.max_fomo_pips).abs() > std::f64::EPSILON { changes.push(("max_fomo_pips".to_string(), format!("{:.1}", base_cfg.max_fomo_pips), format!("{:.1}", loose.max_fomo_pips))); }
            if (base_cfg.max_candle_mult - loose.max_candle_mult).abs() > std::f64::EPSILON { changes.push(("max_candle_mult".to_string(), format!("{:.2}", base_cfg.max_candle_mult), format!("{:.2}", loose.max_candle_mult))); }
            if (base_cfg.sl_mult - loose.sl_mult).abs() > std::f64::EPSILON { changes.push(("sl_mult".to_string(), format!("{:.2}", base_cfg.sl_mult), format!("{:.2}", loose.sl_mult))); }
            if (base_cfg.tp_mult - loose.tp_mult).abs() > std::f64::EPSILON { changes.push(("tp_mult".to_string(), format!("{:.2}", base_cfg.tp_mult), format!("{:.2}", loose.tp_mult))); }
            if base_cfg.require_confirmation != loose.require_confirmation { changes.push(("require_confirmation".to_string(), format!("{}", base_cfg.require_confirmation), format!("{}", loose.require_confirmation))); }

            if let Err(e) = slack.send_optimizer_update("Loose Starter Applied", &summary, Some(changes)) {
                warn!("Failed to send loose optimizer update to Slack: {}", e);
            } else {
                info!("Loose optimizer update sent to Slack");
            }
        }

        }


        if logs.is_empty() && !args.loose_start {
        // No historical logs: apply a loose starter config to create more trades initially
        let mut loose = base_cfg.clone();
        loose.min_score = 3;
        loose.min_confidence = 0.30;
        loose.max_pullback_pips = (loose.max_pullback_pips + 10.0).min(60.0);
        loose.max_fomo_pips = (loose.max_fomo_pips + 15.0).min(80.0);
        loose.max_candle_mult = (loose.max_candle_mult + 0.5).min(3.0);

        let mut metrics = HashMap::new();
                metrics.insert("note".to_string(), 1.0);
                let empty_metrics = optimizer::BacktestResult { total_pnl: 0.0, total_trades: 0, winrate: 0.0, expectancy: 0.0, max_drawdown: 0.0, sharpe_ratio: 0.0 };
                let result = OptimizationResult { best_config: loose.clone(), train_metrics: empty_metrics.clone(), test_metrics: empty_metrics.clone() };

                if let Err(e) = save_optimization_result(&args.optimizer_output_file, &result) {
                    warn!("Failed to save starter optimizer result: {}", e);
                } else {
                    info!("Starter loose optimizer config saved to {}", args.optimizer_output_file);
                }

                // Apply loose config
                config.min_score = loose.min_score;
                config.min_confidence = loose.min_confidence;
                config.sideway_ema_threshold = loose.sideway_ema_threshold;
                config.min_trend_strength = loose.min_trend_strength;
                config.max_pullback_pips = loose.max_pullback_pips;
                config.max_fomo_pips = loose.max_fomo_pips;
                config.max_candle_mult = loose.max_candle_mult;
                config.sl_mult = loose.sl_mult;
                config.tp_mult = loose.tp_mult;
        // When applying starter loose due to no logs, also relax confirmation to increase initial trades
        config.require_confirmation = false;

                    info!("LOOSE STARTER APPLIED: min_score={} min_conf={:.2} pullback={} fomo={} candle_mult={:.2}",
            config.min_score, config.min_confidence, config.max_pullback_pips, config.max_fomo_pips, config.max_candle_mult);

                // Notify Slack about applied starter optimizer
        if slack.is_enabled() {
            let summary = format!(
                "Starter loose config applied: min_score={} min_conf={:.2} pullback={} fomo={} candle_mult={:.2}",
                config.min_score, config.min_confidence, config.max_pullback_pips, config.max_fomo_pips, config.max_candle_mult
            );
            // Build list of changed params (compare base_cfg -> loose)
            let mut changes: Vec<(String,String,String)> = Vec::new();
            if base_cfg.min_score != loose.min_score { changes.push(("min_score".to_string(), base_cfg.min_score.to_string(), loose.min_score.to_string())); }
            if (base_cfg.min_confidence - loose.min_confidence).abs() > std::f64::EPSILON { changes.push(("min_confidence".to_string(), format!("{:.2}", base_cfg.min_confidence), format!("{:.2}", loose.min_confidence))); }
            if (base_cfg.max_pullback_pips - loose.max_pullback_pips).abs() > std::f64::EPSILON { changes.push(("max_pullback_pips".to_string(), format!("{:.1}", base_cfg.max_pullback_pips), format!("{:.1}", loose.max_pullback_pips))); }
            if (base_cfg.max_fomo_pips - loose.max_fomo_pips).abs() > std::f64::EPSILON { changes.push(("max_fomo_pips".to_string(), format!("{:.1}", base_cfg.max_fomo_pips), format!("{:.1}", loose.max_fomo_pips))); }
            if (base_cfg.max_candle_mult - loose.max_candle_mult).abs() > std::f64::EPSILON { changes.push(("max_candle_mult".to_string(), format!("{:.2}", base_cfg.max_candle_mult), format!("{:.2}", loose.max_candle_mult))); }
            if (base_cfg.sl_mult - loose.sl_mult).abs() > std::f64::EPSILON { changes.push(("sl_mult".to_string(), format!("{:.2}", base_cfg.sl_mult), format!("{:.2}", loose.sl_mult))); }
            if (base_cfg.tp_mult - loose.tp_mult).abs() > std::f64::EPSILON { changes.push(("tp_mult".to_string(), format!("{:.2}", base_cfg.tp_mult), format!("{:.2}", loose.tp_mult))); }
            if base_cfg.require_confirmation != loose.require_confirmation { changes.push(("require_confirmation".to_string(), format!("{}", base_cfg.require_confirmation), format!("{}", loose.require_confirmation))); }

            if let Err(e) = slack.send_optimizer_update("Starter Loose Applied", &summary, Some(changes)) {
                warn!("Failed to send starter optimizer update to Slack: {}", e);
            } else {
                info!("Starter optimizer update sent to Slack");
            }
        }

        } else if !args.loose_start {

                        // Load historical candles for optimizer
            let history_candles: Vec<Candle> = if !args.history_file.is_empty() && std::path::Path::new(&args.history_file).exists() {
                match std::fs::read_to_string(&args.history_file) {
                    Ok(s) => serde_json::from_str::<Vec<Candle>>(&s).unwrap_or_default(),
                    Err(_) => Vec::new(),
                }
            } else { Vec::new() };

            if history_candles.is_empty() {
                warn!("Optimizer: no historical candles found at {} - skipping optimization", args.history_file);
            } else {
                let result = optimize(&history_candles, config.clone());
                if let Err(e) = save_optimization_result(&args.optimizer_output_file, &result) {
                    warn!("Failed to save optimization result: {}", e);
                } else {
                    info!("Auto-optimization completed. Result saved to {}", args.optimizer_output_file);
                }

                // Log train/test metrics
                info!("OPTIMIZER TRAIN METRICS: total_pnl={:.3} trades={} winrate={:.2} expectancy={:.3} max_dd={:.3} sharpe={:.3}",
                    result.train_metrics.total_pnl,
                    result.train_metrics.total_trades,
                    result.train_metrics.winrate,
                    result.train_metrics.expectancy,
                    result.train_metrics.max_drawdown,
                    result.train_metrics.sharpe_ratio
                );

                info!("OPTIMIZER TEST METRICS: total_pnl={:.3} trades={} winrate={:.2} expectancy={:.3} max_dd={:.3} sharpe={:.3}",
                    result.test_metrics.total_pnl,
                    result.test_metrics.total_trades,
                    result.test_metrics.winrate,
                    result.test_metrics.expectancy,
                    result.test_metrics.max_drawdown,
                    result.test_metrics.sharpe_ratio
                );

                                // Apply best config
                let old_cfg = config.clone();
                config.min_score = result.best_config.min_score;
                config.min_confidence = result.best_config.min_confidence;
                config.sideway_ema_threshold = result.best_config.sideway_ema_threshold;
                config.min_trend_strength = result.best_config.min_trend_strength;
                config.max_pullback_pips = result.best_config.max_pullback_pips;
                config.max_fomo_pips = result.best_config.max_fomo_pips;
                config.max_candle_mult = result.best_config.max_candle_mult;
                config.sl_mult = result.best_config.sl_mult;
                config.tp_mult = result.best_config.tp_mult;
                config.require_confirmation = result.best_config.require_confirmation;

                info!("OPTIMIZER APPLIED: min_score={} min_conf={:.2} sideway={:.3} trend={:.3} pullback={:.1} fomo={:.1} candle_mult={:.2} sl={:.2} tp={:.2}",
                    config.min_score,
                    config.min_confidence,
                    config.sideway_ema_threshold,
                    config.min_trend_strength,
                    config.max_pullback_pips,
                    config.max_fomo_pips,
                    config.max_candle_mult,
                    config.sl_mult,
                    config.tp_mult
                );

                let summary = format!(
                    "Applied config: min_score={} min_conf={:.2} sideway={:.3} trend={:.3} pullback={:.1} fomo={:.1} candle_mult={:.2} sl={:.2} tp={:.2}\nTrainMetrics: total_pnl={:.3} trades={} winrate={:.2} expectancy={:.3} max_dd={:.3}\nTestMetrics: total_pnl={:.3} trades={} winrate={:.2} expectancy={:.3} max_dd={:.3}",
                    config.min_score,
                    config.min_confidence,
                    config.sideway_ema_threshold,
                    config.min_trend_strength,
                    config.max_pullback_pips,
                    config.max_fomo_pips,
                    config.max_candle_mult,
                    config.sl_mult,
                    config.tp_mult,
                    result.train_metrics.total_pnl,
                    result.train_metrics.total_trades,
                    result.train_metrics.winrate,
                    result.train_metrics.expectancy,
                    result.train_metrics.max_drawdown,
                    result.test_metrics.total_pnl,
                    result.test_metrics.total_trades,
                    result.test_metrics.winrate,
                    result.test_metrics.expectancy,
                    result.test_metrics.max_drawdown,
                );
                if slack.is_enabled() {
                    // Build list of changed params (compare old_cfg -> result.best_config)
                    let mut changes: Vec<(String,String,String)> = Vec::new();
                    if old_cfg.min_score != result.best_config.min_score { changes.push(("min_score".to_string(), old_cfg.min_score.to_string(), result.best_config.min_score.to_string())); }
                    if (old_cfg.min_confidence - result.best_config.min_confidence).abs() > std::f64::EPSILON { changes.push(("min_confidence".to_string(), format!("{:.2}", old_cfg.min_confidence), format!("{:.2}", result.best_config.min_confidence))); }
                    if (old_cfg.sideway_ema_threshold - result.best_config.sideway_ema_threshold).abs() > std::f64::EPSILON { changes.push(("sideway_ema_threshold".to_string(), format!("{:.3}", old_cfg.sideway_ema_threshold), format!("{:.3}", result.best_config.sideway_ema_threshold))); }
                    if (old_cfg.min_trend_strength - result.best_config.min_trend_strength).abs() > std::f64::EPSILON { changes.push(("min_trend_strength".to_string(), format!("{:.3}", old_cfg.min_trend_strength), format!("{:.3}", result.best_config.min_trend_strength))); }
                    if (old_cfg.max_pullback_pips - result.best_config.max_pullback_pips).abs() > std::f64::EPSILON { changes.push(("max_pullback_pips".to_string(), format!("{:.1}", old_cfg.max_pullback_pips), format!("{:.1}", result.best_config.max_pullback_pips))); }
                    if (old_cfg.max_fomo_pips - result.best_config.max_fomo_pips).abs() > std::f64::EPSILON { changes.push(("max_fomo_pips".to_string(), format!("{:.1}", old_cfg.max_fomo_pips), format!("{:.1}", result.best_config.max_fomo_pips))); }
                    if (old_cfg.max_candle_mult - result.best_config.max_candle_mult).abs() > std::f64::EPSILON { changes.push(("max_candle_mult".to_string(), format!("{:.2}", old_cfg.max_candle_mult), format!("{:.2}", result.best_config.max_candle_mult))); }
                    if (old_cfg.sl_mult - result.best_config.sl_mult).abs() > std::f64::EPSILON { changes.push(("sl_mult".to_string(), format!("{:.2}", old_cfg.sl_mult), format!("{:.2}", result.best_config.sl_mult))); }
                    if (old_cfg.tp_mult - result.best_config.tp_mult).abs() > std::f64::EPSILON { changes.push(("tp_mult".to_string(), format!("{:.2}", old_cfg.tp_mult), format!("{:.2}", result.best_config.tp_mult))); }

                    if let Err(e) = slack.send_optimizer_update("Optimizer Applied", &summary, Some(changes)) {
                        warn!("Failed to send optimizer update to Slack: {}", e);
                    } else {
                        info!("Optimizer update sent to Slack");
                    }
                }

            }
        }
    }



    
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

        // Warmup flag - will be set true after we finish warmup (history loaded or ticks collected)
        let mut warmup_done: bool = false;

        // Load historical candles either via MT5 Python bridge or from history file if available

                // Track whether any historical candles were loaded
        let mut history_loaded: bool = false;
        // Whether we've sent the startup/warmup Slack notification
        let mut startup_notified: bool = false;


        if args.use_mt5_bridge {
        // Use ZeroMQ DEALER to request HISTORY from python_bridge (ROUTER on order_addr)
        let bridge_ctx = zmq::Context::new();
        match bridge_ctx.socket(zmq::DEALER) {
            Ok(sock) => {
                if let Err(e) = sock.connect(&args.order_addr) {
                    warn!("Failed to connect to python bridge at {}: {}", args.order_addr, e);
                } else {
                    // set receive timeout
                    sock.set_rcvtimeo(5000).ok();
                    let req = serde_json::json!({
                        "type": "HISTORY",
                        "data": { "symbol": args.mt5_symbol, "count": args.history_count }
                    });
                    let s = req.to_string();

                    // Try repeatedly until we get a response or timeout
                    let start = Instant::now();
                    let mut history_resp: Option<String> = None;
                    while start.elapsed().as_secs() < args.history_wait_sec {
                        debug!("HISTORY request (elapsed {}s)", start.elapsed().as_secs());
                        match sock.send(s.as_bytes(), 0) {
                            Ok(_) => {
                                debug!("Sent HISTORY request to bridge: {}", s);
                                match sock.recv_string(0) {
                                    Ok(Ok(resp)) => {
                                        debug!("Bridge raw response: {}", resp);
                                        history_resp = Some(resp);
                                        break;
                                    }
                                    Ok(Err(_)) => warn!("Non-UTF8 response from bridge"),
                                    Err(e) => debug!("No reply from python bridge yet: {:?}", e),
                                }
                            }
                            Err(e) => debug!("Failed to send HISTORY request to bridge: {}", e),
                        }
                        thread::sleep(Duration::from_millis(500));
                    }

                    if let Some(resp) = history_resp {
                        // Try parse as list of candles or wrapper
                        if let Ok(mut v) = serde_json::from_str::<Vec<Candle>>(&resp) {

                            if v.len() > args.history_count { v.drain(0..v.len()-args.history_count); }
                            if !v.is_empty() {
                                info!("Loaded {} historical candles from python_bridge for {}", v.len(), args.mt5_symbol);
                                history_loaded = true;
                                for c in &v {
                                    state.push_candle(*c);
                                    price_history.push_back(c.close);
                                    high_history.push_back(c.high);
                                    low_history.push_back(c.low);
                                }
                                const MAX_HISTORY: usize = 100;
                                while price_history.len() > MAX_HISTORY { price_history.pop_front(); }
                                while high_history.len() > MAX_HISTORY { high_history.pop_front(); }
                                while low_history.len() > MAX_HISTORY { low_history.pop_front(); }
                                last_completed_candle = v.last().cloned();
                                // If we loaded enough history, mark warmup done
                                                                if price_history.len() >= 100 {
                                    warmup_done = true;
                                    info!("✅ WARMUP COMPLETE (from history): {} ticks collected, indicators ready", price_history.len());
                                    if let (Some(f), Some(s)) = (state.ema_fast, state.ema_slow) {
                                        info!("INDICATORS: EMA_fast={:.3} EMA_slow={:.3}", f, s);
                                    }
                                    if let Some(r) = state.rsi { info!("INDICATORS: RSI={:.2}", r); }
                                    if let Some(a) = state.atr { info!("INDICATORS: ATR={:.4}", a); }

                                    // Send Slack startup/warmup notification once
                                    if slack.is_enabled() && !startup_notified {
                                        let msg = format!("Engine READY (from history): {} ticks collected", price_history.len());
                                        match slack.send_status(&msg) {
                                            Ok(_) => info!("Startup notification sent to Slack"),
                                            Err(e) => warn!("Failed to send startup Slack notification: {}", e),
                                        }
                                        startup_notified = true;
                                    }
                                }

                            }

                        } else if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&resp) {
                            if let Some(arr) = wrapper.get("data").and_then(|d| d.as_array()) {
                                let mut v: Vec<Candle> = Vec::new();
                                for it in arr {
                                    let time = it.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                                    let open = it.get("open").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                    let high = it.get("high").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                    let low = it.get("low").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                    let close = it.get("close").and_then(|x| x.as_f64()).unwrap_or(0.0);
                                    let volume = it.get("volume").and_then(|x| x.as_i64()).unwrap_or(0);
                                    v.push(Candle { time, open, high, low, close, volume });
                                }
                                if v.len() > args.history_count { v.drain(0..v.len()-args.history_count); }
                                if !v.is_empty() {
                                    info!("Loaded {} historical candles from python_bridge (wrapper) for {}", v.len(), args.mt5_symbol);
                                    history_loaded = true;
                                    for c in &v {
                                        state.push_candle(*c);
                                        price_history.push_back(c.close);
                                        high_history.push_back(c.high);
                                        low_history.push_back(c.low);
                                    }
                                    const MAX_HISTORY: usize = 100;
                                    while price_history.len() > MAX_HISTORY { price_history.pop_front(); }
                                    while high_history.len() > MAX_HISTORY { high_history.pop_front(); }
                                    while low_history.len() > MAX_HISTORY { low_history.pop_front(); }
                                    last_completed_candle = v.last().cloned();
                                    if price_history.len() >= 100 {
                                        warmup_done = true;
                                        info!("✅ WARMUP COMPLETE (from history): {} ticks collected, indicators ready", price_history.len());
                                        if let (Some(f), Some(s)) = (state.ema_fast, state.ema_slow) {
                                            info!("INDICATORS: EMA_fast={:.3} EMA_slow={:.3}", f, s);
                                        }
                                        if let Some(r) = state.rsi { info!("INDICATORS: RSI={:.2}", r); }
                                        if let Some(a) = state.atr { info!("INDICATORS: ATR={:.4}", a); }

                                        // Send Slack startup/warmup notification once
                                        if slack.is_enabled() && !startup_notified {
                                            let msg = format!("Engine READY (from history): {} ticks collected", price_history.len());
                                            match slack.send_status(&msg) {
                                                Ok(_) => info!("Startup notification sent to Slack"),
                                                Err(e) => warn!("Failed to send startup Slack notification: {}", e),
                                            }
                                            startup_notified = true;
                                        }
                                    }

                                }
                            }
                        } else {
                            warn!("Unexpected HISTORY response from bridge: {}", resp);
                            // save raw response for debugging
                            if let Err(e) = fs::write("bridge_history_resp.json", resp.as_bytes()) {
                                warn!("Failed to save bridge response file: {}", e);
                            } else {
                                warn!("Saved bridge raw response to bridge_history_resp.json for debugging");
                            }
                        }
                    } else {
                        warn!("Did not receive history from bridge within {}s", args.history_wait_sec);
                    }
                }
                // socket will close when dropped
            }

            Err(e) => warn!("Failed to create ZMQ DEALER socket for bridge: {}", e),
        }
    } else if !args.history_file.is_empty() && std::path::Path::new(&args.history_file).exists() {

        match std::fs::read_to_string(&args.history_file) {
            Ok(s) => {
                if !s.trim().is_empty() {
                    match serde_json::from_str::<Vec<Candle>>(&s) {
                        Ok(mut v) => {
                            // keep only the most recent history_count candles
                            if v.len() > args.history_count {
                                v.drain(0..v.len()-args.history_count);
                            }
                            if !v.is_empty() {
                                info!("Loaded {} historical candles from {}", v.len(), args.history_file);
                                history_loaded = true;
                                for c in &v {
                                    state.push_candle(*c);
                                    price_history.push_back(c.close);
                                    high_history.push_back(c.high);
                                    low_history.push_back(c.low);
                                }
                                // Trim to runtime MAX_HISTORY
                                const MAX_HISTORY: usize = 100;
                                while price_history.len() > MAX_HISTORY { price_history.pop_front(); }
                                while high_history.len() > MAX_HISTORY { high_history.pop_front(); }
                                while low_history.len() > MAX_HISTORY { low_history.pop_front(); }

                                last_completed_candle = v.last().cloned();
                                                                if price_history.len() >= 100 {
                                    warmup_done = true;
                                    info!("✅ WARMUP COMPLETE (from history): {} ticks collected, indicators ready", price_history.len());
                                    if let (Some(f), Some(s)) = (state.ema_fast, state.ema_slow) {
                                        info!("INDICATORS: EMA_fast={:.3} EMA_slow={:.3}", f, s);
                                    }
                                    if let Some(r) = state.rsi { info!("INDICATORS: RSI={:.2}", r); }
                                    if let Some(a) = state.atr { info!("INDICATORS: ATR={:.4}", a); }

                                    // Send Slack startup/warmup notification once
                                    if slack.is_enabled() && !startup_notified {
                                        let msg = format!("Engine READY (from history): {} ticks collected", price_history.len());
                                        match slack.send_status(&msg) {
                                            Ok(_) => info!("Startup notification sent to Slack"),
                                            Err(e) => warn!("Failed to send startup Slack notification: {}", e),
                                        }
                                        startup_notified = true;
                                    }
                                }

                            }

                        }
                        Err(e) => warn!("Failed to parse history file {}: {}", args.history_file, e),
                    }
                }
            }
            Err(e) => warn!("Failed to read history file {}: {}", args.history_file, e),
        }
    }

    // If user requested history to be required, exit if we couldn't load any
    if args.require_history && !history_loaded {
        error!("History required at startup but none was loaded. Exiting.");
        std::process::exit(1);
    }

    

    
        // Cooldown tracking
            let mut last_action_time: Option<DateTime<Utc>> = None;
            let mut last_status_time: Option<DateTime<Utc>> = None;
            let mut last_heartbeat_time: Option<DateTime<Utc>> = None;

            // Order rate limiting window to prevent accidental bursts / batch floods
            // Restrict to MAX_ORDERS_PER_WINDOW orders per WINDOW_SEC seconds
            let mut orders_window_start: Option<Instant> = None;
            let mut orders_in_window: usize = 0;
            const MAX_ORDERS_PER_WINDOW: usize = 20;
            const WINDOW_SEC: u64 = 1;

    
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

                // Short per-tick log (visible when --per-tick-log)
                if args.per_tick_log {
                    let ema_fast = state.ema_fast.unwrap_or(0.0);
                    let ema_slow = state.ema_slow.unwrap_or(0.0);
                    let ema_diff = (ema_fast - ema_slow).abs();
                    let rsi_val = state.rsi.unwrap_or(0.0);
                    let atr_val = state.atr.unwrap_or(0.0);
                    info!("TICK | P={:.3} ΔEMA={:.3} RSI={:.1} ATR={:.3} Ticks={}", price, ema_diff, rsi_val, atr_val, state.ticks_processed);
                }

                                // Optional per-tick analysis logging
                if args.per_tick_log {
                    let ema_fast = state.ema_fast.unwrap_or(0.0);
                    let ema_slow = state.ema_slow.unwrap_or(0.0);
                    let rsi_val = state.rsi.unwrap_or(0.0);
                    let atr_val = state.atr.unwrap_or(0.0);
                    let ema_diff = (ema_fast - ema_slow).abs();
                    let structure = detect_structure(&highs_vec, &lows_vec, 20);
                    // Use INFO level for per-tick logs to ensure visibility when requested
                    debug!("TICK ANALYSIS | Price={:.3} EMA_fast={:.3} EMA_slow={:.3} EMA_diff={:.3} RSI={:.2} ATR={:.4} Struct={:?} Ticks={}",
                        price, ema_fast, ema_slow, ema_diff, rsi_val, atr_val, structure, state.ticks_processed);
                }


                if !warmup_done && price_history.len() < 100 {
                    info!("🔄 WARMUP: {}/{} ticks collected (need {} for indicators)", price_history.len(), 100, 100);
                    continue;
                }
                if !warmup_done && price_history.len() >= 100 {
                    warmup_done = true;
                    info!("✅ WARMUP COMPLETE: {} ticks collected, indicators ready", price_history.len());
                    if let (Some(f), Some(s)) = (state.ema_fast, state.ema_slow) {
                        info!("INDICATORS: EMA_fast={:.3} EMA_slow={:.3}", f, s);
                    }
                    if let Some(r) = state.rsi { info!("INDICATORS: RSI={:.2}", r); }
                    if let Some(a) = state.atr { info!("INDICATORS: ATR={:.4}", a); }
                }



                
                                                                if args.auto_reload_optimized_config {
                                    let now = Utc::now();
                                    if last_heartbeat_time.map_or(true, |t| (now - t).num_seconds() >= args.optimizer_reload_sec as i64) {
                                        if args.scalp_mode {
                                            // When scalp_mode is enabled, prefer the scalping presets and skip auto-reload
                                            info!("SCALP MODE active: skipping auto-reload of optimizer config to preserve scalping presets");
                                        } else {
                                            if let Some(result) = load_optimization_result(&args.optimizer_output_file) {
                                                config.min_score = result.best_config.min_score;
                                                config.min_confidence = result.best_config.min_confidence;
                                                config.sideway_ema_threshold = result.best_config.sideway_ema_threshold;
                                                config.min_trend_strength = result.best_config.min_trend_strength;
                                                config.max_pullback_pips = result.best_config.max_pullback_pips;
                                                config.max_fomo_pips = result.best_config.max_fomo_pips;
                                                config.max_candle_mult = result.best_config.max_candle_mult;
                                                config.sl_mult = result.best_config.sl_mult;
                                                config.tp_mult = result.best_config.tp_mult;
                                                config.require_confirmation = result.best_config.require_confirmation;
                                                info!("CONFIG RELOADED FROM OPTIMIZER: min_score={} min_conf={:.2} sideway={:.3} trend={:.3} pullback={:.1} fomo={:.1} candle_mult={:.2} sl={:.2} tp={:.2}",
                                                    config.min_score,
                                                    config.min_confidence,
                                                    config.sideway_ema_threshold,
                                                    config.min_trend_strength,
                                                    config.max_pullback_pips,
                                                    config.max_fomo_pips,
                                                    config.max_candle_mult,
                                                    config.sl_mult,
                                                    config.tp_mult);
                                            }
                                        }
                                        last_heartbeat_time = Some(now);
                                    }
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
                
                // Periodic status: send bot status to Slack every status_interval_sec seconds (independent of signal)
                let now = Utc::now();
                if last_status_time.map_or(true, |t| (now - t).num_seconds() >= args.status_interval_sec as i64) {
                    let trend_status = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                        if state.ema_fast.unwrap() > state.ema_slow.unwrap() { "📈 Uptrend" } else { "📉 Downtrend" }
                    } else { "⚪ Flat" };

                    // Last or current candle info
                    let candle_info = if let Some(c) = last_completed_candle {
                        format!("LastCandle: t={} O={:.2} H={:.2} L={:.2} C={:.2} V={}", c.time, c.open, c.high, c.low, c.close, c.volume)
                    } else {
                        format!("CurrentCandle: t={} O={:.2} H={:.2} L={:.2} C={:.2} V={}", current_candle.time, current_candle.open, current_candle.high, current_candle.low, current_candle.close, current_candle.volume)
                    };














                                        // Build score breakdown to include in status
                    let ema_diff = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                        (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs()
                    } else { 0.0 };
                    let trend_pts = if ema_diff >= 0.20 { 2 } else if ema_diff >= 0.10 { 1 } else { 0 };
                    let strength_pts = if ema_diff >= 0.40 { 2 } else if ema_diff >= 0.20 { 1 } else { 0 };
                    let highs_vec_tmp: Vec<f64> = state.highs.iter().copied().collect();
                    let lows_vec_tmp: Vec<f64> = state.lows.iter().copied().collect();
                    let structure_tmp = detect_structure(&highs_vec_tmp, &lows_vec_tmp, 20);
                    let struct_pts = match structure_tmp {
                        SwingType::HigherHigh => 2,
                        SwingType::HigherLow => 1,
                        SwingType::LowerLow => 2,
                        SwingType::LowerHigh => 1,
                        _ => 0,
                    };
                    let pullback_ok_tmp = state.ema_fast.map_or(false, |ema| is_pullback(price, ema, config.max_pullback_pips, config.pip_value));
                    let pullback_pts = if pullback_ok_tmp { 1 } else { 0 };
                    let rsi_val_tmp = state.rsi.unwrap_or(50.0);
                    let rsi_pts = if (rsi_val_tmp > 50.0 && rsi_val_tmp < 70.0) || (rsi_val_tmp >= 30.0 && rsi_val_tmp < 50.0) { 1 } else { 0 };
                    let vol_pts = 1; // assumed OK
                    let confirm_pts = if config.require_confirmation { 1 } else { 0 };
                    let total_pts = trend_pts + strength_pts + struct_pts + pullback_pts + rsi_pts + vol_pts + confirm_pts;

                    let breakdown_text = format!("[ScoreBreakdown] T:{} S:{} St:{} P:{} R:{} V:{} C:{} Total:{}",
                        trend_pts, strength_pts, struct_pts, pullback_pts, rsi_pts, vol_pts, confirm_pts, total_pts);

                    let status_msg = format!(
                        "Price={:.2} | {} | EMA20/50={} | {} | Ticks={} | {} | {}",
                        price, trend_status, ema_str, rsi_str, state.ticks_processed, candle_info, breakdown_text
                    );

                    if slack.is_enabled() {
                        if let Err(e) = slack.send_status(&status_msg) {
                            warn!("⚠️ Slack status failed: {}", e);
                        } else {
                            debug!("Slack status sent");
                        }
                    }
                    last_status_time = Some(now);
                }

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
                                            let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                                            let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                                            let log_row = make_strategy_row(
                                                &resolved_symbol,
                                                "NONE",
                                                price,
                                                signal.score,
                                                signal.confidence,
                                                state.ema_fast.unwrap_or(0.0),
                                                state.ema_slow.unwrap_or(0.0),
                                                state.rsi.unwrap_or(0.0),
                                                state.atr.unwrap_or(0.0),
                                                &format!("{:?}", detect_structure(&highs_vec, &lows_vec, 20)),
                                                0,
                                                0,
                                                &signal.reason,
                                                "SKIP",
                                                None,
                                            );
                                            let _ = append_strategy_log(&args.live_log_file, &log_row);
                                        }

                    SignalAction::Hold => { 
                                                // Log status every status_interval_sec seconds to show engine is alive
                                                let now = Utc::now();
                                                if last_status_time.map_or(true, |t| (now - t).num_seconds() >= args.status_interval_sec as i64) {
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
                            
                                                                // Prepare Slack status text (include candle status)
                                                                let candle_info = if let Some(c) = last_completed_candle {
                                                                    format!("LastCandle: t={} O={:.2} H={:.2} L={:.2} C={:.2} V={}", c.time, c.open, c.high, c.low, c.close, c.volume)
                                                                } else {
                                                                    format!("CurrentCandle: t={} O={:.2} H={:.2} L={:.2} C={:.2} V={}", current_candle.time, current_candle.open, current_candle.high, current_candle.low, current_candle.close, current_candle.volume)
                                                                };





                                                                // Build score breakdown to include in status
                                                                let ema_diff = if state.ema_fast.is_some() && state.ema_slow.is_some() {
                                                                    (state.ema_fast.unwrap() - state.ema_slow.unwrap()).abs()
                                                                } else { 0.0 };
                                                                let trend_pts = if ema_diff >= 0.20 { 2 } else if ema_diff >= 0.10 { 1 } else { 0 };
                                                                let strength_pts = if ema_diff >= 0.40 { 2 } else if ema_diff >= 0.20 { 1 } else { 0 };
                                                                let highs_vec_tmp: Vec<f64> = state.highs.iter().copied().collect();
                                                                let lows_vec_tmp: Vec<f64> = state.lows.iter().copied().collect();
                                                                let structure_tmp = detect_structure(&highs_vec_tmp, &lows_vec_tmp, 20);
                                                                let struct_pts = match structure_tmp {
                                                                    SwingType::HigherHigh => 2,
                                                                    SwingType::HigherLow => 1,
                                                                    SwingType::LowerLow => 2,
                                                                    SwingType::LowerHigh => 1,
                                                                    _ => 0,
                                                                };
                                                                let pullback_ok_tmp = state.ema_fast.map_or(false, |ema| is_pullback(price, ema, config.max_pullback_pips, config.pip_value));
                                                                let pullback_pts = if pullback_ok_tmp { 1 } else { 0 };
                                                                let rsi_val_tmp = state.rsi.unwrap_or(50.0);
                                                                let rsi_pts = if (rsi_val_tmp > 50.0 && rsi_val_tmp < 70.0) || (rsi_val_tmp >= 30.0 && rsi_val_tmp < 50.0) { 1 } else { 0 };
                                                                let vol_pts = 1; // assumed OK
                                                                let confirm_pts = if config.require_confirmation { 1 } else { 0 };
                                                                let total_pts = trend_pts + strength_pts + struct_pts + pullback_pts + rsi_pts + vol_pts + confirm_pts;

                                                                let breakdown_text = format!("[ScoreBreakdown] T:{} S:{} St:{} P:{} R:{} V:{} C:{} Total:{}",
                                                                    trend_pts, strength_pts, struct_pts, pullback_pts, rsi_pts, vol_pts, confirm_pts, total_pts);

                                                                let status_msg = format!(
                                                                    "Price={:.2} | {} | EMA20/50={} | {} | Ticks={} | {} | {}",
                                                                    price, trend_status, ema_str, rsi_str, state.ticks_processed, candle_info, breakdown_text
                                                                );

                                                                if slack.is_enabled() {
                                                                    if let Err(e) = slack.send_status(&status_msg) {
                                                                        warn!("⚠️ Slack status failed: {}", e);
                                                                    } else {
                                                                        debug!("Slack status sent");
                                                                    }
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
                        










                                                // Build breakdown for heartbeat as well
                        let highs_vec_tmp: Vec<f64> = state.highs.iter().copied().collect();
                        let lows_vec_tmp: Vec<f64> = state.lows.iter().copied().collect();
                        let structure_tmp = detect_structure(&highs_vec_tmp, &lows_vec_tmp, 20);
                        let trend_pts_h = if ema_diff >= 0.20 { 2 } else if ema_diff >= 0.10 { 1 } else { 0 };
                        let strength_pts_h = if ema_diff >= 0.40 { 2 } else if ema_diff >= 0.20 { 1 } else { 0 };
                        let struct_pts_h = match structure_tmp { SwingType::HigherHigh => 2, SwingType::HigherLow => 1, SwingType::LowerLow => 2, SwingType::LowerHigh => 1, _ => 0 };
                        let pullback_ok_tmp = state.ema_fast.map_or(false, |ema| is_pullback(price, ema, config.max_pullback_pips, config.pip_value));
                        let pullback_pts_h = if pullback_ok_tmp { 1 } else { 0 };
                        let rsi_val_tmp = state.rsi.unwrap_or(50.0);
                        let rsi_pts_h = if (rsi_val_tmp > 50.0 && rsi_val_tmp < 70.0) || (rsi_val_tmp >= 30.0 && rsi_val_tmp < 50.0) { 1 } else { 0 };
                        let vol_pts_h = 1;
                        let confirm_pts_h = if config.require_confirmation { 1 } else { 0 };
                        let total_score = trend_pts_h + strength_pts_h + struct_pts_h + pullback_pts_h + rsi_pts_h + vol_pts_h + confirm_pts_h;

                        let breakdown_text = format!("T:{} S:{} St:{} P:{} R:{} V:{} C:{}", trend_pts_h, strength_pts_h, struct_pts_h, pullback_pts_h, rsi_pts_h, vol_pts_h, confirm_pts_h);

                        let status_msg = format!(
                            "Price={:.2} | {} | Score={}/10 | Breakdown={} | Cooldown={} | Long={} Short={} | Ticks={}",
                            price, trend_status, total_score, breakdown_text, state.cooldown_counter,
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
                            
                                                                                                                // Before sending order, query broker symbol info (stop level, point) via python_bridge
                                                        let mut sl_to_send = signal.stop_loss;
                                                        let mut tp_to_send = signal.take_profit;
                                                        let pip = config.pip_value;
                                                        // Increase min stop distance slightly to avoid SL being placed too tight on low-ATR ticks
                            let mut min_stop_distance = 0.5_f64.max(state.atr.unwrap_or(0.0) * config.sl_mult * 1.2);

                                                        // Try to get symbol info from python bridge to determine stop level
                                                        let sym_req = serde_json::json!({ "type": "SYMBOL_INFO", "data": { "symbol": resolved_symbol } });
                                                        let sym_req_s = sym_req.to_string();
                                                        if let Err(e) = sock.send(sym_req_s.as_bytes(), 0) {
                                                            debug!("Failed to request SYMBOL_INFO: {:?}", e);
                                                        } else {
                                                            match sock.recv_string(0) {
                                                                Ok(Ok(resp)) => {
                                                                    debug!("SYMBOL_INFO raw: {}", resp);
                                                                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp) {
                                                                        if v.get("success").and_then(|x| x.as_bool()).unwrap_or(false) {
                                                                            if let Some(data) = v.get("data") {
                                                                                // Extract point and stop_level if present
                                                                                let point = data.get("point").and_then(|x| x.as_f64()).unwrap_or(config.pip_value);
                                                                                let stop_level_opt = data.get("stop_level").and_then(|x| x.as_f64());
                                                                                if let Some(stop_pts) = stop_level_opt {
                                                                                    let stop_price = stop_pts * point;
                                                                                    if stop_price > 0.0 {
                                                                                        min_stop_distance = stop_price.max(min_stop_distance);
                                                                                        debug!("Broker stop_level: {} pts, point={} => min_stop_distance set to {}", stop_pts, point, min_stop_distance);
                                                                                    }
                                                                                }
                                                                            }
                                                                        } else if v.get("point").is_some() {
                                                                            // sometimes raw symbol dict returned without success wrapper
                                                                            let data = &v;
                                                                            let point = data.get("point").and_then(|x| x.as_f64()).unwrap_or(config.pip_value);
                                                                            let stop_level_opt = data.get("stop_level").and_then(|x| x.as_f64());
                                                                            if let Some(stop_pts) = stop_level_opt {
                                                                                let stop_price = stop_pts * point;
                                                                                if stop_price > 0.0 {
                                                                                    min_stop_distance = stop_price.max(min_stop_distance);
                                                                                    debug!("Broker stop_level (raw): {} pts, point={} => min_stop_distance set to {}", stop_pts, point, min_stop_distance);
                                                                                }
                                                                            }
                                                                        }
                                                                    }
                                                                }
                                                                Ok(Err(_)) => debug!("Non-UTF8 SYMBOL_INFO reply"),
                                                                Err(e) => debug!("No SYMBOL_INFO reply: {:?}", e),
                                                            }
                                                        }

                                                        // Add small buffer (one pip) to be safe
                                                        min_stop_distance += pip;

                                                        // Ensure SL/TP satisfy broker min distance and correct side
                                                        match signal.direction {
                                                            Direction::Long => {
                                                                let entry = signal.entry_price;
                                                                if (entry - sl_to_send).abs() < min_stop_distance {
                                                                    sl_to_send = entry - min_stop_distance;
                                                                }
                                                                // ensure TP is beyond minimum relative to entry (keep ratio)
                                                                let sl_dist = (entry - sl_to_send).abs();
                                                                let tp_req = sl_dist * (config.tp_mult / config.sl_mult);
                                                                if (tp_to_send - entry).abs() < tp_req {
                                                                    tp_to_send = entry + tp_req;
                                                                }
                                                            }
                                                            Direction::Short => {
                                                                let entry = signal.entry_price;
                                                                if (sl_to_send - entry).abs() < min_stop_distance {
                                                                    sl_to_send = entry + min_stop_distance;
                                                                }
                                                                let sl_dist = (sl_to_send - entry).abs();
                                                                let tp_req = sl_dist * (config.tp_mult / config.sl_mult);
                                                                if (entry - tp_to_send).abs() < tp_req {
                                                                    tp_to_send = entry - tp_req;
                                                                }
                                                            }
                                                            _ => {}
                                                        }

                                                                                                                // Round stops to pip grid with directional rounding to ensure they are strictly beyond min_stop_distance
                                                        fn round_down_to_pip(x: f64, pip: f64) -> f64 { (x / pip).floor() * pip }
                                                        fn round_up_to_pip(x: f64, pip: f64) -> f64 { (x / pip).ceil() * pip }

                                                        // Add a larger extra buffer to move SL further away from entry (reduces premature SL hits)
                                                                                                                // Increase this multiplier if you want SL even further away
                                                                                                                let extra = pip * 4.0;
                                                        match signal.direction {
                                                            Direction::Long => {
                                                                let entry = signal.entry_price;
                                                                // ensure SL is at least min_stop_distance + extra away
                                                                let desired_sl = entry - (min_stop_distance + extra);
                                                                sl_to_send = round_down_to_pip(desired_sl, pip);
                                                                // if rounding moved it too close, push one more pip away
                                                                if (entry - sl_to_send) < min_stop_distance + 0.0000001 {
                                                                    sl_to_send = round_down_to_pip(desired_sl - pip, pip);
                                                                }

                                                                // ensure TP is at least required ratio away and rounded up
                                                                let sl_dist = (entry - sl_to_send).abs();
                                                                let tp_req = sl_dist * (config.tp_mult / config.sl_mult);
                                                                let desired_tp = entry + tp_req + extra;
                                                                tp_to_send = round_up_to_pip(desired_tp, pip);
                                                            }
                                                            Direction::Short => {
                                                                let entry = signal.entry_price;
                                                                let desired_sl = entry + (min_stop_distance + extra);
                                                                sl_to_send = round_up_to_pip(desired_sl, pip);
                                                                if (sl_to_send - entry) < min_stop_distance + 0.0000001 {
                                                                    sl_to_send = round_up_to_pip(desired_sl + pip, pip);
                                                                }

                                                                let sl_dist = (sl_to_send - entry).abs();
                                                                let tp_req = sl_dist * (config.tp_mult / config.sl_mult);
                                                                let desired_tp = entry - tp_req - extra;
                                                                tp_to_send = round_down_to_pip(desired_tp, pip);
                                                            }
                                                            _ => {}
                                                        }

                                                        let payload = serde_json::json!({
                                                            "type": "ORDER_SEND",
                                                            "data": {
                                                                "symbol": resolved_symbol,
                                                                "volume": args.volume,
                                                                "order_type": order_type,
                                                                "price": 0,
                                                                "stop_loss": sl_to_send,
                                                                "take_profit": tp_to_send,
                                                                "comment": comment,
                                                                "magic": 2100,
                                                                "request_id": request_id
                                                            }
                                                        });
                            
                                                        let s = payload.to_string();
                            info!("📤 EXECUTING {} {} lots @ {} | SL={:.5} TP={:.5} (min_stop_req={:.5})",
                                order_type, args.volume, signal.entry_price, sl_to_send, tp_to_send, min_stop_distance);

                                                        // Order rate limiting: enforce MAX_ORDERS_PER_WINDOW per WINDOW_SEC
                            let now_instant = Instant::now();
                            if let Some(start) = orders_window_start {
                                if now_instant.duration_since(start).as_secs() >= WINDOW_SEC {
                                    orders_window_start = Some(now_instant);
                                    orders_in_window = 0;
                                }
                            } else {
                                orders_window_start = Some(now_instant);
                                orders_in_window = 0;
                            }

                            if orders_in_window >= MAX_ORDERS_PER_WINDOW {
                                info!("⚠️ Order rate limit reached: {} orders in {}s window - skipping order", MAX_ORDERS_PER_WINDOW, WINDOW_SEC);
                                continue;
                            }
                            orders_in_window += 1;

                            match sock.send(s.as_bytes(), 0) {


                                Ok(_) => match sock.recv_string(0) {
                                                                        Ok(Ok(resp)) => {
                                                        info!("📥 Order response: {}", resp);
                                                        // Parse response JSON and only treat as success if bridge reports success=true
                                                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&resp) {
                                                            if v.get("success").and_then(|x| x.as_bool()).unwrap_or(false) {
                                                                // Successful order - update state and notify Slack
                                                                state.record_trade(signal.entry_price, true);
                                                                if signal.direction == Direction::Long { state.long_positions += 1; }
                                                                else if signal.direction == Direction::Short { state.short_positions += 1; }

                                                                let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                                                                let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                                                                let log_row = make_strategy_row(
                                                                    &resolved_symbol,
                                                                    order_type,
                                                                    signal.entry_price,
                                                                    signal.score,
                                                                    signal.confidence,
                                                                    state.ema_fast.unwrap_or(0.0),
                                                                    state.ema_slow.unwrap_or(0.0),
                                                                    state.rsi.unwrap_or(0.0),
                                                                    state.atr.unwrap_or(0.0),
                                                                    &format!("{:?}", detect_structure(&highs_vec, &lows_vec, 20)),
                                                                    0,
                                                                    0,
                                                                    &signal.reason,
                                                                    "ENTER",
                                                                    None,
                                                                );
                                                                let _ = append_strategy_log(&args.live_log_file, &log_row);

                                                                // Send Slack notification for executed order (only on success)
                                                                if slack.is_enabled() {
                                                                    if let Err(e) = slack.send_order_executed(
                                                                        order_type,
                                                                        args.volume,
                                                                        signal.entry_price,
                                                                        &request_id
                                                                    ) {
                                                                        warn!("⚠️ Slack order notification failed: {}", e);
                                                                    }
                                                                }
                                                            } else {
                                                                // Bridge reported failure - log and do NOT notify Slack
                                                                let err = v.get("error_message").or_else(|| v.get("error")).or_else(|| v.get("comment")).and_then(|x| x.as_str()).unwrap_or("<no error>");
                                                                warn!("ORDER FAILED from bridge: {}", err);
                                                            }
                                                        } else {
                                                            warn!("ORDER RESPONSE JSON parse failed: {}", resp);
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

                        let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                        let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                        let log_row = make_strategy_row(
                            &resolved_symbol,
                            match signal.direction {
                                Direction::Long => "BUY",
                                Direction::Short => "SELL",
                                Direction::None => "HOLD",
                            },
                            signal.entry_price,
                            signal.score,
                            signal.confidence,
                            state.ema_fast.unwrap_or(0.0),
                            state.ema_slow.unwrap_or(0.0),
                            state.rsi.unwrap_or(0.0),
                            state.atr.unwrap_or(0.0),
                            &format!("{:?}", detect_structure(&highs_vec, &lows_vec, 20)),
                            0,
                            0,
                            &signal.reason,
                            "ENTER",
                            None,
                        );
                        let _ = append_strategy_log(&args.live_log_file, &log_row);
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

                        let highs_vec: Vec<f64> = state.highs.iter().copied().collect();
                        let lows_vec: Vec<f64> = state.lows.iter().copied().collect();
                        let log_row = make_strategy_row(
                            &resolved_symbol,
                            direction,
                            price,
                            0,
                            0.0,
                            state.ema_fast.unwrap_or(0.0),
                            state.ema_slow.unwrap_or(0.0),
                            state.rsi.unwrap_or(0.0),
                            state.atr.unwrap_or(0.0),
                            &format!("{:?}", detect_structure(&highs_vec, &lows_vec, 20)),
                            0,
                            0,
                            &format!("close ticket={} magic={}", ticket, magic),
                            "CLOSE",
                            Some(profit),
                        );
                                                let _ = append_strategy_log(&args.live_log_file, &log_row);
                        
                        // Send Slack notification
                        if let Err(e) = slack.send_position_closed(ticket, direction, volume, price, profit, magic) {
                            warn!("Slack close notification failed: {}", e);
                        }

                        // If auto-optimization enabled, run a quick incremental optimizer after each close
                        if args.auto_optimize {
                            let logs = load_trade_logs(&args.strategy_log_file);
                                                        // Determine base config for incremental optimizer (prefer last saved best_config)
                            let base_cfg: Config = if let Some(res) = load_optimization_result(&args.optimizer_output_file) {
                                res.best_config
                            } else {
                                config.clone()
                            };

                            // Load history candles for optimizer (if available)
                                                        let mut history_candles: Vec<Candle> = Vec::new();
                                                        if !args.history_file.is_empty() && std::path::Path::new(&args.history_file).exists() {
                                                            if let Ok(s) = std::fs::read_to_string(&args.history_file) {
                                                                history_candles = serde_json::from_str::<Vec<Candle>>(&s).unwrap_or_default();
                                                            }
                                                        }

                                                        // If no file-based history, fallback to in-memory state.candles (if enough data)
                                                        if history_candles.is_empty() {
                                                            if state.candles.len() >= 50 {
                                                                history_candles = state.candles.iter().copied().collect();
                                                                info!("Incremental optimizer: using in-memory candle history ({} candles)", history_candles.len());
                                                            } else {
                                                                warn!("Incremental optimizer: no historical candles found at {} and in-memory history < 50 - skipping incremental optimization", args.history_file);
                                                            }
                                                        }

                                                        if !history_candles.is_empty() {
                                                            let result = optimize(&history_candles, base_cfg.clone());
                                                            if let Err(e) = save_optimization_result(&args.optimizer_output_file, &result) {
                                                                warn!("Failed to save incremental optimization result: {}", e);
                                                            } else {
                                                                info!("Incremental optimizer saved to {}", args.optimizer_output_file);
                                                                // Notify Slack about incremental optimizer
                                                                                                                                if slack.is_enabled() {
                                                                    let summary = format!(
                                                                        "Incremental optimizer applied: min_score={} min_conf={:.2} pullback={} fomo={}",
                                                                        result.best_config.min_score, result.best_config.min_confidence, result.best_config.max_pullback_pips, result.best_config.max_fomo_pips
                                                                    );
                                                                    // Build list of changed params (compare current config -> result.best_config)
                                                                    let mut changes: Vec<(String,String,String)> = Vec::new();
                                                                    if config.min_score != result.best_config.min_score { changes.push(("min_score".to_string(), config.min_score.to_string(), result.best_config.min_score.to_string())); }
                                                                    if (config.min_confidence - result.best_config.min_confidence).abs() > std::f64::EPSILON { changes.push(("min_confidence".to_string(), format!("{:.2}", config.min_confidence), format!("{:.2}", result.best_config.min_confidence))); }
                                                                    if (config.max_pullback_pips - result.best_config.max_pullback_pips).abs() > std::f64::EPSILON { changes.push(("max_pullback_pips".to_string(), format!("{:.1}", config.max_pullback_pips), format!("{:.1}", result.best_config.max_pullback_pips))); }
                                                                    if (config.max_fomo_pips - result.best_config.max_fomo_pips).abs() > std::f64::EPSILON { changes.push(("max_fomo_pips".to_string(), format!("{:.1}", config.max_fomo_pips), format!("{:.1}", result.best_config.max_fomo_pips))); }

                                                                    if let Err(e) = slack.send_optimizer_update("Incremental Optimizer Applied", &summary, Some(changes)) {
                                                                        warn!("Failed to send incremental optimizer update to Slack: {}", e);
                                                                    } else {
                                                                        info!("Incremental optimizer update sent to Slack");
                                                                    }
                                                                }

                                                            }

                                                            // Apply new config gradually
                                                            config.min_score = result.best_config.min_score;
                                                            config.min_confidence = result.best_config.min_confidence;
                                                            config.sideway_ema_threshold = result.best_config.sideway_ema_threshold;
                                                            config.min_trend_strength = result.best_config.min_trend_strength;
                                                            config.max_pullback_pips = result.best_config.max_pullback_pips;
                                                            config.max_fomo_pips = result.best_config.max_fomo_pips;
                                                            config.max_candle_mult = result.best_config.max_candle_mult;
                                                            config.sl_mult = result.best_config.sl_mult;
                                                            config.tp_mult = result.best_config.tp_mult;
                                                            config.require_confirmation = result.best_config.require_confirmation;
  
                                                            info!("INCREMENTAL OPTIMIZER APPLIED: min_score={} min_conf={:.2} pullback={:.1} fomo={:.1}",
                                                                config.min_score, config.min_confidence, config.max_pullback_pips, config.max_fomo_pips);
                                                        }

                        }
                    }


                }
                _ => { /* No message or error - normal */ }
            }
        }
    }
}
