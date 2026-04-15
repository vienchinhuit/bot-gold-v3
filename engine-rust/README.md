# Rust strategy engine

Đây là engine chiến lược viết bằng Rust, thiết kế để nhận dữ liệu market từ `python_bridge` qua ZeroMQ, gom nén thành thanh 1 phút, tính toán chỉ báo RSI và MACD, rồi ra tín hiệu BUY/SELL. Có tuỳ chọn gửi lệnh qua ZeroMQ về `python_bridge` để thực thi (order router).

## Tổng quan chiến lược

- Dữ liệu vào: nhận JSON `TICK` từ publisher của `python_bridge` (mặc định `tcp://127.0.0.1:5555`).
- Gom nén: tạo thanh 1 phút (mỗi bar dùng giá `last` nếu có, nếu không dùng `bid`).
- Chỉ báo: RSI với period = 14; MACD với cấu hình (fast=12, slow=26, signal=9).
- Quy tắc vào lệnh (entry):
	- BUY khi: RSI < 30 và MACD histogram vừa chuyển lên (hist_curr > 0 và hist_prev < hist_curr).
	- SELL khi: RSI > 70 và MACD histogram vừa chuyển xuống (hist_curr < 0 và hist_prev > hist_curr).

Logic này kết hợp tín hiệu momentum (MACD histogram crossing) với trạng thái quá bán/quá mua của RSI để giảm nhiễu.

## Chi tiết kỹ thuật

- RSI: được tính bằng trung bình cộng các biến động tăng/giảm trong cửa sổ `period` (14). Giá đầu vào là close của mỗi bar.
- MACD: EMA fast (12) và EMA slow (26) được tính trên chuỗi close; MACD line = EMA_fast - EMA_slow; signal = EMA(signal_period) trên MACD line; histogram = MACD - signal. Implementation dùng SMA seed cho EMA signal và hệ số k = 2/(period+1).
- Bar aggregation: mỗi tick cập nhật bar hiện tại (open được khởi tạo bởi first tick, high/low/close cập nhật liên tục, volume cộng dồn). Khi bước sang phút mới, bar trước được hoàn tất và dùng close để tính chỉ báo.

## Đầu ra / Gửi lệnh

- Khi `--trade` bật (hoặc gửi thủ công qua prompt test), engine sẽ gửi một JSON `ORDER_SEND` tới order router (mặc định `tcp://127.0.0.1:5556`) dưới dạng DEALER socket. Payload mẫu:

```json
{
	"type": "ORDER_SEND",
	"data": {
		"symbol": "GOLD",
		"volume": 0.01,
		"order_type": "BUY",
		"price": 0,
		"stop_loss": null,
		"take_profit": null,
		"comment": "rust-strategy:BUY",
		"magic": 2000,
		"request_id": "<uuid>"
	}
}
```

- Lưu ý: `price: 0` nghĩa là bridge/MT5 connector sẽ dùng giá hiện tại (bid/ask) khi gửi lệnh thực tế.

## Cấu hình & khởi chạy

- CLI flags:
	- `--market-addr` : địa chỉ ZeroMQ publisher feed (default `tcp://127.0.0.1:5555`).
	- `--order-addr`  : địa chỉ order router (default `tcp://127.0.0.1:5556`).
	- `--trade`       : bật gửi lệnh thực tế; nếu không bật chỉ log tín hiệu.
	- `--cooldown`    : thời gian (giây) tối thiểu giữa hai lệnh liên tiếp (mặc định 60).
	- `--volume`      : khối lượng (lots) khi gửi lệnh (mặc định 0.01).
	- `--symbol`      : symbol cần theo dõi (mặc định `GOLD`).
	- `--log-level`   : mức log console (`error,warn,info,debug,trace`).

- Biến môi trường (override):
	- `ZMQ_MARKET_ADDR` và `ZMQ_ORDER_ADDR` để ghi đè địa chỉ ZMQ nếu cần.

- Tương tác khi khởi động: nếu stdin là TTY, engine sẽ hỏi có gửi test-order ngay khi start hay không (mặc định No). Dùng để kiểm tra kết nối tới `python_bridge`.

Ví dụ build & chạy:

```bash
cd engine-rust
cargo build --release
# chỉ log tín hiệu
cargo run --release -- --symbol GOLD --log-level info
# bật gửi lệnh
cargo run --release -- --symbol GOLD --trade --log-level info
```

Hoặc dùng biến môi trường để override địa chỉ:

```bash
ZMQ_MARKET_ADDR=tcp://127.0.0.1:5555 ZMQ_ORDER_ADDR=tcp://127.0.0.1:5556 cargo run --release -- --symbol GOLD --trade
```

## Ghi log

- Engine ghi log console theo format `timestamp LEVEL message` (mức log có thể điều chỉnh bằng `--log-level` hoặc `RUST_LOG`). Log bao gồm: kết nối ZMQ, subscription, nhận tick cho symbol đang theo dõi, tín hiệu bar, gửi order và phản hồi từ bridge.

## Hạn chế & khuyến nghị

- Đây là bản prototype: không có quản lý vị thế nội bộ, không set SL/TP mặc định, không tính sizing theo rủi ro. Việc gửi lệnh dựa trên tín hiệu tại thời điểm bar đóng — cần kiểm thử kĩ trước khi bật auto-trade.
- Khuyến nghị cải thiện trước khi live-trade:
	- Thêm quản lý trạng thái lệnh/positions (để tránh gửi lệnh chồng chéo).
	- Áp dụng sizing dựa trên equity/risk và thêm SL/TP mặc định.
	- Thêm xác nhận hoặc retry khi order bị reject.
	- Thử nghiệm trên dữ liệu replay hoặc simulator trước khi kết nối tới MT5 thật.

## Kiểm thử nhanh (smoke test)

1. Chạy `python_bridge` (đảm bảo publisher và order router đang chạy).
2. Chạy engine ở chế độ test (`--trade` và trả lời `y` khi được hỏi) để gửi một `ORDER_SEND` thử và kiểm tra `python_bridge` nhận được.

---

Nếu bạn muốn tôi mở rộng README thêm phần "Cấu hình risk/SL/TP", hoặc bổ sung ví dụ payload chi tiết từ `python_bridge`, mình có thể cập nhật thêm.
