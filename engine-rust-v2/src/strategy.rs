// ============================================================
// PRO SCALPING STRATEGY - Market Structure + Pullback + Confirmation
// ============================================================
// 
// This module implements a professional scalping strategy with:
// - Market Structure Detection (HH, HL, LH, LL)
// - Trend Confirmation (EMA 20/50)
// - RSI Confirmation (not overbought/oversold)
// - Volatility Filter (ATR-based)
// - Pullback Entry (wait for retracement to EMA)
// - Confidence Scoring System
//
// Author: Senior Quantitative Developer
// Target: XAUUSD / GOLD on M1 timeframe
// ============================================================

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// ============================================================
/// CONFIGURATION
/// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    // EMA Periods
    pub ema_fast: usize,
    pub ema_slow: usize,
    
    // RSI Period
    pub rsi_period: usize,
    
    // ATR Period for volatility
    pub atr_period: usize,
    
    // Risk Management
    pub sl_atr_mult: f64,      // SL = price +/- (ATR * sl_atr_mult)
    pub tp_atr_mult: f64,      // TP = price +/- (ATR * tp_atr_mult) (should be >= 1.5 * SL)
    
    // Volatility Filter
    pub max_candle_atr_mult: f64,  // Max candle range = ATR * this
    
    // Pullback Filter
    pub max_pullback_pips: f64,    // Max distance from EMA to enter
    
    // RSI Filters
    pub rsi_overbought: f64,
    pub rsi_oversold: f64,
    pub rsi_sell_confirm: f64,     // RSI must drop below this for SELL
    pub rsi_buy_confirm: f64,      // RSI must rise above this for BUY
    
    // Structure Lookback
    pub structure_lookback: usize,
    
    // Confidence Threshold
    pub min_confidence: f64,
    
    // Cooldown (in ticks)
    pub cooldown_ticks: usize,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            ema_fast: 20,
            ema_slow: 50,
            rsi_period: 14,
            atr_period: 14,
            
            // Risk: SL = 1.2 * ATR, TP = 2.0 * ATR (TP >= 1.5 * SL)
            sl_atr_mult: 1.2,
            tp_atr_mult: 2.0,
            
            // Don't enter on large candles (> 1.5 * ATR)
            max_candle_atr_mult: 1.5,
            
            // Pullback: within 15 pips of EMA
            max_pullback_pips: 15.0,
            
            // RSI filters
            rsi_overbought: 70.0,
            rsi_oversold: 30.0,
            rsi_sell_confirm: 50.0,   // RSI below 50 for SELL confirmation
            rsi_buy_confirm: 50.0,    // RSI above 50 for BUY confirmation
            
            // Structure detection lookback
            structure_lookback: 20,
            
            // Minimum confidence to enter (0.0 to 1.0)
            min_confidence: 0.6,
            
            // Cooldown between trades
            cooldown_ticks: 30,
        }
    }
}

/// ============================================================
/// MARKET STRUCTURE ENUM
/// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketStructure {
    HigherHigh,      // Bullish structure - can BUY
    HigherLow,       // Bullish continuation
    LowerHigh,       // Bearish structure - can SELL  
    LowerLow,        // Bearish continuation
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendDirection {
    Up,
    Down,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TradeDirection {
    Buy,
    Sell,
    None,
}

/// ============================================================
/// CANDLE DATA
/// ============================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Candle {
    pub time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: i64,
}

impl Candle {
    pub fn new(time: i64, open: f64, high: f64, low: f64, close: f64, volume: i64) -> Self {
        Self { time, open, high, low, close, volume }
    }
    
    pub fn range(&self) -> f64 {
        self.high - self.low
    }
    
    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }
    
    pub fn is_bearish(&self) -> bool {
        self.close < self.open
    }
    
    pub fn upper_wick(&self) -> f64 {
        if self.is_bullish() {
            self.high - self.close
        } else {
            self.high - self.open
        }
    }
    
    pub fn lower_wick(&self) -> f64 {
        if self.is_bullish() {
            self.open - self.low
        } else {
            self.close - self.low
        }
    }
}

