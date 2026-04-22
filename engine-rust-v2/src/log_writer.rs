use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use serde::{Serialize, Deserialize};
use chrono::Utc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyLogRow {
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

pub fn append_strategy_log(path: &str, row: &StrategyLogRow) -> std::io::Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let existing = if Path::new(path).exists() {
        fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string())
    } else {
        "[]".to_string()
    };

    let mut rows: Vec<StrategyLogRow> = serde_json::from_str(&existing).unwrap_or_default();
    rows.push(row.clone());
    let json = serde_json::to_string_pretty(&rows).unwrap_or_else(|_| "[]".to_string());

    let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

pub fn make_strategy_row(
    symbol: &str,
    direction: &str,
    price: f64,
    score: i32,
    confidence: f64,
    ema20: f64,
    ema50: f64,
    rsi: f64,
    atr: f64,
    structure: &str,
    reversal_penalty: i32,
    structure_penalty: i32,
    reason: &str,
    action: &str,
    pnl: Option<f64>,
) -> StrategyLogRow {
    StrategyLogRow {
        timestamp: Utc::now().to_rfc3339(),
        symbol: symbol.to_string(),
        direction: direction.to_string(),
        price,
        score,
        confidence,
        ema20,
        ema50,
        rsi,
        atr,
        structure: structure.to_string(),
        reversal_penalty,
        structure_penalty,
        reason: reason.to_string(),
        action: action.to_string(),
        pnl,
    }
}
