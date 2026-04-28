# Advanced Scalping Engine v2 (Backtest + Optimizer)

README này mô tả nhanh cách sử dụng, nội dung kỹ thuật và chi tiết về backtest + optimizer đã được tích hợp trong `engine-rust-v2`.

Lưu ý: README viết bằng tiếng Việt để thuận tiện cho đội. Nếu cần tiếng Anh, mình sẽ chuyển đổi.

---

## Mục tiêu

Mục tiêu của module optimizer mới:
- Thực thi một backtest "sạch" (replay strategy) — KHÔNG dùng lại trade logs để đánh giá.
- Chạy random-search trên không gian tham số, mỗi config được replay đầy đủ trên dữ liệu candle (no data leakage).
- Đảm bảo out-of-sample validation (train/test split 70/30) và reject các cấu hình không generalize.

## Tính năng chính

1. Backtest engine
   - Hàm: `pub fn backtest(candles: &[Candle], cfg: &Config) -> BacktestResult`
   - Replay hoàn chỉnh: khởi tạo `State::new()`, cập nhật lần lượt từng candle, gọi `should_trade(...)` để nhận signal.
   - Khi có signal enter, simulate trade forward qua candles để kiểm tra TP / SL:
     - LONG: TP nếu `high >= take_profit`, SL nếu `low <= stop_loss`.
     - SHORT: TP nếu `low <= take_profit`, SL nếu `high >= stop_loss`.
     - Nếu TP và SL cùng chạm trong một candle → ưu tiên SL (conservative).
   - Nếu trade không đóng trước khi hết dữ liệu, đóng tại `close` của candle cuối.
   - Trả về struct `BacktestResult` với các metrics: total_pnl, total_trades, winrate, expectancy, max_drawdown, sharpe_ratio.

2. Optimizer (Random Search)
   - Hàm: `pub fn optimize(candles: &[Candle], base_config: Config) -> OptimizationResult`
   - Train/Test split: 70% train / 30% test (theo time order).
   - Random search: mặc định 400 samples (người dùng có thể chỉnh code nếu cần).
   - Mỗi candidate config: chạy full `backtest` trên TRAIN.
   - Objective function (THEO YÊU CẦU):
     `score = sharpe_ratio * 2.0 + expectancy * 10.0 - max_drawdown * 3.0`
   - Parameter sampling ranges (đã dùng trong random search):
     - min_score: 3 → 9 (integer)
     - min_confidence: 0.30 → 0.90
     - sideway_ema_threshold: 0.05 → 1.0
     - min_trend_strength: 0.05 → 1.0
     - max_pullback_pips: 5 → 40
     - max_fomo_pips: 8 → 60
     - max_candle_mult: 1.0 → 3.0
     - sl_mult: 0.5 → 3.0
     - tp_mult: 1.0 → 4.0
   - Sau khi chọn best trên TRAIN, validate trên TEST. Reject best nếu:
     - test.total_pnl <= 0
     - hoặc test.drawdown quá lớn (hiện dùng rule conservative: test.max_drawdown > 0.5 * |test.total_pnl|)
   - Nếu reject → fallback về `base_config` (không thay đổi config runtime).

3. Output
   - `OptimizationResult` trả về:
     ```text
     OptimizationResult {
       best_config: Config,
       train_metrics: BacktestResult,
       test_metrics: BacktestResult,
     }
     ```
   - Lưu kết quả optimizer ra `optimizer_result.json` (đường dẫn configurable).

## File cấu trúc dữ liệu / Format

- Candle (JSON object) phải có các trường: `time` (i64), `open` (f64), `high` (f64), `low` (f64), `close` (f64), `volume` (i64)

Ví dụ `mt5_history.json` (một phần nhỏ):

```json
[
  {"time":1622505600,"open":1900.0,"high":1900.4,"low":1899.7,"close":1900.2,"volume":12},
  {"time":1622505660,"open":1900.2,"high":1900.9,"low":1900.1,"close":1900.7,"volume":10}
]
```

