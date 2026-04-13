# Python Bridge - MT5 to ZeroMQ Bridge

## Overview

Python Bridge là ứng dụng Python kết nối MetaTrader 5 (MT5) với ZeroMQ, cho phép:

1. **Nhận dữ liệu market** từ MT5 (symbols như GOLD, XAUUSD)
2. **Publish dữ liệu** qua ZeroMQ publisher trên port 5555
3. **Nhận lệnh trading** qua ZeroMQ subscriber trên port 5556

## Features

- ✅ Kết nối MT5 để lấy real-time tick data
- ✅ ZeroMQ publisher (bind) trên port 5555 cho market data
- ✅ ZeroMQ publisher (bind) trên port 5556 cho order responses
- ✅ ZeroMQ subscriber (connect) trên port 5556 cho order commands
- ✅ Automatic cleanup khi đóng ứng dụng
- ✅ Logging toàn bộ hoạt động ra console và file riêng biệt
- ✅ Thread-safe design

## Architecture

```
┌─────────────────┐
│   MT5 Terminal  │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  MT5Connector   │  (Kết nối MT5, lấy tick data)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   ZMQManager    │  (Quản lý ZeroMQ connections)
└───────┬─────────┘
        │
   ┌────┴────┐
   ▼         ▼
┌────────┐ ┌────────┐
│ Port   │ │ Port   │
│ 5555   │ │ 5556   │
│ PUB    │ │ SUB/PUB│
└────────┘ └────────┘
```

## Installation

### 1. Cài đặt MetaTrader 5

Đảm bảo MT5 đã được cài đặt và đang chạy trên máy của bạn.

### 2. Cài đặt Python dependencies

```bash
cd python_bridge
pip install -r requirements.txt
```

### 3. Chạy ứng dụng

```bash
# Cách 1: Chạy trực tiếp
python -m python_bridge

# Cách 2: Import trong code
from python_bridge.main import run
run()
```

## Configuration

Chỉnh sửa file `config.py` để thay đổi cấu hình:

```python
# ZeroMQ ports
zmq.market_data_port = 5555  # Market data publisher
zmq.order_port = 5556        # Order commands & responses

# MT5 symbols
mt5.symbols = ["GOLD", "XAUUSD"]

# Logging
logging.log_file = "logs/bridge.log"
```

## Message Formats

### Market Data (Port 5555 - PUB)

```json
{
  "type": "TICK",
  "data": {
    "symbol": "GOLD",
    "bid": 2345.67,
    "ask": 2345.89,
    "spread": 0.22,
    "spread_points": 22.0,
    "time": "2024-01-15T10:30:00",
    "server_time": "2024-01-15T10:30:00.123"
  },
  "timestamp": "2024-01-15T10:30:00.125"
}
```

### Order Command (Port 5556 - SUB)

#### Send Order
```json
{
  "type": "ORDER_SEND",
  "data": {
    "symbol": "GOLD",
    "volume": 0.1,
    "order_type": "BUY",
    "price": 2345.67,
    "stop_loss": 2340.0,
    "take_profit": 2355.0,
    "comment": "EA Order",
    "request_id": "unique_id_123"
  }
}
```

#### Close Position
```json
{
  "type": "POSITION_CLOSE",
  "data": {
    "ticket": 12345678,
    "volume": 0.1
  }
}
```

#### Modify Position
```json
{
  "type": "POSITION_MODIFY",
  "data": {
    "ticket": 12345678,
    "stop_loss": 2342.0,
    "take_profit": 2358.0
  }
}
```

### Order Response (Port 5556 - PUB)

```json
{
  "success": true,
  "message_type": "ORDER_SEND",
  "ticket": 12345678,
  "volume": 0.1,
  "price": 2345.67,
  "comment": "...",
  "request_id": "unique_id_123",
  "timestamp": "2024-01-15T10:30:05"
}
```

## Logging

Logs được ghi vào thư mục `logs/`:

- `logs/bridge.log` - System logs (kết nối, khởi động, tắt máy)
- `logs/market.log` - Market data logs
- `logs/order.log` - Order execution logs

## ZeroMQ Connection Points

| Component | Type | Port | Address |
|-----------|------|------|---------|
| Market Publisher | BIND | 5555 | tcp://*:5555 |
| Order Publisher | BIND | 5556 | tcp://*:5556 |
| Order Subscriber | CONNECT | 5556 | tcp://localhost:5556 |

## Example Usage

### Python Client

```python
import zmq
import json

# Subscribe to market data
context = zmq.Context()
socket = context.socket(zmq.SUB)
socket.connect("tcp://localhost:5555")
socket.setsockopt(zmq.SUBSCRIBE, b"")

while True:
    message = socket.recv_string()
    data = json.loads(message)
    print(f"{data['type']}: {data['data']}")
```

### Send Order

```python
import zmq
import json

context = zmq.Context()
socket = zmq.Context().socket(zmq.PUB)
socket.connect("tcp://localhost:5556")

order = {
    "type": "ORDER_SEND",
    "data": {
        "symbol": "GOLD",
        "volume": 0.1,
        "order_type": "BUY",
        "stop_loss": 2340.0,
        "take_profit": 2355.0,
        "comment": "Test order"
    }
}

socket.send_json(order)
```

## Graceful Shutdown

Ứng dụng hỗ trợ tắt graceful qua:
- Ctrl+C
- SIGTERM
- SIGINT

Tất cả threads và connections sẽ được clean up tự động.

## Project Structure

```
python_bridge/
├── __init__.py          # Package init
├── __main__.py          # Entry point
├── config.py            # Configuration
├── models.py            # Data models
├── mt5_connector.py     # MT5 connection
├── zmq_publisher.py     # ZeroMQ publisher/subscriber
├── worker.py            # Background workers
├── main.py              # Main application
├── requirements.txt     # Dependencies
└── README.md            # This file
```

## License

MIT License