/// ============================================================
/// STRATEGY STATE
/// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyState {
    // Price history
    pub prices: VecDeque<f64>,
    pub candles: VecDeque<Candle>,
    
    // High/Low history for structure
    pub highs: VecDeque<f64>,
    pub lows: VecDeque<f64>,
    
    // Current indicators (cached)
    pub ema_fast: Option<f64>,
    pub ema_slow: Option<f64>,
    pub rsi: Option<f64>,
    pub atr: Option<f64>,
    
    // Previous RSI for confirmation
    pub rsi_prev: Option<f64>,
    
    // Trend and structure
    pub trend: TrendDirection,
    pub structure: MarketStructure,
    
    // Last trade info
    pub last_trade_direction: TradeDirection,
    pub last_trade_time: i64,
    pub cooldown_counter: usize,
    
    // Confidence score
    pub last_confidence: f64,
    
    // Debug info
    pub debug_info: String,
}

impl StrategyState {
    pub fn new(max_history: usize) -> Self {
        Self {
            prices: VecDeque::with_capacity(max_history),
            candles: VecDeque::with_capacity(max_history),
            highs: VecDeque::with_capacity(max_history),
            lows: VecDeque::with_capacity(max_history),
            ema_fast: None,
            ema_slow: None,
            rsi: None,
            atr: None,
            rsi_prev: None,
            trend: TrendDirection::Flat,
            structure: MarketStructure::Neutral,
            last_trade_direction: TradeDirection::None,
            last_trade_time: 0,
            cooldown_counter: 0,
            last_confidence: 0.0,
            debug_info: String::new(),
        }
    }
    
    pub fn add_price(&mut self, price: f64, time: i64) {
        self.prices.push_back(price);
        
        // Maintain max history
        let max_hist = 100; // Enough for EMA50 + ATR14
        if self.prices.len() > max_hist {
            self.prices.pop_front();
        }
        
        // Update cooldown
        if self.cooldown_counter > 0 {
            self.cooldown_counter -= 1;
        }
    }
    
    pub fn add_candle(&mut self, candle: Candle) {
        self.candles.push_back(candle);
        self.highs.push_back(candle.high);
        self.lows.push_back(candle.low);
        
        let max_hist = 50;
        if self.candles.len() > max_hist {
            self.candles.pop_front();
        }
        if self.highs.len() > max_hist {
            self.highs.pop_front();
        }
        if self.lows.len() > max_hist {
            self.lows.pop_front();
        }
    }
}

/// ============================================================
/// INDICATOR CALCULATIONS
/// ============================================================

/// Calculate Simple Moving Average
#[inline]
pub fn sma(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period { return None; }
    let slice = &values[values.len() - period..];
    Some(slice.iter().sum::<f64>() / period as f64)
}

/// Calculate Exponential Moving Average (optimized)
#[inline]
pub fn ema(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period { return None; }
    let multiplier = 2.0 / (period as f64 + 1.0);
    
    // Start with SMA
    let mut ema_val = values[values.len() - period..].iter().sum::<f64>() / period as f64;
    
    // Calculate EMA forward
    for i in (values.len() - period + 1)..values.len() {
        ema_val = (values[i] - ema_val) * multiplier + ema_val;
    }
    Some(ema_val)
}

