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
use log::debug;

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
    pub debug: String,
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
    pub structure: i32,       // 0-2
    pub pullback: i32,        // 0-1
    pub rsi: i32,             // 0-1
    pub volatility: i32,      // 0-1
    pub confirmation: i32,    // 0-1
    pub reversal_risk: i32,   // -3..0 penalty
    pub total: i32,           // Sum
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
    
    // === CONFIRMATION ===
    pub require_confirmation: bool, // If false, skip the one-candle confirmation to increase frequency

    // === MOMENTUM OVERRIDE ===
    // If true, strong recent candles (relative to ATR) can override sideway EMA filter
    pub momentum_override_enabled: bool,
    // Multiplier of ATR to consider a candle 'strong' for override (e.g. 1.0 = body >= ATR)
    pub momentum_override_mult: f64,

    // === SCALP MODE FLAG ===
    // When true, several filters are relaxed to favor higher-frequency scalping
    pub scalp_mode: bool,
    }

impl Default for Config {
    fn default() -> Self {
        Self {
            // Indicator periods
            ema_fast: 8,
            ema_slow: 20,
            rsi_period: 14,
            atr_period: 14,
            
            // Sideway detection
            sideway_ema_threshold: 0.30,  // 30 cents for GOLD (unchanged)
            
            // Trend strength
            min_trend_strength: 0.02,    // lowered to allow weaker trends for scalping
            
            // Pullback & FOMO
            max_pullback_pips: 60.0,  // increased to allow scalp entries further from EMA
            max_fomo_pips: 80.0,   // allow larger distance before anti-fomo blocks entry
            
            // RSI zones
            rsi_oversold: 15.0,   // relax RSI oversold/overbought for scalp
            rsi_overbought: 85.0,
            rsi_sell_confirm_low: 50.0,
            rsi_sell_confirm_high: 60.0,
            rsi_buy_confirm_low: 40.0,
            rsi_buy_confirm_high: 50.0,
            
            // Volatility
            max_candle_mult: 3.0,   // allow larger candles (less strict volatility filter)
            max_wick_ratio: 2.0,   // tolerate larger wick-to-body ratio for fast moves
            
            // Scoring
            min_score: 1, // keep low min_score for scalping
            min_confidence: 0.30, // lower confidence threshold to allow more entries
            
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
            
            // Confirmation
            require_confirmation: false, // no confirmation by default for higher frequency
            // Momentum override defaults
            momentum_override_enabled: true,
            momentum_override_mult: 0.6, // make momentum override easier to trigger

            // Scalp mode default
            scalp_mode: false,
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
    pending_long_setup: Option<i64>,  // timestamp for confirmation
    pending_short_setup: Option<i64>,

    // Recent candle behavior for reversal detection
    pub recent_bullish_streak: usize,
    pub recent_bearish_streak: usize,
    pub last_candle_direction: Option<Direction>,
    pub last_candle_range: Option<f64>,
    
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
            recent_bullish_streak: 0,
            recent_bearish_streak: 0,
            last_candle_direction: None,
            last_candle_range: None,
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

        let dir = if candle.close > candle.open {
            Direction::Long
        } else if candle.close < candle.open {
            Direction::Short
        } else {
            Direction::None
        };

        match dir {
            Direction::Long => {
                self.recent_bullish_streak += 1;
                self.recent_bearish_streak = 0;
            }
            Direction::Short => {
                self.recent_bearish_streak += 1;
                self.recent_bullish_streak = 0;
            }
            Direction::None => {
                self.recent_bullish_streak = 0;
                self.recent_bearish_streak = 0;
            }
        }

        self.last_candle_direction = Some(dir);
        self.last_candle_range = Some(candle.range());
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

/// 1b. MOMENTUM OVERRIDE CHECK
/// Detects whether recent candles show strong directional momentum (relative to ATR)
/// that should override a sideway EMA condition. Returns true if override should apply.
pub fn detect_momentum_override(state: &State, atr: f64, mult: f64) -> bool {
    // Need at least one completed candle
    if state.candles.is_empty() { return false; }

    // Check last candle
    if let Some(last) = state.candles.back() {
        let body = last.body();
        if body >= atr * mult {
            // Strong single candle
            return true;
        }
    }

    // Check last two candles same-direction and combined strength
    if state.candles.len() >= 2 {
        let n = state.candles.len();
        let last = state.candles[n-1];
        let prev = state.candles[n-2];
        let body_sum = last.body() + prev.body();
        // Require both candles to have the same direction (both bullish or both bearish)
        let dir_last = last.close.partial_cmp(&last.open);
        let dir_prev = prev.close.partial_cmp(&prev.open);
        if dir_last.is_some() && dir_prev.is_some() && dir_last == dir_prev {
            if body_sum >= atr * mult * 1.5 {
                return true;
            }
        }
    }

    false
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

// Helper wrappers to log reasons when returning early
fn ret_hold(reason: &str) -> Signal {
    debug!("STRATEGY HOLD: {}", reason);
    Signal::hold(reason)
}

fn ret_skip(reason: &str) -> Signal {
    debug!("STRATEGY SKIP: {}", reason);
    Signal::skip(reason)
}


/// Calculate comprehensive trade score
/// Returns (score, confidence, breakdown)
#[inline]
pub fn detect_reversal_risk(
    current_candle: &Candle,
    state: &State,
    direction: Direction,
    _cfg: &Config,
) -> (i32, String) {
    let mut penalty = 0;
    let mut reasons = Vec::new();
    let body = current_candle.body().max(0.0001);
    let range = current_candle.range().max(0.0001);
    let upper_wick = current_candle.upper_wick();
    let lower_wick = current_candle.lower_wick();
    let wick_body_ratio = upper_wick.max(lower_wick) / body;
    let close_position = (current_candle.close - current_candle.low) / range;

    match direction {
        Direction::Long => {
            if current_candle.close < current_candle.open { penalty -= 1; reasons.push("bearish candle"); }
            if upper_wick > body * 1.2 { penalty -= 1; reasons.push("upper wick"); }
            if wick_body_ratio > 2.0 { penalty -= 1; reasons.push("wick/body too large"); }
            if state.recent_bearish_streak >= 2 { penalty -= 1; reasons.push("bearish streak"); }
            if close_position < 0.35 { penalty -= 1; reasons.push("close near low"); }
        }
        Direction::Short => {
            if current_candle.close > current_candle.open { penalty -= 1; reasons.push("bullish candle"); }
            if lower_wick > body * 1.2 { penalty -= 1; reasons.push("lower wick"); }
            if wick_body_ratio > 2.0 { penalty -= 1; reasons.push("wick/body too large"); }
            if state.recent_bullish_streak >= 2 { penalty -= 1; reasons.push("bullish streak"); }
            if close_position > 0.65 { penalty -= 1; reasons.push("close near high"); }
        }
        Direction::None => {}
    }

    if let Some(prev_range) = state.last_candle_range {
        if range > prev_range * 1.6 { penalty -= 1; reasons.push("range expansion"); }
    }

    if penalty < -3 { penalty = -3; }
    (penalty, reasons.join("; "))
}

pub fn build_debug_line(
    direction: Direction,
    price: f64,
    ema_fast: f64,
    ema_slow: f64,
    rsi: f64,
    atr: f64,
    structure: SwingType,
    reversal_reason: &str,
    reversal_penalty: i32,
    structure_soft_penalty: i32,
    score: i32,
    confidence: f64,
    breakdown: &ScoreBreakdown,
) -> String {
    format!(
        "dir={:?} price={:.2} ema20={:.2} ema50={:.2} rsi={:.1} atr={:.3} struct={:?} rev_pen={} struct_pen={} score={} conf={:.2} break[T:{} S:{} St:{} P:{} R:{} V:{} C:{} Rev:{}] reason={}",
        direction,
        price,
        ema_fast,
        ema_slow,
        rsi,
        atr,
        structure,
        reversal_penalty,
        structure_soft_penalty,
        score,
        confidence,
        breakdown.trend,
        breakdown.strength,
        breakdown.structure,
        breakdown.pullback,
        breakdown.rsi,
        breakdown.volatility,
        breakdown.confirmation,
        breakdown.reversal_risk,
        reversal_reason
    )
}

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
    // Structure is now a soft score, not a hard blocker
    match (direction, structure) {
        (Direction::Long, SwingType::HigherHigh) => breakdown.structure = 2,
        (Direction::Long, SwingType::HigherLow) => breakdown.structure = 1,
        (Direction::Long, SwingType::LowerHigh) => breakdown.structure = -1,
        (Direction::Long, SwingType::LowerLow) => breakdown.structure = -2,
        (Direction::Short, SwingType::LowerLow) => breakdown.structure = 2,
        (Direction::Short, SwingType::LowerHigh) => breakdown.structure = 1,
        (Direction::Short, SwingType::HigherLow) => breakdown.structure = -1,
        (Direction::Short, SwingType::HigherHigh) => breakdown.structure = -2,
        _ => breakdown.structure = 0,
    }
    
    // === PULLBACK SCORE (0-1) ===
    let pullback_distance = (price - ema_fast).abs() / cfg.pip_value;
    if pullback_distance <= cfg.max_pullback_pips {
        breakdown.pullback = 1;
    } else if pullback_distance <= cfg.max_pullback_pips * 2.0 {
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
                } else if rsi > 45.0 {
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
                } else if rsi < 55.0 {
                    breakdown.rsi = 1;
                } else {
                    breakdown.rsi = 0;
                }
            }
        }
        Direction::None => breakdown.rsi = 0,
    }
    