- Output `optimizer_result.json` (ví dụ):

```json
{
  "best_config": { "min_score": 5, "min_confidence": 0.6, "sideway_ema_threshold": 0.3 },
  "train_metrics": { "total_pnl": 123.4, "total_trades": 50, "winrate": 0.56, "expectancy": 2.468, "max_drawdown": 20.5, "sharpe_ratio": 1.23 },
  "test_metrics": { }
}
```

## Cài đặt & Build

Yêu cầu môi trường:
- Rust toolchain (rustc, cargo) — khuyến nghị stable hoặc mới hơn
- ZeroMQ (nếu chạy engine live) — chỉ cần để connect tới market/order sockets

Build:

```bash
cd engine-rust-v2
cargo build --release
```

## Chạy

Ví dụ chạy với optimizer tự động dùng file lịch sử candles:

```bash
# Chạy engine và tự động optimize tại khởi động bằng file mt5_history.json
cargo run --release -- --auto_optimize --history_file mt5_history.json --history_count 2000 --auto_reload_optimized_config
```

Một số flag hữu dụng (xem trong `src/main.rs`):
- --auto_optimize : bật optimizer khi startup (cần history file)
- --history_file : đường dẫn file candles JSON (mặc định mt5_history.json)
- --history_count : số candle cần load
- --optimizer_output_file : nơi lưu optimizer result (mặc định optimizer_result.json)
- --auto_reload_optimized_config : cho phép auto reload config đã được lưu

Nếu bạn chỉ muốn chạy optimizer offline (không khởi động engine chính) bạn có thể tạo một binary nhỏ để gọi `optimizer::optimize` trên một file history và lưu kết quả; hiện `main.rs` đã tích hợp lệnh auto_optimize để làm việc này.

## Metrics & Giải thích

- total_pnl: tổng lợi nhuận (price units) của tất cả trade đóng trong backtest
- total_trades: số trades
- winrate: tỷ lệ trades có pnl > 0
- expectancy: total_pnl / total_trades (kỳ vọng P&L/trade)
- max_drawdown: tối đa drawdown của equity curve (cumulative P&L)
- sharpe_ratio: mean(pnls) / stddev(pnls) * sqrt(N)

Objective optimizer: sử dụng sharpe * 2 + expectancy * 10 - max_drawdown * 3 như yêu cầu để so sánh và chọn best trên TRAIN.

## Các quy tắc mô phỏng trade (quan trọng)

- Replay strategy bằng cách gọi `should_trade` từng candle.
- Khi nhận signal enter, simulate forward theo candles, kiểm tra điều kiện TP/SL trên trường high/low:
  - LONG: nếu candle.low <= stop_loss -> SL hit; nếu candle.high >= take_profit -> TP hit
  - SHORT: nếu candle.high >= stop_loss -> SL hit; nếu candle.low <= take_profit -> TP hit
  - Nếu TP và SL cùng chạm trong 1 candle => ưu tiên SL
- Nếu không chạm TP/SL trước khi dữ liệu kết thúc => đóng tại giá close của candle cuối.

Đây là quy tắc conservative để tránh đánh giá quá lạc quan.

## Hiệu năng & Tối ưu

- Backtest/optimizer đã thiết kế để giảm clone không cần thiết: truyền slice & khởi tạo `State::new()` cho mỗi chạy.
- Random search mặc định 400 samples (cân bằng giữa tốc độ & coverage). Có thể tăng nếu bạn có nhiều CPU / muốn kỹ hơn.
- Đề xuất: nếu có dataset lớn (M1 nhiều triệu candles), bạn có thể parallelize sampling (ví dụ `rayon`) để tận dụng nhiều core. Hiện implementation sequential để đơn giản và tránh race trên `State`.

## Testing

- Nên tạo vài test candles nhỏ để kiểm thử luật TP/SL (đặc biệt kiểm SL-priority khi cùng chạm trong 1 candle).
- Có thể viết unit test sử dụng `backtest()` trên chuỗi candles mô phỏng và assert các giá trị PnL/trades.