/// Calculate RSI (Relative Strength Index)
#[inline]
pub fn rsi(closes: &[f64], period: usize) -> Option<f64> {
    if closes.len() < period + 1 { return None; }
    
    let mut gains = 0.0;
    let mut losses = 0.0;
    
    for i in (closes.len() - period)..closes.len() {
        let delta = closes[i] - closes[i - 1];
        if delta > 0.0 {
            gains += delta;
        } else {
            losses -= delta;
        }
    }
    
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    
    if avg_loss == 0.0 { return Some(100.0); }
    
    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

/// Calculate ATR (Average True Range)
#[inline]
pub fn atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Option<f64> {
    if highs.len() < period + 1 || lows.len() < period + 1 || closes.len() < period + 1 {
        return None;
    }
    
    let mut tr_sum = 0.0;
    for i in 1..=period {
        let high_low = highs[i] - lows[i];
        let high_close = (highs[i] - closes[i-1]).abs();
        let low_close = (lows[i] - closes[i-1]).abs();
        let tr = high_low.max(high_close).max(low_close);
        tr_sum += tr;
    }
    Some(tr_sum / period as f64)
}

/// ============================================================
/// STRATEGY LOGIC
/// ============================================================

/// Detect market trend using EMA
#[inline]
pub fn detect_trend(ema_fast: f64, ema_slow: f64, price: f64) -> TrendDirection {
    if ema_fast > ema_slow && price > ema_fast {
        TrendDirection::Up
    } else if ema_fast < ema_slow && price < ema_fast {
        TrendDirection::Down
    } else {
        TrendDirection::Flat
    }
}

/// Detect market structure: HH, HL, LH, LL
/// 
/// Rules:
/// - Higher High (HH): Current high > previous high > ...
/// - Higher Low (HL): Current low > previous low > ... (bullish continuation)
/// - Lower High (LH): Current high < previous high < ... (bearish)
/// - Lower Low (LL): Current low < previous low < ...
#[inline]
pub fn detect_structure(highs: &[f64], lows: &[f64], lookback: usize) -> MarketStructure {
    if highs.len() < lookback || lows.len() < lookback {
        return MarketStructure::Neutral;
    }
    
    // Get recent highs and lows
    let recent_highs = &highs[highs.len() - lookback..];
    let recent_lows = &lows[lows.len() - lookback..];
    
    // Find local peaks
    let mut hh_idx = 0;
    let mut hh_val = f64::NEG_INFINITY;
    let mut hl_idx = 0;
    let mut hl_val = f64::NEG_INFINITY;
    
    let mut lh_idx = 0;
    let mut lh_val = f64::INFINITY;
    let mut ll_idx = 0;
    let mut ll_val = f64::INFINITY;
    
    for (i, &h) in recent_highs.iter().enumerate() {
        if h > hh_val {
            hh_val = h;
            hh_idx = i;
        }
        if h < lh_val {
            lh_val = h;
            lh_idx = i;
        }
    }
    
    for (i, &l) in recent_lows.iter().enumerate() {
        if l > hl_val {
            hl_val = l;
            hl_idx = i;
        }
        if l < ll_val {
            ll_val = l;
            ll_idx = i;
        }
    }
    
    // Determine structure based on recent patterns
    // For HH/HL: need ascending peaks and troughs
    let has_hh = hh_val > recent_highs.iter().take(hh_idx).fold(f64::NEG_INFINITY, |a, b| f64::max(a, *b));
    let has_hl = hl_val > recent_lows.iter().take(hl_idx).fold(f64::NEG_INFINITY, |a, b| f64::max(a, *b));
    let has_lh = lh_val < recent_highs.iter().take(lh_idx).fold(f64::INFINITY, |a, b| f64::min(a, *b));
    let has_ll = ll_val < recent_lows.iter().take(ll_idx).fold(f64::INFINITY, |a, b| f64::min(a, *b));
    
    if has_hh && has_hl {
        MarketStructure::HigherHigh // Can buy, but wait for pullback
    } else if has_lh && has_ll {
        MarketStructure::LowerHigh // Can sell, but wait for pullback
    } else if has_hl {
        MarketStructure::HigherLow // Bullish continuation
    } else if has_lh {
        MarketStructure::LowerLow // Bearish continuation
    } else {
        MarketStructure::Neutral
    }
}

/// Check if price is in pullback to EMA
#[inline]
pub fn is_in_pullback(price: f64, ema: f64, max_pips: f64, pip_value: f64) -> bool {
    let distance_pips = (price - ema).abs() / pip_value;
    distance_pips <= max_pips
}

/// Check RSI confirmation (not just overbought/oversold)
/// 
/// Better logic:
/// - SELL when RSI drops from >60 to <50 (momentum shifting down)
/// - BUY when RSI rises from <40 to >50 (momentum shifting up)
#[inline]
pub fn rsi_confirmation(
    rsi_current: f64,
    rsi_previous: Option<f64>,
    direction: TradeDirection,
    config: &StrategyConfig,
) -> bool {
    let rsi_prev = rsi_previous.unwrap_or(rsi_current);
    
    match direction {
        TradeDirection::Sell => {
            // SELL: RSI should be below confirm level AND dropping
            rsi_current < config.rsi_sell_confirm && rsi_current < rsi_prev
        }
        TradeDirection::Buy => {
            // BUY: RSI should be above confirm level AND rising
            rsi_current > config.rsi_buy_confirm && rsi_current > rsi_prev
        }
        TradeDirection::None => false,
    }
}

/// Check volatility filter
/// 
/// Avoid entering when:
/// - Candle range > max_candle_atr_mult * ATR
/// - Large wicks (possible stop hunt)
#[inline]
pub fn check_volatility(candle: &Candle, atr: f64, max_mult: f64) -> (bool, String) {
    let candle_range = candle.range();
    let max_allowed = atr * max_mult;
    
    if candle_range > max_allowed {
        return (false, format!(
            "VOLATILITY: candle range {:.2} > max {:.2} ({} * ATR)",
            candle_range, max_allowed, max_mult
        ));
    }
    
    // Check for large wicks (possible liquidity sweep)
    let upper_wick = candle.upper_wick();
    let lower_wick = candle.lower_wick();
    let wick_threshold = candle_range * 0.4; // Wick > 40% of range is suspicious
    
    if upper_wick > wick_threshold || lower_wick > wick_threshold {
        return (false, format!(
            "VOLATILITY: large wick detected (upper: {:.2}, lower: {:.2}, range: {:.2})",
            upper_wick, lower_wick, candle_range
        ));
    }
    
    (true, "OK".to_string())
}

/// ============================================================
/// SCORING SYSTEM
/// ============================================================

#[derive(Debug, Clone)]
pub struct TradeSignal {
    pub direction: TradeDirection,
    pub confidence: f64,           // 0.0 to 1.0
    pub sl_price: Option<f64>,
    pub tp_price: Option<f64>,
    pub reason: String,
    pub score_breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Default)]
