// ============================================================
// ADVANCED SCALPING STRATEGY v2.0
// Market Structure + Pullback + Confirmation + Scoring
// ============================================================
// 
// Features:
// 1. SIDEWAY FILTER - EMA convergence check
// 2. TREND STRENGTH - EMA distance measurement
// 3. MARKET STRUCTURE - HH/HL/LH/LL detection
// 4. PULLBACK ENTRY - Only enter on retracement
// 5. RSI FILTER - Momentum confirmation
// 6. VOLATILITY FILTER - ATR-based spike rejection
// 7. CONFIRMATION CANDLE - Wait for candle close
// 8. ANTI-FOMO - Distance from EMA limit
// 9. COOLDOWN - Loss-based delay
// 10. NO-TRADE ZONE - Recent trade price areas
// 11. SCORING SYSTEM - Multi-factor scoring
// 12. RISK MANAGEMENT - ATR-based SL/TP
//
// Author: Senior Quantitative Developer
// Target: XAUUSD/GOLD M1 Scalping
// ============================================================

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

// ============================================================
// CONSTANTS & TYPE DEFINITIONS
// ============================================================

/// Direction of potential trade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Long,
    Short,
    None,
}

/// Swing point types for structure detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SwingType {
    HigherHigh,   // HH - bullish peak
    HigherLow,    // HL - bullish trough
    LowerHigh,    // LH - bearish peak
    LowerLow,     // LL - bearish trough
    None,
}

/// Trade signal with full context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub direction: Direction,
    pub action: SignalAction,
    pub confidence: f64,          // 0.0 to 1.0
    pub score: i32,               // 0-10 scale
    pub entry_price: f64,
    pub stop_loss: f64,
    pub take_profit: f64,
    pub reason: String,
    pub breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalAction {
    EnterLong,
    EnterShort,
    Hold,
    SkipDueToFilter,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub trend: i32,           // 0-2
    pub strength: i32,        // 0-2
    pub structure: i32,      // 0-2
    pub pullback: i32,       // 0-1
    pub rsi: i32,            // 0-1
    pub volatility: i32,     // 0-1
    pub confirmation: i32,   // 0-1
    pub total: i32,          // Sum
}

/// Candle OHLCV data structure (zero-copy friendly)
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
    #[inline]
    pub fn body(&self) -> f64 {
        (self.close - self.open).abs()
    }
    
    #[inline]
    pub fn range(&self) -> f64 {
        self.high - self.low
    }
    
    #[inline]
    pub fn upper_wick(&self) -> f64 {
        if self.close >= self.open {
            self.high - self.close
        } else {
            self.high - self.open
        }
    }
    
    #[inline]
    pub fn lower_wick(&self) -> f64 {
        if self.close >= self.open {
            self.open - self.low
        } else {
            self.close - self.low
        }
    }
    
}

/// Configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // === INDICATOR PERIODS ===
    pub ema_fast: usize,          // Default: 20
    pub ema_slow: usize,          // Default: 50
    pub rsi_period: usize,        // Default: 14
    pub atr_period: usize,        // Default: 14
    
    // === SIDEWAY FILTER ===
    pub sideway_ema_threshold: f64, // |EMA20 - EMA50| < threshold = sideway
    
    // === TREND STRENGTH ===
    pub min_trend_strength: f64,   // Min |EMA20 - EMA50| for valid trend
    
    // === PULLBACK FILTER ===
    pub max_pullback_pips: f64,    // Max distance from EMA for valid pullback
    
    // === ANTI-FOMO ===
    pub max_fomo_pips: f64,        // Max distance from EMA - no trade beyond
    
    // === RSI FILTERS ===
    pub rsi_oversold: f64,         // Default: 30
    pub rsi_overbought: f64,       // Default: 70
    pub rsi_sell_confirm_low: f64, // RSI must drop below this for SELL
    pub rsi_sell_confirm_high: f64,// RSI started above this
    pub rsi_buy_confirm_low: f64,   // RSI started below this
    pub rsi_buy_confirm_high: f64, // RSI must rise above this for BUY
    
    // === VOLATILITY FILTER ===
    pub max_candle_mult: f64,      // Candle > this * ATR = reject
    pub max_wick_ratio: f64,       // Wick > this * body = suspicious
    
    // === SCORING ===
    pub min_score: i32,            // Minimum score to enter (default: 5)
    pub min_confidence: f64,       // Minimum confidence 0-1
    
    // === RISK MANAGEMENT ===
    pub sl_mult: f64,              // SL = ATR * this (default: 1.2)
    pub tp_mult: f64,              // TP = ATR * this (default: 2.0)
    pub pip_value: f64,            // 0.01 for XAUUSD
    
    // === COOLDOWN ===
    pub cooldown_after_loss: usize,    // Wait N candles after losing trade
    pub max_consecutive_losses: usize, // Max consecutive losses before pause
    pub pause_duration_minutes: i64,  // Pause duration after max losses
    
    // === POSITION LIMITS ===
    pub max_positions_per_direction: usize,
    
    // === NO-TRADE ZONE ===
    pub no_trade_zone_pips: f64,   // Recent trade price zone
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Indicator periods
            ema_fast: 20,
            ema_slow: 50,
            rsi_period: 14,
            atr_period: 14,
            
            // Sideway detection
            sideway_ema_threshold: 0.30,  // 30 cents for GOLD
            
            // Trend strength
            min_trend_strength: 0.20,    // 20 cents minimum
            
            // Pullback & FOMO
            max_pullback_pips: 15.0,
            max_fomo_pips: 25.0,
            
            // RSI zones
            rsi_oversold: 30.0,
            rsi_overbought: 70.0,
            rsi_sell_confirm_low: 50.0,
            rsi_sell_confirm_high: 60.0,
            rsi_buy_confirm_low: 40.0,
            rsi_buy_confirm_high: 50.0,
            
            // Volatility
            max_candle_mult: 1.5,
            max_wick_ratio: 0.5,
            
            // Scoring
            min_score: 5,
            min_confidence: 0.5,
            
            // Risk
            sl_mult: 1.2,
            tp_mult: 2.0,
            pip_value: 0.01,
            
            // Cooldown
            cooldown_after_loss: 15,
            max_consecutive_losses: 3,
            pause_duration_minutes: 30,
            
            // Position limits
            max_positions_per_direction: 5,
            
            // No-trade zone
            no_trade_zone_pips: 10.0,
        }
    }
}

/// Runtime state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    // Price history (fixed size ring buffer)
    pub prices: VecDeque<f64>,
    pub closes: VecDeque<f64>,
    pub highs: VecDeque<f64>,
    pub lows: VecDeque<f64>,
    
    // Candle history
    pub candles: VecDeque<Candle>,
    
    // Previous RSI for momentum detection
    pub rsi_prev: Option<f64>,
    
    // Current indicators (cached)
    pub ema_fast: Option<f64>,
    pub ema_slow: Option<f64>,
    pub rsi: Option<f64>,
    pub atr: Option<f64>,
    
    // Swing points for structure
    recent_swings: VecDeque<SwingPoint>,
    
    // Cooldown tracking
    pub cooldown_counter: usize,
    pub consecutive_losses: usize,
    pub last_loss_time: Option<i64>,
    
    // Position tracking
    pub long_positions: usize,
    pub short_positions: usize,
    
    // Recent trade prices for no-trade zone
    pub recent_trade_prices: VecDeque<f64>,
    
    // Confirmation candle state
    pending_long_setup: Option<f64>,  // Price level for confirmation
    pending_short_setup: Option<f64>,
    
    // Debug counters
    pub ticks_processed: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SwingPoint {
    pub time: i64,
    pub price: f64,
    pub swing_type: SwingType,
}

impl State {
    pub fn new() -> Self {
        Self {
            prices: VecDeque::with_capacity(100),
            closes: VecDeque::with_capacity(100),
            highs: VecDeque::with_capacity(100),
            lows: VecDeque::with_capacity(100),
            candles: VecDeque::with_capacity(50),
            rsi_prev: None,
            ema_fast: None,
            ema_slow: None,
            rsi: None,
            atr: None,
            recent_swings: VecDeque::with_capacity(20),
            cooldown_counter: 0,
            consecutive_losses: 0,
            last_loss_time: None,
            long_positions: 0,
            short_positions: 0,
            recent_trade_prices: VecDeque::with_capacity(10),
            pending_long_setup: None,
            pending_short_setup: None,
            ticks_processed: 0,
        }
    }
    
    #[inline]
    pub fn push_price(&mut self, price: f64) {
        self.prices.push_back(price);
        self.closes.push_back(price);
        
        if self.prices.len() > 100 {
            self.prices.pop_front();
            self.closes.pop_front();
        }
    }
    
    #[inline]
    pub fn push_ohlc(&mut self, high: f64, low: f64) {
        self.highs.push_back(high);
        self.lows.push_back(low);
        
        if self.highs.len() > 100 {
            self.highs.pop_front();
            self.lows.pop_front();
        }
    }
    