## Troubleshooting

- Nếu không thấy optimizer chạy: kiểm tra `--history_file` tồn tại và có format JSON đúng.
- Nếu kết quả optimizer bị reject (fallback): kiểm tra `test_metrics.total_pnl` và `test_metrics.max_drawdown` — optimizer có rule conservative reject.
- Warnings trong `cargo build` (unused variables) không ảnh hưởng đến chạy; nếu muốn mình sẽ dọn.

## Quy ước & Lưu ý

- Không sử dụng trade logs để đánh giá optimizer (đã tránh để chống data leakage).
- Không thay đổi logic của `should_trade` trong `strategy_new` — optimizer replay chính xác chiến lược gốc.
- Slack/notification logic không bị thay đổi (mình chỉ tích hợp để gửi summary như trước).

## Contribute

Nếu bạn muốn cải thiện:
- Thêm parallel random-search (rayon) cho tốc độ.
- Thay objective function hoặc cho phép cấu hình objective qua CLI.
- Thêm k-fold time-series cross-validation thay vì single 70/30 split.
- Bổ sung unit tests cho backtest edge-cases.

---

Nếu bạn muốn mình tạo thêm README tiếng Anh, hoặc thêm phần hướng dẫn chạy một ví dụ thực tế (kèm file history mẫu), chỉ cần cho biết — mình sẽ bổ sung.

## CLI Options (tất cả các flag)

Dưới đây là danh sách đầy đủ các tham số dòng lệnh mà `engine-rust-v2` hỗ trợ (tên flag, kiểu, giá trị mặc định và mô tả ngắn). Bạn có thể truyền các flag khi khởi chạy để điều chỉnh hành vi engine.

- --market-addr (String, default: "tcp://127.0.0.1:5555")
  - Địa chỉ ZMQ PUB để nhận market ticks.

- --order-addr (String, default: "tcp://127.0.0.1:5556")
  - Địa chỉ ZMQ ROUTER/DEALER để gửi lệnh tới python_bridge / order router.

- --trade (bool, default: false)
  - Bật trading thực tế (nếu false chỉ gửi signals/logs, không gửi order).

- --symbol (String, default: "GOLD")
  - Tên symbol để trade/lọc ticks.

- --cooldown-sec (u64, default: 5)
  - Khoảng thời gian tối thiểu (giây) giữa hai lệnh gửi (cooldown giữa các order requests).

- --volume (f64, default: 0.01)
  - Volume mặc định (lots) khi mở lệnh.

- --max-volume-per-trade (f64, default: 0.10)
  - Giới hạn tổng volume tối đa cho 1 hướng (long/short).

- --max-total-volume (f64, default: 0.50)
  - Giới hạn tổng volume trên tất cả các vị thế đang mở.

- --min-score (i32, default: 5)
  - Ngưỡng điểm tối thiểu (scoring) để chấp nhận entry.

- --min-confidence (f64, default: 0.5)
  - Ngưỡng niềm tin (confidence 0.0-1.0) cần đạt để vào lệnh.

- --sideway-threshold (f64, default: 0.30)
  - Ngưỡng kiểm tra sideway dựa trên |EMA_fast - EMA_slow| (đơn vị giá).

- --min-trend-strength (f64, default: 0.20)
  - Khoảng cách EMA tối thiểu để coi là trend hợp lệ.

- --max-pullback-pips (f64, default: 15.0)
  - Khoảng pullback tối đa (pips) từ EMA để chấp nhận entry.

- --max-fomo-pips (f64, default: 25.0)
  - Giới hạn anti-FOMO: nếu giá quá xa EMA (pips) sẽ không vào.

- --cooldown-candles (usize, default: 15)
  - Số candle cooldown sau 1 lệnh thua.

- --max-losses (usize, default: 3)
  - Số lệnh thua liên tiếp tối đa trước khi tạm dừng.

- --pause-minutes (i64, default: 30)
  - Thời gian tạm dừng (phút) khi đạt max_losses.

