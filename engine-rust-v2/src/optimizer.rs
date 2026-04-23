use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use rand::prelude::*;

use std::f64;

use crate::strategy_new::{Config, State, Candle, should_trade};

/// Backtest result summary required by the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResult {
    pub total_pnl: f64,
    pub total_trades: usize,
    pub winrate: f64,
    pub expectancy: f64,
    pub max_drawdown: f64,
    pub sharpe_ratio: f64,
}

/// Optimization result as required
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub best_config: Config,
    pub train_metrics: BacktestResult,
    pub test_metrics: BacktestResult,
}

// ------------------- Utilities -------------------
fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() { return 0.0; }
    xs.iter().sum::<f64>() / xs.len() as f64
}

fn stddev(xs: &[f64]) -> f64 {
    let m = mean(xs);
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() as f64);
    var.sqrt()
}

fn max_drawdown_from_equity(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &v in equity {
        if v > peak { peak = v; }
        let dd = peak - v;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd
}

// ------------------- Backtest Engine -------------------

/// Replay strategy.should_trade over candles and simulate trades conservatively
pub fn backtest(candles: &[Candle], cfg: &Config) -> BacktestResult {
    let mut state = State::new();

    let mut trades_pnl: Vec<f64> = Vec::new();
    let mut equity: Vec<f64> = Vec::new();
    let mut cum: f64 = 0.0;

    let n = candles.len();
    let mut i = 0usize;
    while i < n {
        let c = &candles[i];
        // update state with candle
        state.push_price(c.close);
        state.push_ohlc(c.high, c.low);
        state.push_candle(*c);

        // call strategy
        // Use close as both bid/ask for backtest replay (conservative)
        let signal = should_trade(&mut state, c.close, c.close, c.close, c, cfg);

        if signal.is_enter() {
            // Execute trade at signal.entry_price (fallback to candle close)
            let entry = if signal.entry_price != 0.0 { signal.entry_price } else { c.close };
            let sl = signal.stop_loss;
            let tp = signal.take_profit;
            let dir = signal.direction.clone();

            // simulate forward until TP or SL hit
            let mut closed = false;
            let mut j = i; // check the same candle first
            while j < n {
                let cc = &candles[j];
                match dir {
                    crate::strategy_new::Direction::Long => {
                        // if both TP and SL hit in same candle -> prefer SL
                        let sl_hit = cc.low <= sl;
                        let tp_hit = cc.high >= tp;
                        if sl_hit {
                            let pnl = sl - entry;
                            trades_pnl.push(pnl);
                            cum += pnl;
                            equity.push(cum);
                            closed = true;
                            break;
                        } else if tp_hit {
                            let pnl = tp - entry;
                            trades_pnl.push(pnl);
                            cum += pnl;
                            equity.push(cum);
                            closed = true;
                            break;
                        }
                    }
                    crate::strategy_new::Direction::Short => {
                        let sl_hit = cc.high >= sl;
                        let tp_hit = cc.low <= tp;
                        if sl_hit {
                            let pnl = entry - sl;
                            trades_pnl.push(pnl);
                            cum += pnl;
                            equity.push(cum);
                            closed = true;
                            break;
                        } else if tp_hit {
                            let pnl = entry - tp;
                            trades_pnl.push(pnl);
                            cum += pnl;
                            equity.push(cum);
                            closed = true;
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }

            if !closed {
                // If not closed by TP/SL before dataset end, close at last candle close
                let last = &candles[n - 1];
                let exit = last.close;
                let pnl = match dir {
                    crate::strategy_new::Direction::Long => exit - entry,
                    crate::strategy_new::Direction::Short => entry - exit,
                    _ => 0.0,
                };
                trades_pnl.push(pnl);
                cum += pnl;
                equity.push(cum);
            }

            // Advance i to j to avoid overlapping entries inside the closed period
            // but be conservative: continue from next candle after entry to allow overlapping signals
            // We'll simply continue to next candle (i += 1)
        }

        i += 1;
    }

    // compute metrics
    let total_trades = trades_pnl.len();
    let total_pnl = trades_pnl.iter().sum::<f64>();
    let wins = trades_pnl.iter().filter(|&&p| p > 0.0).count();
    let winrate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };
    let expectancy = if total_trades > 0 { total_pnl / total_trades as f64 } else { 0.0 };
    let max_dd = max_drawdown_from_equity(&equity);

    let sharpe = if total_trades > 1 {
        let avg = mean(&trades_pnl);
        let sd = stddev(&trades_pnl);
        if sd > 0.0 { avg / sd * (total_trades as f64).sqrt() } else { 0.0 }
    } else { 0.0 };

    BacktestResult {
        total_pnl,
        total_trades,
        winrate,
        expectancy,
        max_drawdown: max_dd,
        sharpe_ratio: sharpe,
    }
}

// ------------------- Optimizer (Random Search) -------------------

/// Randomly search parameter space (300..500 samples) using full replay backtest.
pub fn optimize(candles: &[Candle], base_config: Config) -> OptimizationResult {
    // Train/Test split 70/30
    let n = candles.len();
    let train_n = (n as f64 * 0.7).round() as usize;
    let (train_candles, test_candles) = candles.split_at(train_n);

    // Random search
    let samples = 400usize; // between 300-500
    let mut rng = StdRng::from_entropy();

    let mut best_cfg = base_config.clone();
    let mut best_score = f64::NEG_INFINITY;
    let mut best_train_metrics = backtest(train_candles, &best_cfg);

    for _ in 0..samples {
        // sample candidate
        let mut cfg = base_config.clone();
        cfg.min_score = rng.gen_range(3..=9);
        cfg.min_confidence = rng.gen_range(300..=900) as f64 / 1000.0; // 0.3..0.9
        cfg.sideway_ema_threshold = rng.gen_range(0.05..=1.0);
        cfg.min_trend_strength = rng.gen_range(0.05..=1.0);
        cfg.max_pullback_pips = rng.gen_range(5.0..=40.0);
        cfg.max_fomo_pips = rng.gen_range(8.0..=60.0);
        cfg.max_candle_mult = rng.gen_range(1.0..=3.0);
        cfg.sl_mult = rng.gen_range(0.5..=3.0);
        cfg.tp_mult = rng.gen_range(1.0..=4.0);

        // run backtest on train
        let train_metrics = backtest(train_candles, &cfg);

        // objective: score = sharpe_ratio * 2.0 + expectancy * 10.0 - max_drawdown * 3.0
        let obj = train_metrics.sharpe_ratio * 2.0 + train_metrics.expectancy * 10.0 - train_metrics.max_drawdown * 3.0;

        if obj.is_finite() && obj > best_score {
            best_score = obj;
            best_cfg = cfg.clone();
            best_train_metrics = train_metrics;
        }
    }

    // Validate on test
    let test_metrics = backtest(test_candles, &best_cfg);

    // Reject if test_pnl <= 0 or drawdown too large (conservative rule)
    let mut final_cfg = best_cfg.clone();
    let mut final_train = best_train_metrics.clone();
    let mut final_test = test_metrics.clone();

    let reject = final_test.total_pnl <= 0.0 || (final_test.total_pnl > 0.0 && final_test.max_drawdown > final_test.total_pnl.abs() * 0.5);
    if reject {
        // fallback to base_config (no change) and compute metrics
        final_cfg = base_config.clone();
        final_train = backtest(train_candles, &final_cfg);
        final_test = backtest(test_candles, &final_cfg);
    }

    OptimizationResult {
        best_config: final_cfg,
        train_metrics: final_train,
        test_metrics: final_test,
    }
}

// ------------------- Persistence helpers -------------------

pub fn save_optimization_result(path: &str, result: &OptimizationResult) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, json)
}

pub fn load_optimization_result(path: &str) -> Option<OptimizationResult> {
    if !Path::new(path).exists() { return None; }
    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() { return None; }
    serde_json::from_str::<OptimizationResult>(&content).ok()
}

// Keep a trade log loader for backward compatibility (does not affect optimizer logic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeLogEntry {
    pub timestamp: String,
    pub symbol: String,
    pub direction: String,
    pub price: f64,
    pub score: i32,
    pub confidence: f64,
    pub ema20: f64,
    pub ema50: f64,
    pub rsi: f64,
    pub atr: f64,
    pub structure: String,
    pub reversal_penalty: i32,
    pub structure_penalty: i32,
    pub reason: String,
    pub action: String,
    pub pnl: Option<f64>,
}

pub fn load_trade_logs(path: &str) -> Vec<TradeLogEntry> {
    if !Path::new(path).exists() { return vec![]; }
    let content = match fs::read_to_string(path) { Ok(c) => c, Err(_) => return vec![] };
    if content.trim().is_empty() { return vec![] }
    serde_json::from_str::<Vec<TradeLogEntry>>(&content).unwrap_or_default()
}

pub fn save_legacy_optimization_result(path: &str, data: &serde_json::Value) -> std::io::Result<()> {
    fs::write(path, serde_json::to_string_pretty(data).unwrap_or_else(|_| "{}".to_string()))
}

pub fn load_legacy_optimization_result(path: &str) -> Option<serde_json::Value> {
    if !Path::new(path).exists() { return None; }
    let s = fs::read_to_string(path).ok()?;
    if s.trim().is_empty() { return None; }
    serde_json::from_str(&s).ok()
}