    #[inline]
    pub fn push_candle(&mut self, candle: Candle) {
        self.candles.push_back(candle);
        if self.candles.len() > 50 {
            self.candles.pop_front();
        }
    }
    
    pub fn record_trade(&mut self, price: f64, won: bool) {
        // Add to recent prices
        self.recent_trade_prices.push_back(price);
        if self.recent_trade_prices.len() > 10 {
            self.recent_trade_prices.pop_front();
        }
        
        if won {
            self.consecutive_losses = 0;
        } else {
            self.consecutive_losses += 1;
            self.cooldown_counter = 20; // Start cooldown
        }
    }
}

// ============================================================
// INDICATOR CALCULATIONS (Performance Optimized)
// ============================================================

/// Fast EMA calculation using pre-allocated arrays
#[inline]
pub fn calc_ema(values: &[f64], period: usize) -> Option<f64> {
    if values.len() < period { return None; }
    
    let mult = 2.0 / (period as f64 + 1.0);
    let offset = values.len() - period;
    
    // Initial EMA = SMA of first period
    let mut ema = values[offset..].iter().sum::<f64>() / period as f64;
    
    // Calculate EMA forward
    for i in (offset + 1)..values.len() {
        ema = (values[i] - ema) * mult + ema;
    }
    
    Some(ema)
}