pub struct ScoreBreakdown {
    pub trend_score: f64,          // 0-0.2
    pub structure_score: f64,      // 0-0.2
    pub pullback_score: f64,       // 0-0.2
    pub rsi_score: f64,            // 0-0.2
    pub volatility_score: f64,     // 0-0.2
}

impl TradeSignal {
    pub fn none() -> Self {
        Self {
            direction: TradeDirection::None,
            confidence: 0.0,
            sl_price: None,
            tp_price: None,
            reason: String::new(),
            score_breakdown: ScoreBreakdown::default(),
        }
    }
}

/// Main entry decision function with scoring
/// 
/// Returns a TradeSignal with confidence score and reasoning
#[inline]
pub fn should_enter_trade(
    state: &mut StrategyState,
    price: f64,
    bid: f64,
    ask: f64,
    candle: &Candle,
    config: &StrategyConfig,
) -> TradeSignal {
    // Get current indicator values
    let ema_fast = match state.ema_fast {
        Some(v) => v,
        None => return TradeSignal::none(),
    };
    
    let ema_slow = match state.ema_slow {
        Some(v) => v,
        None => return TradeSignal::none(),
    };
    
    let rsi = match state.rsi {
        Some(v) => v,
        None => return TradeSignal::none(),
    };
    
    let atr = match state.atr {
        Some(v) => v,
        None => return TradeSignal::none(),
    };
    
    // Pip value for XAUUSD
    let pip_value = 0.01;
    
    // ============================================================
    // 1. TREND CHECK (max 0.2 points)
    // ============================================================
    let trend = detect_trend(ema_fast, ema_slow, price);
    let (trend_ok, trend_score, trend_reason) = match trend {
        TrendDirection::Up => (true, 0.2, "UPTREND confirmed".to_string()),
        TrendDirection::Down => (true, 0.2, "DOWNTREND confirmed".to_string()),
        TrendDirection::Flat => (false, 0.0, "FLAT trend - NO TRADE".to_string()),
    };
    
    if !trend_ok {
        return TradeSignal::none();
    }
    
    // ============================================================
    // 2. STRUCTURE CHECK (max 0.2 points)
    // ============================================================
    let structure = detect_structure(
        &state.highs.make_contiguous(),
        &state.lows.make_contiguous(),
        config.structure_lookback,
    );
    
    let (structure_ok, structure_score, structure_reason) = match (trend, structure) {
        // Uptrend + HH/HL = can BUY
        (TrendDirection::Up, MarketStructure::HigherHigh) => 
            (true, 0.2, "HH detected - bullish structure".to_string()),
        (TrendDirection::Up, MarketStructure::HigherLow) => 
            (true, 0.15, "HL detected - bullish continuation".to_string()),
        // Downtrend + LH/LL = can SELL
        (TrendDirection::Down, MarketStructure::LowerHigh) => 
            (true, 0.2, "LH detected - bearish structure".to_string()),
        (TrendDirection::Down, MarketStructure::LowerLow) => 
            (true, 0.15, "LL detected - bearish continuation".to_string()),
        // Opposite structure = NO TRADE
        (TrendDirection::Up, MarketStructure::LowerHigh) =>
            (false, 0.0, "LH in uptrend - NO SELL".to_string()),
        (TrendDirection::Down, MarketStructure::HigherHigh) =>
            (false, 0.0, "HH in downtrend - NO BUY".to_string()),
        // Neutral structure
        _ => (false, 0.0, "Neutral structure".to_string()),
    };
    
    if !structure_ok {
        let mut signal = TradeSignal::none();
        signal.reason = structure_reason;
        return signal;
    }
    
    // ============================================================
    // 3. PULLBACK CHECK (max 0.2 points)
    // ============================================================
    let in_pullback = is_in_pullback(price, ema_fast, config.max_pullback_pips, pip_value);
    let pullback_score = if in_pullback { 0.2 } else { 0.0 };
    
    if !in_pullback {
        let mut signal = TradeSignal::none();
        signal.reason = format!(
            "NO PULLBACK: price {:.2} is {:.1} pips from EMA20 {:.2}",
            price, (price - ema_fast).abs() / pip_value, ema_fast
        );
        return signal;
    }
    
    // ============================================================
    // 4. RSI CHECK (max 0.2 points)
    // ============================================================
    // Don't buy at overbought, don't sell at oversold
    let rsi_overbought = rsi >= config.rsi_overbought;
    let rsi_oversold = rsi <= config.rsi_oversold;
    
    if rsi_overbought || rsi_oversold {
        let mut signal = TradeSignal::none();
        signal.reason = format!("RSI at {:.1} - overbought/oversold zone", rsi);
        return signal;
    }
    
    // RSI confirmation: direction-specific
    let rsi_confirm_buy = rsi_confirmation(rsi, state.rsi_prev, TradeDirection::Buy, config);
    let rsi_confirm_sell = rsi_confirmation(rsi, state.rsi_prev, TradeDirection::Sell, config);
    
    let rsi_score = match trend {
        TrendDirection::Up if rsi_confirm_buy => 0.2,
        TrendDirection::Down if rsi_confirm_sell => 0.2,
        _ => 0.1, // Partial credit for being in correct zone
    };
    
    // ============================================================
    // 5. VOLATILITY CHECK (max 0.2 points)
    // ============================================================
    let (volatility_ok, vol_reason) = check_volatility(candle, atr, config.max_candle_atr_mult);
    let volatility_score = if volatility_ok { 0.2 } else { 0.0 };
    
    if !volatility_ok {
        let mut signal = TradeSignal::none();
        signal.reason = vol_reason;
        return signal;
    }
    
    // ============================================================
    // CALCULATE TOTAL SCORE
    // ============================================================
    let total_score = trend_score + structure_score + pullback_score + rsi_score + volatility_score;
    let confidence = total_score; // Already normalized to 0-1.0
    
    // Check minimum confidence
    if confidence < config.min_confidence {
        let mut signal = TradeSignal::none();
        signal.confidence = confidence;
        signal.reason = format!(
            "LOW CONFIDENCE: {:.2} < {:.2} | {}",
            confidence, config.min_confidence,
            format!(
                "trend={:.1}, struct={:.1}, pullback={:.1}, rsi={:.1}, vol={:.1}",
                trend_score, structure_score, pullback_score, rsi_score, volatility_score
            )
        );
        return signal;
    }
    
    // ============================================================
    // DETERMINE DIRECTION AND SET SL/TP
    // ============================================================
    let (direction, sl_price, tp_price, direction_reason) = match trend {
        TrendDirection::Up => {
            let entry = ask; // Buy at ask
            let sl = entry - (atr * config.sl_atr_mult);
            let tp = entry + (atr * config.tp_atr_mult);
            (
                TradeDirection::Buy,
                Some(sl),
                Some(tp),
                "BUY: Uptrend + pullback + RSI rising".to_string(),
            )
        }
        TrendDirection::Down => {
            let entry = bid; // Sell at bid
            let sl = entry + (atr * config.sl_atr_mult);
            let tp = entry - (atr * config.tp_atr_mult);
            (
                TradeDirection::Sell,
                Some(sl),
                Some(tp),
                "SELL: Downtrend + pullback + RSI falling".to_string(),
            )
        }
        TrendDirection::Flat => {
            return TradeSignal::none();
        }
    };
    
    // ============================================================
    // COOLDOWN CHECK
    // ============================================================
    if state.cooldown_counter > 0 {
        let mut signal = TradeSignal::none();
        signal.reason = format!("COOLDOWN: {} ticks remaining", state.cooldown_counter);
        return signal;
    }
    
    // ============================================================
    // RETURN SIGNAL
    // ============================================================
    TradeSignal {
        direction,
        confidence,
        sl_price,
        tp_price,
        reason: direction_reason,
        score_breakdown: ScoreBreakdown {
            trend_score,
            structure_score,
            pullback_score,
            rsi_score,
            volatility_score,
        },
    }
}

/// ============================================================
/// HELPER FUNCTIONS
/// ============================================================

/// Update all indicators in state
pub fn update_indicators(state: &mut StrategyConfig, state_data: &mut StrategyState) {
    let prices: Vec<f64> = state_data.prices.iter().copied().collect();
    let highs: Vec<f64> = state_data.highs.iter().copied().collect();
    let lows: Vec<f64> = state_data.lows.iter().copied().collect();
    let closes: Vec<f64> = state_data.prices.iter().copied().collect();
    
    // Update EMA
    state_data.ema_fast = ema(&prices, state.ema_fast);
    state_data.ema_slow = ema(&prices, state.ema_slow);
    
    // Update RSI
    state_data.rsi_prev = state_data.rsi;
    state_data.rsi = rsi(&closes, state.rsi_period);
    
    // Update ATR
    state_data.atr = atr(&highs, &lows, &closes, state.atr_period);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ema_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let result = ema(&values, 3);
        assert!(result.is_some());
    }
    
    #[test]
    fn test_rsi_calculation() {
        let values = vec![44.0, 44.34, 44.09, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03, 45.61, 46.28];
        let result = rsi(&values, 14);
        assert!(result.is_some());
    }
}
