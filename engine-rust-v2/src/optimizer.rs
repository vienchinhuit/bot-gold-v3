use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use chrono::{DateTime, Utc};

use rand::prelude::*;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    pub min_score: i32,
    pub min_confidence: f64,
    pub sideway_threshold: f64,
    pub min_trend_strength: f64,
    pub max_pullback_pips: f64,
    pub max_fomo_pips: f64,
    pub max_candle_mult: f64,
    pub sl_mult: f64,
    pub tp_mult: f64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            min_score: 5,
            min_confidence: 0.5,
            sideway_threshold: 0.30,
            min_trend_strength: 0.20,
            max_pullback_pips: 15.0,
            max_fomo_pips: 25.0,
            max_candle_mult: 1.5,
            sl_mult: 1.2,
            tp_mult: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub before: OptimizerConfig,
    pub after: OptimizerConfig,
    pub metrics: HashMap<String, f64>,
    pub rationale: Vec<String>,
    pub updated_at: String,
}

pub fn load_trade_logs(path: &str) -> Vec<TradeLogEntry> {
    if !Path::new(path).exists() {
        return vec![];
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    if content.trim().is_empty() {
        return vec![];
    }

    serde_json::from_str::<Vec<TradeLogEntry>>(&content).unwrap_or_default()
}

fn clamp<T: PartialOrd>(v: T, min: T, max: T) -> T {
    if v < min { min } else if v > max { max } else { v }
}

// Parse timestamp in logs; fall back to Utc::now() on failure
fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[derive(Debug, Clone)]
struct Enter {
    ts: DateTime<Utc>,
    direction: String,
    score: i32,
    confidence: f64,
}

#[derive(Debug, Clone)]
struct Close {
    ts: DateTime<Utc>,
    direction: String,
    pnl: f64,
}

// Pair ENTER records to subsequent CLOSE records (same direction) in chronological order
fn pair_enters_closes(enters: &[Enter], closes: &mut Vec<Close>) -> Vec<Option<(f64,i64)>> {
    // returns for each enter: Option<(pnl, duration_seconds)>
    let mut res: Vec<Option<(f64,i64)>> = vec![None; enters.len()];

    for (i, e) in enters.iter().enumerate() {
        if let Some(pos) = closes.iter().position(|c| c.direction == e.direction && c.ts > e.ts) {
            let c = closes.remove(pos);
            let dur = (c.ts - e.ts).num_seconds();
            res[i] = Some((c.pnl, dur));
        }
    }

    res
}

// Evaluate a candidate config: consider ENTER entries that meet thresholds, sum paired pnl
fn evaluate_config(
    current_enters: &[Enter],
    closes_orig: &[Close],
    cfg: &OptimizerConfig,
) -> (f64, usize, f64, f64, f64, f64) {
    // returns (total_pnl, matched_count, winrate, expectancy, avg_duration_minutes, max_drawdown)

    let filtered: Vec<Enter> = current_enters
        .iter()
        .cloned()
        .filter(|e| e.score >= cfg.min_score && e.confidence >= cfg.min_confidence)
        .collect();

    // copy closes
    let mut closes = closes_orig.to_vec();

    // pair
    let paired = pair_enters_closes(&filtered, &mut closes);

    let mut total_pnl = 0.0;
    let mut wins = 0usize;
    let mut matched = 0usize;
    let mut durations_sum_secs: i64 = 0;

    // Build equity curve to compute max drawdown
    let mut equity: Vec<f64> = Vec::new();
    let mut cum = 0.0;

    for opt in paired.iter() {
        if let Some((pnl, dur)) = opt {
            total_pnl += *pnl;
            matched += 1;
            if *pnl > 0.0 { wins += 1; }
            durations_sum_secs += *dur;
            cum += *pnl;
            equity.push(cum);
        }
    }

    let winrate = if matched > 0 { wins as f64 / matched as f64 } else { 0.0 };
    let expectancy = if matched > 0 { total_pnl / matched as f64 } else { 0.0 };
    let avg_duration_min = if matched > 0 { durations_sum_secs as f64 / matched as f64 / 60.0 } else { 0.0 };

    // compute max drawdown on equity series
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &v in equity.iter() {
        if v > peak { peak = v; }
        let dd = peak - v;
        if dd > max_dd { max_dd = dd; }
    }

    (total_pnl, matched, winrate, expectancy, avg_duration_min, max_dd)
}

pub fn optimize_from_logs(entries: &[TradeLogEntry], current: &OptimizerConfig) -> OptimizationResult {
    let mut rationale = Vec::new();
    let mut metrics = HashMap::new();

    if entries.is_empty() {
        rationale.push("No log entries found, keeping current config".to_string());
        return OptimizationResult { before: current.clone(), after: current.clone(), metrics, rationale, updated_at: Utc::now().to_rfc3339() };
    }

    // Build structured enters and closes
    let mut enters: Vec<Enter> = Vec::new();
    let mut closes: Vec<Close> = Vec::new();
    for e in entries.iter() {
        let ts = parse_ts(&e.timestamp);
        match e.action.as_str() {
            "ENTER" => enters.push(Enter { ts, direction: e.direction.clone(), score: e.score, confidence: e.confidence }),
            "CLOSE" => {
                let pnl = e.pnl.unwrap_or(0.0);
                closes.push(Close { ts, direction: e.direction.clone(), pnl });
            }
            _ => {}
        }
    }

    // baseline metrics with current config
    let (base_pnl, base_trades, base_winrate, base_expectancy, base_avg_duration, base_max_dd) = evaluate_config(&enters, &closes, current);

    metrics.insert("base_pnl".to_string(), base_pnl);
    metrics.insert("base_trades".to_string(), base_trades as f64);
    metrics.insert("base_winrate".to_string(), base_winrate);
    metrics.insert("base_expectancy".to_string(), base_expectancy);
    metrics.insert("base_avg_duration_min".to_string(), base_avg_duration);
    metrics.insert("base_max_drawdown".to_string(), base_max_dd);

    // Search for better config via random sampling around current
    let mut rng = StdRng::from_entropy();
    let mut best_cfg = current.clone();
    let mut best_obj = base_pnl + (base_trades as f64) * 0.05 + (base_winrate - 0.5) * 10.0 + base_expectancy * 5.0 - base_max_dd * 2.0;

    let samples = 400usize;
    for _ in 0..samples {
        let mut cand = current.clone();
        // perturb parameters with small random steps
        let ds: i32 = rng.gen_range(-2..=2);
        cand.min_score = clamp(cand.min_score + ds, 3, 9);
        let dc: f64 = rng.gen_range(-3..=3) as f64 * 0.03;
        cand.min_confidence = clamp((cand.min_confidence + dc * 1.0).round() , 0.30, 0.90);

        let dside: f64 = rng.gen_range(-2..=2) as f64 * 0.05;
        cand.sideway_threshold = clamp(cand.sideway_threshold + dside, 0.05, 1.0);
        let dtr: f64 = rng.gen_range(-2..=2) as f64 * 0.05;
        cand.min_trend_strength = clamp(cand.min_trend_strength + dtr, 0.05, 1.0);

        let dpb: f64 = rng.gen_range(-3..=3) as f64 * 2.0;
        cand.max_pullback_pips = clamp(cand.max_pullback_pips + dpb, 5.0, 40.0);
        let df: f64 = rng.gen_range(-3..=3) as f64 * 2.0;
        cand.max_fomo_pips = clamp(cand.max_fomo_pips + df, 8.0, 60.0);

        let dcm: f64 = rng.gen_range(-2..=2) as f64 * 0.1;
        cand.max_candle_mult = clamp(cand.max_candle_mult + dcm, 1.0, 3.0);

        let dsl: f64 = rng.gen_range(-2..=2) as f64 * 0.1;
        cand.sl_mult = clamp(cand.sl_mult + dsl, 0.5, 3.0);
        let dtp: f64 = rng.gen_range(-2..=2) as f64 * 0.1;
        cand.tp_mult = clamp(cand.tp_mult + dtp, 1.0, 4.0);

        // Evaluate
        let (pnl, trades_count, winrate, expectancy, avg_duration, max_dd) = evaluate_config(&enters, &closes, &cand);
        // objective includes pnl, trade count, winrate, expectancy; penalize drawdown and long durations
        let obj = pnl + (trades_count as f64) * 0.05 + (winrate - 0.5) * 10.0 + expectancy * 5.0 - max_dd * 2.0 - avg_duration * 0.02;

        if obj > best_obj {
            best_obj = obj;
            best_cfg = cand.clone();
        }
    }

    // Add rationale messages about adjustments
    if (best_cfg.min_score != current.min_score) || (best_cfg.min_confidence != current.min_confidence) {
        rationale.push(format!("Adjusted entry thresholds: min_score {}->{} min_conf {:.2}->{:.2}", current.min_score, best_cfg.min_score, current.min_confidence, best_cfg.min_confidence));
    }
    if (best_cfg.max_pullback_pips - current.max_pullback_pips).abs() > f64::EPSILON {
        rationale.push(format!("Adjusted pullback/fomo: pullback {:.1}->{:.1} fomo {:.1}->{:.1}", current.max_pullback_pips, best_cfg.max_pullback_pips, current.max_fomo_pips, best_cfg.max_fomo_pips));
    }
    if (best_cfg.sl_mult - current.sl_mult).abs() > f64::EPSILON || (best_cfg.tp_mult - current.tp_mult).abs() > f64::EPSILON {
        rationale.push(format!("Adjusted risk parameters: sl_mult {:.2}->{:.2} tp_mult {:.2}->{:.2}", current.sl_mult, best_cfg.sl_mult, current.tp_mult, best_cfg.tp_mult));
    }

    // compute final metrics for best_cfg
    let (final_pnl, final_trades, final_winrate, final_expectancy, final_avg_duration, final_max_dd) = evaluate_config(&enters, &closes, &best_cfg);
    metrics.insert("optimized_pnl".to_string(), final_pnl);
    metrics.insert("optimized_trades".to_string(), final_trades as f64);
    metrics.insert("optimized_winrate".to_string(), final_winrate);
    metrics.insert("optimized_expectancy".to_string(), final_expectancy);
    metrics.insert("optimized_avg_duration_min".to_string(), final_avg_duration);
    metrics.insert("optimized_max_drawdown".to_string(), final_max_dd);

    OptimizationResult {
        before: current.clone(),
        after: best_cfg,
        metrics,
        rationale,
        updated_at: Utc::now().to_rfc3339(),
    }
}

pub fn save_optimization_result(path: &str, result: &OptimizationResult) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(result).unwrap_or_else(|_| "{}".to_string());
    fs::write(path, json)
}

pub fn load_optimization_result(path: &str) -> Option<OptimizationResult> {
    if !Path::new(path).exists() {
        return None;
    }

    let content = fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }

    serde_json::from_str::<OptimizationResult>(&content).ok()
}