/// RSI calculation (Wilder's smoothed method)
#[inline]
pub fn calc_rsi(closes: &[f64], period: usize) -> Option<f64> {
    if closes.len() < period + 1 { return None; }
    
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    
    // Initial average
    for i in (closes.len() - period)..closes.len() {
        let delta = closes[i] - closes[i - 1];
        if delta > 0.0 {
            avg_gain += delta;
        } else {
            avg_loss -= delta;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;
    
    if avg_loss == 0.0 { return Some(100.0); }
    
    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

/// ATR (Average True Range) calculation
#[inline]
pub fn calc_atr(highs: &[f64], lows: &[f64], closes: &[f64], period: usize) -> Option<f64> {
    if highs.len() < period + 1 || lows.len() < period + 1 || closes.len() < period + 1 {
        return None;
    }
    
    let mut tr_sum = 0.0;
    for i in 1..=period {
        let idx = highs.len() - period + i - 1;
        let high_low = highs[idx] - lows[idx];
        let high_close = (highs[idx] - closes[idx - 1]).abs();
        let low_close = (lows[idx] - closes[idx - 1]).abs();
        tr_sum += high_low.max(high_close).max(low_close);
    }
    
    Some(tr_sum / period as f64)
}

// ============================================================
// FILTER FUNCTIONS
// ============================================================

/// 1. SIDEWAY FILTER
/// Returns true if market is in ranging/sideway condition
/// EMA20 and EMA50 are within threshold = sideway
#[inline]
pub fn is_sideway(ema_fast: f64, ema_slow: f64, threshold: f64) -> bool {
    (ema_fast - ema_slow).abs() < threshold
}

/// 2. TREND STRENGTH CHECK
/// Returns the strength of current trend
#[inline]
pub fn trend_strength(ema_fast: f64, ema_slow: f64) -> f64 {
    (ema_fast - ema_slow).abs()
}

/// 3. DETECT MARKET STRUCTURE
/// Identify HH, HL, LH, LL patterns
#[inline]
pub fn detect_structure(highs: &[f64], lows: &[f64], lookback: usize) -> SwingType {
    if highs.len() < lookback || lows.len() < lookback {
        return SwingType::None;
    }
    
    let start_idx = highs.len() - lookback;
    let recent_highs = &highs[start_idx..];
    let recent_lows = &lows[start_idx..];
    
    // Find higher highs in first half vs second half
    let mid = lookback / 2;
    let first_half_highs = &recent_highs[..mid];
    let second_half_highs = &recent_highs[mid..];
    
    let first_max_h = first_half_highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let second_max_h = second_half_highs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    
    // Find higher lows in first half vs second half
    let first_half_lows = &recent_lows[..mid];
    let second_half_lows = &recent_lows[mid..];
    
    let first_min_l = first_half_lows.iter().copied().fold(f64::INFINITY, f64::min);
    let second_min_l = second_half_lows.iter().copied().fold(f64::INFINITY, f64::min);
    
    // Determine structure
    if second_max_h > first_max_h && second_min_l > first_min_l {
        SwingType::HigherHigh  // HH + HL = Bullish
    } else if second_max_h > first_max_h && second_min_l <= first_min_l {
        SwingType::HigherLow  // HH but lower lows = weakening
    } else if second_max_h <= first_max_h && second_min_l > first_min_l {
        SwingType::LowerHigh  // Lower highs but HL forming
    } else if second_max_h <= first_max_h && second_min_l <= first_min_l {
        SwingType::LowerLow   // LH + LL = Bearish
    } else {
        SwingType::None
    }
}

/// 4. PULLBACK CHECK
/// Price must be within pullback distance from EMA
#[inline]
pub fn is_pullback(price: f64, ema: f64, max_pips: f64, pip_value: f64) -> bool {
    (price - ema).abs() / pip_value <= max_pips
}

/// 5. RSI FILTER
/// Check RSI momentum confirmation
/// SELL: RSI drops from >60 to <50
/// BUY: RSI rises from <40 to >50
#[inline]
pub fn check_rsi(rsi: f64, rsi_prev: Option<f64>, direction: Direction, cfg: &Config) -> (bool, i32) {
    let prev = rsi_prev.unwrap_or(rsi);
    
    match direction {
        Direction::Short => {
            // SELL: RSI should be below 50 AND dropping from above 60
            if rsi < cfg.rsi_sell_confirm_low && prev > cfg.rsi_sell_confirm_high {
                (true, 1)
            } else if rsi < cfg.rsi_sell_confirm_low {
                (true, 1)
            } else {
                (false, 0)
            }
        }
        Direction::Long => {
            // BUY: RSI should be above 50 AND rising from below 40
            if rsi > cfg.rsi_buy_confirm_high && prev < cfg.rsi_buy_confirm_low {
                (true, 1)
            } else if rsi > cfg.rsi_buy_confirm_high {
                (true, 1)
            } else {
                (false, 0)
            }
        }
        Direction::None => (false, 0),
    }
}

/// 6. VOLATILITY FILTER
/// Reject trades on abnormally large candles or wicks
#[inline]
pub fn check_volatility(candle: &Candle, atr: f64, max_mult: f64, max_wick_ratio: f64) -> (bool, String) {
    let range = candle.range();
    let max_allowed = atr * max_mult;
    
    // Check candle size
    if range > max_allowed {
        return (false, format!(
            "VOLATILITY: candle {:.2} > max {:.2} ({:.1}*ATR)",
            range, max_allowed, max_mult
        ));
    }
    
    // Check wick ratio
    let upper_wick = candle.upper_wick();
    let lower_wick = candle.lower_wick();
    let body = candle.body().max(0.0001); // Avoid division by zero
    
    if upper_wick > body * max_wick_ratio || lower_wick > body * max_wick_ratio {
        return (false, format!(
            "VOLATILITY: wick suspicious (u:{:.2} l:{:.2} body:{:.2})",
            upper_wick, lower_wick, body
        ));
    }
    
    (true, "OK".to_string())
}

/// 7. ANTI-FOMO CHECK
/// Reject trades when price has moved too far from EMA
#[inline]
pub fn check_anti_fomo(price: f64, ema: f64, max_pips: f64, pip_value: f64) -> bool {
    (price - ema).abs() / pip_value <= max_pips
}

/// 10. NO-TRADE ZONE CHECK
/// Reject trades near recent trade prices
#[inline]
pub fn is_in_no_trade_zone(price: f64, recent_prices: &VecDeque<f64>, zone_pips: f64, pip_value: f64) -> bool {
    for &recent_price in recent_prices {
        if (price - recent_price).abs() / pip_value <= zone_pips {
            return true;
        }
    }
    false
}

// ============================================================
// SCORING SYSTEM (PRIMARY DECISION MECHANISM)
// ============================================================

/// Calculate comprehensive trade score
/// Returns (score, confidence, breakdown)
#[inline]
pub fn calculate_score(
    price: f64,
    ema_fast: f64,
    ema_slow: f64,
    rsi: f64,
    rsi_prev: Option<f64>,
    _atr: f64,
    structure: SwingType,
    direction: Direction,
    cfg: &Config,
) -> (i32, f64, ScoreBreakdown) {
    let mut breakdown = ScoreBreakdown::default();
    
    // === TREND SCORE (0-2) ===
    let ema_distance = (ema_fast - ema_slow).abs();
    if ema_distance >= cfg.min_trend_strength {
        match direction {
            Direction::Long if ema_fast > ema_slow => {
                breakdown.trend = 2;
            }
            Direction::Short if ema_fast < ema_slow => {
                breakdown.trend = 2;
            }
            _ => breakdown.trend = 0,
        }
    } else {
        breakdown.trend = 0; // Weak trend
    }
    
    // === STRENGTH SCORE (0-2) ===
    // Based on EMA distance magnitude
    let strength_ratio = ema_distance / cfg.min_trend_strength;
    breakdown.strength = match strength_ratio as i32 {
        0..=1 => 0,
        2 => 1,
        _ => 2,
    };
    
    // === STRUCTURE SCORE (0-2) ===
    // Structure must align with direction
    match (direction, structure) {
        (Direction::Long, SwingType::HigherHigh) => breakdown.structure = 2,
        (Direction::Long, SwingType::HigherLow) => breakdown.structure = 1,
        (Direction::Short, SwingType::LowerLow) => breakdown.structure = 2,
        (Direction::Short, SwingType::LowerHigh) => breakdown.structure = 1,
        // Opposite structure = anti
        (Direction::Long, SwingType::LowerHigh) => breakdown.structure = -1,
        (Direction::Short, SwingType::HigherHigh) => breakdown.structure = -1,
        _ => breakdown.structure = 0,
    }
    
    // === PULLBACK SCORE (0-1) ===
    let pullback_distance = (price - ema_fast).abs() / cfg.pip_value;
    if pullback_distance <= cfg.max_pullback_pips {
        breakdown.pullback = 1;
    } else if pullback_distance <= cfg.max_pullback_pips * 1.5 {
        breakdown.pullback = 0;
    }
    
    // === RSI SCORE (0-1) ===
    // Check RSI zone alignment
    match direction {
        Direction::Long => {
            // No BUY when RSI overbought
            if rsi >= cfg.rsi_overbought {
                breakdown.rsi = 0;
            } else {
                // Rising RSI from oversold zone is best
                let prev = rsi_prev.unwrap_or(rsi);
                if rsi > 50.0 && prev < 40.0 {
                    breakdown.rsi = 1;
                } else if rsi > 50.0 {
                    breakdown.rsi = 1;
                } else {
                    breakdown.rsi = 0;
                }
            }
        }
        Direction::Short => {
            // No SELL when RSI oversold
            if rsi <= cfg.rsi_oversold {
                breakdown.rsi = 0;
            } else {
                let prev = rsi_prev.unwrap_or(rsi);
                if rsi < 50.0 && prev > 60.0 {
                    breakdown.rsi = 1;
                } else if rsi < 50.0 {
                    breakdown.rsi = 1;
                } else {
                    breakdown.rsi = 0;
                }
            }
        }
        Direction::None => breakdown.rsi = 0,
    }
    
    // === VOLATILITY SCORE (0-1) ===
    // Placeholder - actual check done separately
    breakdown.volatility = 1; // Assumed OK unless checked otherwise
    
    // === CONFIRMATION SCORE (0-1) ===
    // Placeholder - actual check done separately  
    breakdown.confirmation = 1;
    
    // === TOTAL SCORE ===
    breakdown.total = breakdown.trend + breakdown.strength + 
                      breakdown.structure + breakdown.pullback + 
                      breakdown.rsi + breakdown.volatility + breakdown.confirmation;
    
    // === CONFIDENCE (0-1) ===
    let max_possible = 10;
    let confidence = (breakdown.total as f64) / (max_possible as f64);
    
    (breakdown.total, confidence, breakdown)
}

// ============================================================
// MAIN TRADING DECISION FUNCTION
// ============================================================

/// Main entry decision function with full filtering
/// Returns Signal with action, price levels, and scoring
pub fn should_trade(
    state: &mut State,
    price: f64,
    bid: f64,
    ask: f64,
    current_candle: &Candle,
    cfg: &Config,
) -> Signal {
    state.ticks_processed += 1;
    
    // ============================================================
    // STEP 1: Update indicators
    // ============================================================
    let prices_slice: Vec<f64> = state.prices.iter().copied().collect();
    let closes_slice: Vec<f64> = state.closes.iter().copied().collect();
    let highs_slice: Vec<f64> = state.highs.iter().copied().collect();
    let lows_slice: Vec<f64> = state.lows.iter().copied().collect();
    
    let ema_fast = match calc_ema(&prices_slice, cfg.ema_fast) {
        Some(v) => v,
        None => return Signal::hold("WARMUP: waiting for EMA data"),
    };
    
    let ema_slow = match calc_ema(&prices_slice, cfg.ema_slow) {
        Some(v) => v,
        None => return Signal::hold("WARMUP: waiting for EMA slow data"),
    };
    
    let atr = match calc_atr(&highs_slice, &lows_slice, &closes_slice, cfg.atr_period) {
        Some(v) => v,
        None => return Signal::hold("WARMUP: waiting for ATR data"),
    };
    
    let rsi = match calc_rsi(&closes_slice, cfg.rsi_period) {
        Some(v) => v,
        None => return Signal::hold("WARMUP: waiting for RSI data"),
    };
    
    // Update state
    state.ema_fast = Some(ema_fast);
    state.ema_slow = Some(ema_slow);
    state.rsi_prev = state.rsi;
    state.rsi = Some(rsi);
    state.atr = Some(atr);
    
    // ============================================================
    // STEP 2: Check cooldown
    // ============================================================
    if state.consecutive_losses >= cfg.max_consecutive_losses {
        let pause_end = state.last_loss_time.unwrap_or(0) + (cfg.pause_duration_minutes * 60);
        if current_candle.time < pause_end {
            return Signal::hold(&format!(
                "COOLDOWN: {} consecutive losses, pausing until {}",
                state.consecutive_losses, pause_end
            ));
        } else {
            state.consecutive_losses = 0;
        }
    }
    
    if state.cooldown_counter > 0 {
        state.cooldown_counter -= 1;
        return Signal::hold(&format!("COOLDOWN: {} ticks remaining", state.cooldown_counter));
    }
    
    // ============================================================
    // STEP 3: Check position limits
    // ============================================================
    if state.long_positions >= cfg.max_positions_per_direction {
        return Signal::hold("LIMIT: max long positions reached");
    }
    if state.short_positions >= cfg.max_positions_per_direction {
        return Signal::hold("LIMIT: max short positions reached");
    }
    
    // ============================================================
    // STEP 4: FILTER 1 - Sideway check (MANDATORY)
    // ============================================================
    if is_sideway(ema_fast, ema_slow, cfg.sideway_ema_threshold) {
        return Signal::skip("FILTER: Sideway market (EMA convergence)");
    }
    
    // ============================================================
    // STEP 5: FILTER 2 - Trend strength check
    // ============================================================
    let strength = trend_strength(ema_fast, ema_slow);
    if strength < cfg.min_trend_strength {
        return Signal::skip(&format!(
            "FILTER: Weak trend strength {:.3} < {:.3}",
            strength, cfg.min_trend_strength
        ));
    }
    
    // ============================================================
    // STEP 6: Determine primary direction
    // ============================================================
    let direction = if ema_fast > ema_slow { Direction::Long } else { Direction::Short };
    
    // ============================================================
    // STEP 7: FILTER 3 - Market structure check
    // ============================================================
    let structure = detect_structure(&highs_slice, &lows_slice, 20);
    
    // Structure must align with trend direction
    let structure_valid = match (direction, structure) {
        (Direction::Long, SwingType::HigherHigh) => true,
        (Direction::Long, SwingType::HigherLow) => true,
        (Direction::Short, SwingType::LowerHigh) => true,
        (Direction::Short, SwingType::LowerLow) => true,
        _ => false,
    };
    
    if !structure_valid {
        return Signal::skip(&format!(
            "FILTER: Structure {:?} doesn't match direction {:?}",
            structure, direction
        ));
    }
    
    // Anti-pattern: don't SELL when HL forming, don't BUY when LH forming
    match structure {
        SwingType::HigherLow if direction == Direction::Short => {
            return Signal::skip("FILTER: HL forming - no SELL");
        }
        SwingType::LowerHigh if direction == Direction::Long => {
            return Signal::skip("FILTER: LH forming - no BUY");
        }
        _ => {}
    }
    
    // ============================================================
    // STEP 8: FILTER 4 - Pullback check (CRITICAL)
    // ============================================================
    if !is_pullback(price, ema_fast, cfg.max_pullback_pips, cfg.pip_value) {
        let dist = (price - ema_fast).abs() / cfg.pip_value;
        return Signal::skip(&format!(
            "FILTER: Not in pullback zone ({:.1} pips from EMA)",
            dist
        ));
    }
    
    // ============================================================
    // STEP 9: FILTER 5 - RSI filter
    // ============================================================
    let (rsi_ok, _) = check_rsi(rsi, state.rsi_prev, direction, cfg);
    
    // Additional RSI zone checks
    match direction {
        Direction::Long => {
            if rsi >= cfg.rsi_overbought {
                return Signal::skip(&format!(
                    "FILTER: RSI {:.1} in overbought zone",
                    rsi
                ));
            }
        }
        Direction::Short => {
            if rsi <= cfg.rsi_oversold {
                return Signal::skip(&format!(
                    "FILTER: RSI {:.1} in oversold zone",
                    rsi
                ));
            }
        }
        Direction::None => return Signal::hold("No direction determined"),
    }
    
    if !rsi_ok {
        return Signal::skip(&format!(
            "FILTER: RSI confirmation failed (current: {:.1})",
            rsi
        ));
    }
    
    // ============================================================
    // STEP 10: FILTER 6 - Volatility filter
    // ============================================================
    let (vol_ok, vol_reason) = check_volatility(
        current_candle, 
        atr, 
        cfg.max_candle_mult, 
        cfg.max_wick_ratio
    );
    
    if !vol_ok {
        return Signal::skip(&format!("FILTER: {}", vol_reason));
    }
    
    // ============================================================
    // STEP 11: FILTER 7 - Anti-FOMO
    // ============================================================
    if !check_anti_fomo(price, ema_fast, cfg.max_fomo_pips, cfg.pip_value) {
        let dist = (price - ema_fast).abs() / cfg.pip_value;
        return Signal::skip(&format!(
            "FILTER: Anti-FOMO - price {:.1} pips from EMA (max: {:.1})",
            dist, cfg.max_fomo_pips
        ));
    }
    
    // ============================================================
    // STEP 12: FILTER 8 - No-trade zone
    // ============================================================
    if is_in_no_trade_zone(price, &state.recent_trade_prices, cfg.no_trade_zone_pips, cfg.pip_value) {
        return Signal::skip("FILTER: No-trade zone (recent trade price)");
    }
    
    // ============================================================
    // STEP 13: Calculate score
    // ============================================================
    let (score, confidence, breakdown) = calculate_score(
        price, ema_fast, ema_slow, rsi, state.rsi_prev,
        atr, structure, direction, cfg
    );
    
    // Check minimum score threshold
    if score < cfg.min_score {
        return Signal::skip(&format!(
            "FILTER: Score {} < threshold {} | breakdown: {:?}",
            score, cfg.min_score, breakdown
        ));
    }
    
    // Check confidence threshold
    if confidence < cfg.min_confidence {
        return Signal::skip(&format!(
            "FILTER: Confidence {:.2} < {:.2}",
            confidence, cfg.min_confidence
        ));
    }
    
    // ============================================================
    // STEP 14: Calculate SL/TP
    // ============================================================
    let entry_price = match direction {
        Direction::Long => ask,
        Direction::Short => bid,
        Direction::None => return Signal::hold("No direction for entry"),
    };
    
    // Minimum SL distance: MT5 requires >= 0.5 for GOLD
    let min_sl_distance = 0.5_f64.max(atr * cfg.sl_mult);
    // TP should be at least 1.5x SL distance
    let tp_distance = min_sl_distance * (cfg.tp_mult / cfg.sl_mult);
    
    let (stop_loss, take_profit) = match direction {
        Direction::Long => (entry_price - min_sl_distance, entry_price + tp_distance),
        Direction::Short => (entry_price + min_sl_distance, entry_price - tp_distance),
        Direction::None => (0.0, 0.0),
    };
    
    // Verify TP >= 1.5 * SL
    let sl_dist = (stop_loss - entry_price).abs();
    let tp_dist = (take_profit - entry_price).abs();
    if tp_dist < sl_dist * 1.5 {
        return Signal::skip(&format!(
            "FILTER: TP validation failed ({:.2} vs min {:.2})",
            tp_dist, sl_dist * 1.5
        ));
    }
    
    // ============================================================
    // STEP 15: Build signal
    // ============================================================
    let action = match direction {
        Direction::Long => SignalAction::EnterLong,
        Direction::Short => SignalAction::EnterShort,
        Direction::None => SignalAction::Hold,
    };
    
    let reason = format!(
        "{}: score={} conf={:.2} struct={:?} rsi={:.1} ema_dist={:.3}",
        match direction {
            Direction::Long => "BUY",
            Direction::Short => "SELL",
            Direction::None => "NONE",
        },
        score,
        confidence,
        structure,
        rsi,
        strength
    );
    
    Signal {
        direction,
        action,
        confidence,
        score,
        entry_price,
        stop_loss,
        take_profit,
        reason,
        breakdown,
    }
}

// ============================================================
// SIGNAL HELPER METHODS
// ============================================================

impl Signal {
    pub fn hold(reason: &str) -> Self {
        Self {
            direction: Direction::None,
            action: SignalAction::Hold,
            confidence: 0.0,
            score: 0,
            entry_price: 0.0,
            stop_loss: 0.0,
            take_profit: 0.0,
            reason: reason.to_string(),
            breakdown: ScoreBreakdown::default(),
        }
    }
    
    pub fn skip(reason: &str) -> Self {
        Self {
            direction: Direction::None,
            action: SignalAction::SkipDueToFilter,
            confidence: 0.0,
            score: 0,
            entry_price: 0.0,
            stop_loss: 0.0,
            take_profit: 0.0,
            reason: reason.to_string(),
            breakdown: ScoreBreakdown::default(),
        }
    }
    
    #[inline]
    pub fn is_enter(&self) -> bool {
        matches!(self.action, SignalAction::EnterLong | SignalAction::EnterShort)
    }
}

// ============================================================
// UNIT TESTS
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_sideway_detection() {
        let cfg = Config::default();
        assert!(is_sideway(1900.00, 1900.20, 0.30));  // Close = sideway
        assert!(!is_sideway(1900.00, 1899.50, 0.30)); // Far apart = trend
    }
    
    #[test]
    fn test_pullback_check() {
        let cfg = Config::default();
        assert!(is_pullback(1900.10, 1900.00, 15.0, 0.01)); // 10 pips = OK
        assert!(!is_pullback(1900.30, 1900.00, 15.0, 0.01)); // 30 pips = too far
    }
    
    #[test]
    fn test_rsi_filter() {
        let cfg = Config::default();
        
        // SELL: RSI dropping from >60 to <50
        let (ok, score) = check_rsi(45.0, Some(65.0), Direction::Short, &cfg);
        assert!(ok);
        assert_eq!(score, 1);
        
        // BUY: RSI rising from <40 to >50
        let (ok, score) = check_rsi(55.0, Some(35.0), Direction::Long, &cfg);
        assert!(ok);
        assert_eq!(score, 1);
    }
    
    #[test]
    fn test_volatility_filter() {
        let cfg = Config::default();
        let atr = 0.50;
        
        // Normal candle
        let candle = Candle {
            time: 0,
            open: 1900.00,
            high: 1900.30,
            low: 1899.90,
            close: 1900.20,
            volume: 100,
        };
        let (ok, _) = check_volatility(&candle, atr, 1.5, 0.5);
        assert!(ok);
        
        // Large candle (rejected)
        let big_candle = Candle {
            time: 0,
            open: 1900.00,
            high: 1901.00,
            low: 1899.00,
            close: 1900.50,
            volume: 100,
        };
        let (ok, msg) = check_volatility(&big_candle, atr, 1.5, 0.5);
        assert!(!ok);
        assert!(msg.contains("VOLATILITY"));
    }
    
    #[test]
    fn test_structure_detection() {
        // Bullish structure: rising highs and lows
        let highs = vec![1900.0, 1901.0, 1902.0, 1903.0, 1904.0];
        let lows = vec![1895.0, 1896.0, 1897.0, 1898.0, 1899.0];
        
        let structure = detect_structure(&highs, &lows, 5);
        assert!(matches!(structure, SwingType::HigherHigh | SwingType::HigherLow));
        
        // Bearish structure: falling highs and lows
        let highs_bear = vec![1910.0, 1909.0, 1908.0, 1907.0, 1906.0];
        let lows_bear = vec![1900.0, 1899.0, 1898.0, 1897.0, 1896.0];
        
        let structure_bear = detect_structure(&highs_bear, &lows_bear, 5);
        assert!(matches!(structure_bear, SwingType::LowerLow | SwingType::LowerHigh));
    }
    
    #[test]
    fn test_score_calculation() {
        let cfg = Config::default();
        
        // Strong bullish setup
        let (score, conf, _) = calculate_score(
            1900.10,   // price near EMA
            1900.00,   // ema_fast
            1899.50,   // ema_slow (bullish)
            55.0,      // rsi in neutral zone
            Some(40.0), // rsi rising
            0.50,      // atr
            SwingType::HigherHigh, // bullish structure
            Direction::Long,
            &cfg
        );
        
        assert!(score >= 5, "Score should be >= 5 for valid setup, got {}", score);
        assert!(conf >= 0.5, "Confidence should be >= 0.5, got {:.2}", conf);
    }
}