    // === VOLATILITY SCORE (0-1) ===
    breakdown.volatility = 1;
    
    // === CONFIRMATION SCORE (0-1) ===
    breakdown.confirmation = if cfg.require_confirmation { 1 } else { 0 };

    // === REVERSION RISK PENALTY ===
    breakdown.reversal_risk = 0;
    if direction == Direction::Long && structure == SwingType::HigherHigh { breakdown.reversal_risk = 0; }
    if direction == Direction::Short && structure == SwingType::LowerLow { breakdown.reversal_risk = 0; }
    
    // === TOTAL SCORE ===
    breakdown.total = breakdown.trend + breakdown.strength + 
                      breakdown.structure + breakdown.pullback + 
                      breakdown.rsi + breakdown.volatility + breakdown.confirmation +
                      breakdown.reversal_risk;
    
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
        None => return ret_hold("WARMUP: waiting for EMA data"),
    };
    
    let ema_slow = match calc_ema(&prices_slice, cfg.ema_slow) {
        Some(v) => v,
        None => return ret_hold("WARMUP: waiting for EMA slow data"),
    };
    
    let atr = match calc_atr(&highs_slice, &lows_slice, &closes_slice, cfg.atr_period) {
        Some(v) => v,
        None => return ret_hold("WARMUP: waiting for ATR data"),
    };
    
    let rsi = match calc_rsi(&closes_slice, cfg.rsi_period) {
        Some(v) => v,
        None => return ret_hold("WARMUP: waiting for RSI data"),
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
            return ret_hold(&format!(
                "COOLDOWN: {} consecutive losses, pausing until {}",
                state.consecutive_losses, pause_end
            ));
        } else {
            state.consecutive_losses = 0;
        }
    }
    
    if state.cooldown_counter > 0 {
        state.cooldown_counter -= 1;
        return ret_hold(&format!("COOLDOWN: {} ticks remaining", state.cooldown_counter));
    }
    
    // ============================================================
    // STEP 3: Check position limits
    // ============================================================
    if state.long_positions >= cfg.max_positions_per_direction {
        return ret_hold("LIMIT: max long positions reached");
    }
    if state.short_positions >= cfg.max_positions_per_direction {
        return ret_hold("LIMIT: max short positions reached");
    }
    
    // ============================================================
    // STEP 4: FILTER 1 - Sideway check (MANDATORY)
    // ============================================================
    if is_sideway(ema_fast, ema_slow, cfg.sideway_ema_threshold) {
        // Allow strong momentum to override sideway filter for fast scalping setups
        if cfg.momentum_override_enabled {
            if let Some(a) = Some(atr) {
                if detect_momentum_override(state, a, cfg.momentum_override_mult) {
                    debug!("SIDEWAY OVERRIDE: strong recent candles detected -> bypass sideway filter");
                } else {
                    return ret_skip("FILTER: Sideway market (EMA convergence)");
                }
            } else {
                return ret_skip("FILTER: Sideway market (EMA convergence)");
            }
        } else {
            return ret_skip("FILTER: Sideway market (EMA convergence)");
        }
    }
    
    // ============================================================
    // STEP 5: FILTER 2 - Trend strength check
    // ============================================================
    let strength = trend_strength(ema_fast, ema_slow);
    if strength < cfg.min_trend_strength {
        return ret_skip(&format!(
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
    let lookback = if cfg.scalp_mode { 8 } else { 20 };
    let structure = detect_structure(&highs_slice, &lows_slice, lookback);
    
    let structure_valid = match (direction, structure) {
        (Direction::Long, SwingType::HigherHigh) => true,
        (Direction::Long, SwingType::HigherLow) => true,
        (Direction::Short, SwingType::LowerHigh) => true,
        (Direction::Short, SwingType::LowerLow) => true,
        _ => false,
    };
    
    let structure_soft_penalty = if structure_valid { 0 } else { -1 };
    let (mut reversal_penalty, reversal_reason) = detect_reversal_risk(current_candle, state, direction, cfg);
    if reversal_penalty <= -3 {
        if cfg.scalp_mode {
            // In scalp mode be more permissive: cap severe reversal penalty to -1 instead of skipping
            debug!("SCALP MODE: capping reversal penalty {} -> -1", reversal_penalty);
            reversal_penalty = -1;
        } else {
            return ret_skip(&format!("FILTER: reversal risk too high ({})", reversal_reason));
        }
    }
    
    // ============================================================
    // STEP 8: FILTER 4 - Pullback check (CRITICAL)
    // ============================================================
    if !is_pullback(price, ema_fast, cfg.max_pullback_pips, cfg.pip_value) {
        let dist = (price - ema_fast).abs() / cfg.pip_value;
        let multiplier = if cfg.scalp_mode { 4.0 } else { 3.0 };
        if dist > cfg.max_pullback_pips * multiplier {
            return ret_skip(&format!(
                "FILTER: Too far from EMA ({:.1} pips)",
                dist
            ));
        }
    }

    let body = current_candle.body().max(0.0001);
    let range = current_candle.range().max(0.0001);
    let wick_ratio = current_candle.upper_wick().max(current_candle.lower_wick()) / body;
    let close_pos = (current_candle.close - current_candle.low) / range;

    if direction == Direction::Long {
        if !cfg.scalp_mode {
            if current_candle.close < current_candle.open && wick_ratio > 1.5 {
                return ret_skip("FILTER: bearish rejection candle - avoid BUY");
            }
            if current_candle.upper_wick() > body * 1.0 && close_pos < 0.6 {
                return ret_skip("FILTER: weak bullish close - possible reversal");
            }
            if state.recent_bearish_streak >= 2 && current_candle.close <= current_candle.open {
                return ret_skip("FILTER: bearish streak with weak candle - avoid BUY");
            }
        } else {
            debug!("SCALP MODE: bypassing candle reversal filters for LONG");
        }
    } else if direction == Direction::Short {
        if !cfg.scalp_mode {
            if current_candle.close > current_candle.open && wick_ratio > 1.5 {
                return ret_skip("FILTER: bullish rejection candle - avoid SELL");
            }
            if current_candle.lower_wick() > body * 1.0 && close_pos > 0.4 {
                return ret_skip("FILTER: weak bearish close - possible reversal");
            }
            if state.recent_bullish_streak >= 2 && current_candle.close >= current_candle.open {
                return ret_skip("FILTER: bullish streak with weak candle - avoid SELL");
            }
        } else {
            debug!("SCALP MODE: bypassing candle reversal filters for SHORT");
        }
    }
    
    // ============================================================
    // STEP 9: FILTER 5 - RSI filter
    // ============================================================
    let (rsi_ok, _) = check_rsi(rsi, state.rsi_prev, direction, cfg);
    
    // Additional RSI zone checks
    match direction {
        Direction::Long => {
            if rsi >= cfg.rsi_overbought {
                if cfg.scalp_mode {
                    debug!("SCALP MODE: allowing RSI overbought {:.1}", rsi);
                } else {
                    return ret_skip(&format!(
                        "FILTER: RSI {:.1} in overbought zone",
                        rsi
                    ));
                }
            }
        }
        Direction::Short => {
            if rsi <= cfg.rsi_oversold {
                if cfg.scalp_mode {
                    debug!("SCALP MODE: allowing RSI oversold {:.1}", rsi);
                } else {
                    return ret_skip(&format!(
                        "FILTER: RSI {:.1} in oversold zone",
                        rsi
                    ));
                }
            }
        }
        Direction::None => return ret_hold("No direction determined"),
    }
    
    if !rsi_ok {
        if cfg.scalp_mode {
            debug!("SCALP MODE: bypassing RSI confirmation (current: {:.1})", rsi);
        } else {
            return ret_skip(&format!(
                "FILTER: RSI confirmation failed (current: {:.1})",
                rsi
            ));
        }
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
        if cfg.scalp_mode {
            debug!("SCALP MODE: ignoring volatility filter: {}", vol_reason);
        } else {
            return ret_skip(&format!("FILTER: {}", vol_reason));
        }
    }
    
    // ============================================================
    // STEP 11: FILTER 7 - Anti-FOMO
    // ============================================================
    if !check_anti_fomo(price, ema_fast, cfg.max_fomo_pips, cfg.pip_value) {
        let dist = (price - ema_fast).abs() / cfg.pip_value;
        return ret_skip(&format!(
            "FILTER: Anti-FOMO - price {:.1} pips from EMA (max: {:.1})",
            dist, cfg.max_fomo_pips
        ));
    }
    
    // ============================================================
    // STEP 12: FILTER 8 - No-trade zone
    // ============================================================
    if cfg.scalp_mode {
        // In scalp mode we disable no-trade zone to allow quick re-entry
        debug!("SCALP MODE: skipping No-trade zone check");
    } else {
        if is_in_no_trade_zone(price, &state.recent_trade_prices, cfg.no_trade_zone_pips, cfg.pip_value) {
            return ret_skip("FILTER: No-trade zone (recent trade price)");
        }
    }
    
    // ============================================================
    // STEP 13: Calculate score
    // ============================================================
    let (mut score, confidence, mut breakdown) = calculate_score(
        price, ema_fast, ema_slow, rsi, state.rsi_prev,
        atr, structure, direction, cfg
    );
    breakdown.reversal_risk = reversal_penalty + structure_soft_penalty;
    score += reversal_penalty + structure_soft_penalty;
    let debug_line = build_debug_line(
        direction,
        price,
        ema_fast,
        ema_slow,
        rsi,
        atr,
        structure,
        &reversal_reason,
        reversal_penalty,
        structure_soft_penalty,
        score,
        confidence,
        &breakdown,
    );
    println!("[STRATEGY DEBUG] {}", debug_line);
    
    // Check minimum score threshold
    if score < cfg.min_score {
        return ret_skip(&format!(
            "FILTER: Score {} < threshold {} | breakdown: {:?}",
            score, cfg.min_score, breakdown
        ));
    }
    
    // Check confidence threshold
    if confidence < cfg.min_confidence {
        return ret_skip(&format!(
            "FILTER: Confidence {:.2} < {:.2}",
            confidence, cfg.min_confidence
        ));
    }
    
    // ============================================================
    // CONFIRMATION: optional one-candle confirmation to avoid entering on immediate reversal
    // If cfg.require_confirmation is false we skip the wait to increase trade frequency
    if cfg.require_confirmation {
        if direction == Direction::Long {
            match state.pending_long_setup {
                None => {
                    // mark current candle as pending confirmation and wait for the next candle
                    state.pending_long_setup = Some(current_candle.time);
                    // clear opposite pending
                    state.pending_short_setup = None;
                    debug!("AWAIT_CONFIRM: pending LONG at time {}", current_candle.time);
                    return ret_hold("AWAIT_CONFIRM: waiting next candle for LONG confirmation");
                }
                Some(pending_ts) => {
                    if current_candle.time <= pending_ts {
                        return ret_hold("AWAIT_CONFIRM: waiting next candle for LONG confirmation");
                    }
                    // We're on the candle after the pending one - require bullish confirmation
                    let range = current_candle.range().max(0.0001);
                    let close_pos = (current_candle.close - current_candle.low) / range;
                    if current_candle.close > current_candle.open && close_pos > 0.5 {
                        // confirmed
                        state.pending_long_setup = None;
                        debug!("CONFIRMED LONG at time {}", current_candle.time);
                    } else {
                        // failed confirmation - clear and skip
                        state.pending_long_setup = None;
                        debug!("CONFIRM_FAIL LONG at time {}", current_candle.time);
                        return ret_skip("CONFIRM_FAIL: LONG confirmation failed on next candle");
                    }
                }
            }
        } else if direction == Direction::Short {
            match state.pending_short_setup {
                None => {
                    state.pending_short_setup = Some(current_candle.time);
                    state.pending_long_setup = None;
                    debug!("AWAIT_CONFIRM: pending SHORT at time {}", current_candle.time);
                    return ret_hold("AWAIT_CONFIRM: waiting next candle for SHORT confirmation");
                }
                Some(pending_ts) => {
                    if current_candle.time <= pending_ts {
                        return ret_hold("AWAIT_CONFIRM: waiting next candle for SHORT confirmation");
                    }
                    let range = current_candle.range().max(0.0001);
                    let close_pos = (current_candle.close - current_candle.low) / range;
                    if current_candle.close < current_candle.open && close_pos < 0.5 {
                        state.pending_short_setup = None;
                        debug!("CONFIRMED SHORT at time {}", current_candle.time);
                    } else {
                        state.pending_short_setup = None;
                        debug!("CONFIRM_FAIL SHORT at time {}", current_candle.time);
                        return ret_skip("CONFIRM_FAIL: SHORT confirmation failed on next candle");
                    }
                }
            }
        }
    } else {
        // Confirmation disabled - clear any pending markers and proceed
        state.pending_long_setup = None;
        state.pending_short_setup = None;
    }

    // ============================================================
    // STEP 14: Calculate SL/TP
    // ============================================================
    let entry_price = match direction {
        Direction::Long => ask,
        Direction::Short => bid,
        Direction::None => return ret_hold("No direction for entry"),
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
    
    // Verify TP >= required_ratio * SL
    // In scalp_mode we SKIP TP validation entirely to favor faster entries
    if cfg.scalp_mode {
        debug!("SCALP MODE: skipping TP validation (allowing any TP/SL ratio)");
    } else {
        let sl_dist = (stop_loss - entry_price).abs();
        let tp_dist = (take_profit - entry_price).abs();
        let required_ratio = 1.5;
        if tp_dist < sl_dist * required_ratio {
            return ret_skip(&format!(
                "FILTER: TP validation failed ({:.2} vs min {:.2})",
                tp_dist, sl_dist * required_ratio
            ));
        }
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
        debug: debug_line,
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
            debug: String::new(),
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
            debug: String::new(),
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