- --max-candle-mult (f64, default: 1.5)
  - Hệ số nhân ATR để xác định candle quá lớn (volatility filter).

- --sl-mult (f64, default: 1.2)
  - Hệ số nhân ATR để tính Stop Loss (SL = ATR * sl_mult).

- --tp-mult (f64, default: 2.0)
  - Hệ số nhân ATR để tính Take Profit.

- --verbose (bool, default: false)
  - Bật verbose per-tick logging.

- --log-level (String, default: "info")
  - Mức log RUST_LOG (info, debug, warn, error).

- --slack-enabled (bool, default: false)
  - Bật gửi thông báo Slack qua webhook khi phát hiện signal/close/optimizer.

- --slack-webhook (String, default: "")
  - URL webhook Slack để gửi message (nếu bật slack_enabled).

- --slack-channel (String, default: "#trading")
  - Channel (ví dụ #trading) hoặc user (@...) để override channel khi gửi webhook.

- --slack-notify-port (u16, default: 0)
  - Port TCP (ZMQ SUB) để nhận notifications đóng vị thế từ order_monitor.py (0 = disabled).

- --auto-optimize (bool, default: false)
  - Khi bật, engine sẽ chạy optimizer tại startup nếu có history và lưu kết quả.

- --strategy-log-file (String, default: "strategy_logs.json")
  - Đường dẫn file lưu logs/entries chiến lược.

- --optimizer-output-file (String, default: "optimizer_result.json")
  - File để lưu kết quả optimizer (best config + metrics).

- --live-log-file (String, default: "strategy_logs.json")
  - File append các logs per-live (một file thường được dùng cho cả live & strategy logs).

- --auto-reload-optimized-config (bool, default: true)
  - Auto reload file optimizer_output_file trong runtime (áp dụng best_config khi file thay đổi theo chu kỳ).

- --optimizer-reload-sec (u64, default: 60)
  - Số giây giữa các lần kiểm tra reload optimizer file.

- --status-interval-sec (u64, default: 15)
  - Khoảng thời gian (giây) để engine gửi Status message lên Slack (mặc định ~15s). Thích hợp đặt trong khoảng 10-15s.

- --loose-start (bool, default: false)
  - Nếu bật, engine áp config 'loose starter' để tăng tần suất trade lúc khởi động (dùng cho demo/testing).

- --per-tick-log (bool, default: false)
  - Log phân tích chi tiết mỗi tick (debug-level) khi bật.

- --require-confirmation (bool, default: true)
  - Mới: Bật/Tắt cơ chế one-candle confirmation trước entry. Nếu false thì skip bước chờ 1 candle xác nhận và sẽ vào lệnh ngay.

- --history-file (String, default: "mt5_history.json")
  - File chứa M1 historical candles (JSON array) để dùng cho optimizer/warmup nếu use_mt5_bridge=false.

- --history-count (usize, default: 500)
  - Số candle gần nhất cần load từ history_file hoặc python bridge.

- --require-history (bool, default: false)
  - Nếu true và không load được history khi startup -> engine sẽ exit.

- --history-wait-sec (u64, default: 30)
  - Thời gian chờ (giây) khi yêu cầu history qua python_bridge tại startup.

- --use-mt5-bridge (bool, default: false)
  - Dùng Python MT5 bridge (python_bridge) để lấy history / gửi lệnh thay vì file.

- --mt5-bridge-script (String, default: "python_bridge/mt5_bridge.py")
  - Đường dẫn tới script python bridge (thường không cần thay đổi).

- --mt5-symbol (String, default: "GOLD")
  - Symbol để request history khi dùng mt5 bridge.


Lưu ý: Bạn có thể xem mô tả ngắn cho từng flag khi chạy `--help` (clap sẽ in giúp):

```bash
./engine-rust-v2 --help
```

Nếu bạn muốn mình thêm ví dụ chạy với 1 hoặc 2 tổ hợp flag thường dùng (ví dụ demo mode, live mode với Slack), mình sẽ bổ sung ở phần đầu README.


