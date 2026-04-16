use clap::Parser;
use serde_json::Value;
use chrono::{DateTime, NaiveDateTime, Utc, Local};
use std::collections::VecDeque;
use std::time::Duration;
use std::thread;
use uuid::Uuid;
use log::{info, warn, debug};
use env_logger::Env;

/// engine-rust-v3: breakout OR volume-spike (direction by last tick)
#[derive(Parser, Debug)]
#[command(author, version, about = "Rust engine v3: breakout OR vol-spike", long_about = None)]
struct Args {
    #[arg(long, default_value = "tcp://127.0.0.1:5555")]
    market_addr: String,
    #[arg(long, default_value = "tcp://127.0.0.1:5556")]
    order_addr: String,
    #[arg(long, default_value_t = false)]
    trade: bool,
    #[arg(long, default_value_t = 15)]
    cooldown: u64,
    #[arg(long, default_value_t = 0.01)]
    volume: f64,
    #[arg(long, default_value = "GOLD")]
    symbol: String,
    #[arg(long, default_value_t = 20usize)]
    breakout_window: usize,
    #[arg(long, default_value_t = 12usize)]
    vol_avg_ticks: usize,
    #[arg(long, default_value_t = 1.6)]
    vol_spike_mult: f64,
    #[arg(long, default_value_t = 0.3)]
    breakout_min_move: f64,
    /// Number of ticks to evaluate momentum
    #[arg(long, default_value_t = 3usize)]
    momentum_ticks: usize,
    /// Minimum absolute cumulative tick-delta to consider momentum strong
    #[arg(long, default_value_t = 0.05)]
    momentum_min_delta: f64,
    #[arg(long, default_value_t = false)]
    tick_verbose: bool,
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[derive(Clone)]
struct Tick {
    price: f64,
    volume: i64,
    ts: i64,
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

fn max_in_slice(slice: &[f64]) -> Option<f64> {
    if slice.is_empty() { return None; }
    Some(slice.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
}

fn min_in_slice(slice: &[f64]) -> Option<f64> {
    if slice.is_empty() { return None; }
    Some(slice.iter().cloned().fold(f64::INFINITY, f64::min))
}

fn avg_volume(vols: &[i64]) -> Option<f64> {
    if vols.is_empty() { return None; }
    let sum: i64 = vols.iter().sum();
    Some(sum as f64 / (vols.len() as f64))
}

fn main() {
    let args = Args::parse();

    let log_env = Env::default().filter_or("RUST_LOG", &args.log_level);
    env_logger::Builder::from_env(log_env)
        .format(|buf, record| {
            use std::io::Write;
            let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
            writeln!(buf, "{} {:<5} {}", ts, record.level(), record.args())
        })
        .init();

    info!("Starting engine-rust-v3 (symbol={})", args.symbol);

    let ctx = zmq::Context::new();
    let sub = ctx.socket(zmq::SUB).expect("failed to create SUB socket");
    sub.connect(&args.market_addr).expect("failed to connect SUB");
    info!("Connected to market publisher at {}", args.market_addr);
    sub.set_subscribe(b"").expect("failed to subscribe");

    let dealer = if args.trade {
        let s = ctx.socket(zmq::DEALER).expect("failed to create DEALER socket");
        s.connect(&args.order_addr).expect("failed to connect DEALER");
        s.set_rcvtimeo(5000).ok();
        info!("Connected to order router at {}", args.order_addr);
        Some(s)
    } else { None };

    let mut window: VecDeque<Tick> = VecDeque::new();
    let mut last_action_time: Option<DateTime<Utc>> = None;

    loop {
        match sub.recv_string(0) {
            Ok(Ok(msg)) => {
                debug!("raw message: {}", msg);
                let v: Value = match serde_json::from_str(&msg) {
                    Ok(v) => v,
                    Err(e) => { warn!("invalid json from publisher: {}", e); continue; }
                };

                let msg_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if msg_type != "TICK" { continue; }

                let data = &v["data"];
                let symbol = data.get("symbol").and_then(|x| x.as_str()).unwrap_or("");
                if symbol != args.symbol { continue; }

                let price = data.get("last").and_then(|x| x.as_f64())
                    .or_else(|| data.get("bid").and_then(|x| x.as_f64()))
                    .unwrap_or(0.0);
                let volume = data.get("volume").and_then(|x| x.as_i64()).unwrap_or(0);
                let time_str = data.get("server_time").and_then(|x| x.as_str())
                    .or_else(|| data.get("time").and_then(|x| x.as_str()))
                    .unwrap_or("");

                let dt = match parse_iso_datetime(time_str) {
                    Some(d) => d,
                    None => { warn!("failed parse timestamp: {} ; using Utc::now()", time_str); Utc::now() }
                };
                let ts = dt.timestamp();

                window.push_back(Tick { price, volume, ts });

                while let Some(front) = window.front() {
                    if (ts - front.ts) as usize > args.breakout_window { window.pop_front(); } else { break; }
                }

                if window.len() < 2 { if args.tick_verbose { debug!("insufficient ticks: {}", window.len()); } continue; }

                let prices: Vec<f64> = window.iter().map(|t| t.price).collect();
                let n = prices.len();
                let prev_slice = &prices[..(n - 1)];
                let prev_high = max_in_slice(prev_slice).unwrap_or(prices[n - 1]);
                let prev_low = min_in_slice(prev_slice).unwrap_or(prices[n - 1]);

                let breakout_up = prices[n - 1] > prev_high + args.breakout_min_move;
                let breakout_down = prices[n - 1] < prev_low - args.breakout_min_move;

                // avg vol excluding current tick
                let vols_iter = window.iter().rev().skip(1).take(args.vol_avg_ticks).map(|t| t.volume).collect::<Vec<i64>>();
                let avg_vol = avg_volume(&vols_iter).unwrap_or(0.0);
                let vol_spike = if avg_vol > 0.0 { (volume as f64) >= avg_vol * args.vol_spike_mult } else { false };

                // compute momentum over recent ticks
                let m = args.momentum_ticks.min(window.len());
                let mut momentum = 0.0f64;
                if m >= 2 {
                    let start = n - m;
                    for i in (start + 1)..n {
                        momentum += prices[i] - prices[i - 1];
                    }
                }
                let momentum_ok = momentum.abs() >= args.momentum_min_delta;
                let momentum_dir = if momentum > 0.0 { 1 } else if momentum < 0.0 { -1 } else { 0 };

                // last-tick direction (fast simple check)
                let mut vol_dir = 0i32;
                if n >= 2 {
                    if prices[n - 1] > prices[n - 2] { vol_dir = 1; }
                    else if prices[n - 1] < prices[n - 2] { vol_dir = -1; }
                }

                // Decision: require momentum + (breakout OR vol_spike aligned with momentum)
                let mut action = "HOLD";
                if momentum_ok && ((breakout_up && momentum_dir == 1) || (vol_spike && momentum_dir == 1)) {
                    action = "BUY";
                } else if momentum_ok && ((breakout_down && momentum_dir == -1) || (vol_spike && momentum_dir == -1)) {
                    action = "SELL";
                }

                if args.tick_verbose || action != "HOLD" {
                    info!("Tick summary price={:.5} high_prev={:.5} low_prev={:.5} breakout_up={} breakout_down={} momentum={:.4} momentum_ok={} vol={} avg_vol={:.2} vol_spike={} vol_dir={} action={}",
                          price, prev_high, prev_low, breakout_up, breakout_down, momentum, momentum_ok, volume, avg_vol, vol_spike, vol_dir, action);
                }

                if action != "HOLD" {
                    let now = Utc::now();
                    let can_send = match last_action_time {
                        Some(t) => (now - t).num_seconds() >= args.cooldown as i64,
                        None => true,
                    };
                    if can_send {
                        if args.trade {
                            if let Some(ref sock) = dealer {
                                let order_type = if action == "BUY" { "BUY" } else { "SELL" };
                                let request_id = Uuid::new_v4().to_string();
                                let payload = serde_json::json!({
                                    "type": "ORDER_SEND",
                                    "data": {
                                        "symbol": args.symbol,
                                        "volume": args.volume,
                                        "order_type": order_type,
                                        "price": 0,
                                        "stop_loss": serde_json::Value::Null,
                                        "take_profit": serde_json::Value::Null,
                                        "comment": format!("v3-breakout:{}", action),
                                        "magic": 2200,
                                        "request_id": request_id
                                    }
                                });
                                let s = payload.to_string();
                                info!("Sending ORDER {} {} lots (comment={})", order_type, args.volume, format!("v3-breakout:{}", action));
                                debug!("order payload: {}", s);
                                match sock.send(s.as_bytes(), 0) {
                                    Ok(_) => match sock.recv_string(0) {
                                        Ok(Ok(resp)) => info!("order response: {}", resp),
                                        Ok(Err(_)) => warn!("non-utf8 reply from bridge"),
                                        Err(e) => warn!("no reply or error receiving reply: {:?}", e),
                                    },
                                    Err(e) => warn!("failed to send order: {:?}", e),
                                }
                            }
                        } else {
                            info!("Signal {} (trade disabled).", action);
                        }
                        last_action_time = Some(now);
                    } else {
                        debug!("Signal {} suppressed due cooldown", action);
                    }
                }
            }
            Ok(Err(e)) => { warn!("recv_string error: {:?}", e); thread::sleep(Duration::from_millis(50)); }
            Err(e) => { warn!("zmq recv error: {:?}", e); thread::sleep(Duration::from_millis(200)); }
        }
    }
}
