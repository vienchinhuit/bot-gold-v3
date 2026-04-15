"""Test script: Gửi order riêng lẻ và chờ lệnh tiếp theo."""

import zmq
import json
import time
import uuid
import threading
import os

# Magic number counter cho mỗi batch
_current_magic = 1000

# File lưu batch info
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BATCH_INFO_FILE = os.path.join(SCRIPT_DIR, "batch_info.json")

def get_next_magic():
    global _current_magic
    _current_magic += 100
    return _current_magic

def save_batch_info(magic, pnl_target, tickets):
    """Lưu batch info vào file JSON."""
    batch_info = {}
    # Ensure directory exists
    try:
        os.makedirs(os.path.dirname(BATCH_INFO_FILE), exist_ok=True)
    except Exception:
        pass

    if os.path.exists(BATCH_INFO_FILE):
        try:
            with open(BATCH_INFO_FILE, 'r') as f:
                batch_info = json.load(f)
        except Exception:
            batch_info = {}

    # Lưu tickets theo magic với target
    batch_info[str(magic)] = {
        "pnl_target": pnl_target,
        "tickets": tickets
    }

    try:
        with open(BATCH_INFO_FILE, 'w') as f:
            json.dump(batch_info, f, indent=2)
    except Exception as e:
        print(f"Failed to save batch info to {BATCH_INFO_FILE}: {e}")

def remove_batch_info(magic):
    """Xóa batch info sau khi đóng xong."""
    try:
        if os.path.exists(BATCH_INFO_FILE):
            with open(BATCH_INFO_FILE, 'r') as f:
                batch_info = json.load(f)
        else:
            batch_info = {}

        if str(magic) in batch_info:
            del batch_info[str(magic)]
            try:
                with open(BATCH_INFO_FILE, 'w') as f:
                    json.dump(batch_info, f, indent=2)
            except Exception as e:
                print(f"Failed to update batch info file: {e}")
    except Exception as e:
        print(f"Error removing batch info: {e}")


class OrderClient:
    def __init__(self, port=5556):
        self.port = port
        self.context = zmq.Context()
        self.socket = self.context.socket(zmq.DEALER)
        self.socket.connect(f"tcp://localhost:{port}")
        self.socket.setsockopt(zmq.RCVTIMEO, 5000)
        
        self._running = True
        self._response_queue = []
        self._thread = threading.Thread(target=self._receive_loop, daemon=True)
        self._thread.start()
    
    def _receive_loop(self):
        while self._running:
            try:
                msg = self.socket.recv_json()
                self._response_queue.append(msg)
            except zmq.Again:
                pass
            except Exception:
                pass
    
    def send(self, msg_type, data):
        message = {"type": msg_type, "data": data}
        self.socket.send_json(message)
    
    def get_response(self, timeout=10):
        start = time.time()
        while time.time() - start < timeout:
            if self._response_queue:
                return self._response_queue.pop(0)
            time.sleep(0.05)
        return None
    
    def close(self):
        self._running = False
        self._thread.join(timeout=2)
        self.socket.close()
        self.context.term()


def send_order(client, symbol, order_type, volume, magic, comment=""):
    """Send order and return ticket if success."""
    request_id = str(uuid.uuid4())[:8]
    
    client.send("ORDER_SEND", {
        "symbol": symbol,
        "volume": volume,
        "order_type": order_type,
        "price": 0,
        "stop_loss": None,
        "take_profit": None,
        "comment": comment,
        "magic": magic,
        "request_id": request_id
    })
    
    response = client.get_response(timeout=10)
    if response and response.get('success'):
        return response.get('ticket')
    return None


def get_all_positions(client, symbol=None):
    """Get all positions."""
    client.send("ORDER_INFO", {"symbol": symbol} if symbol else {})
    response = client.get_response(timeout=5)
    
    if response and response.get('success'):
        return response.get('positions', [])
    return []


def close_position(client, ticket):
    """Close a specific position."""
    client.send("POSITION_CLOSE", {"ticket": ticket, "volume": 0})
    return client.get_response(timeout=10)


def get_input(prompt, default, cast_type=str):
    """Get input with default value."""
    try:
        value = input(prompt).strip()
        if not value:
            return default
        return cast_type(value)
    except:
        return default


def print_positions(client, symbol="GOLD"):
    """In ra danh sách positions hiện tại."""
    positions = get_all_positions(client, symbol)
    if positions:
        print("\n--- Current Positions ---")
        for p in positions:
            print(f"  #{p.get('ticket')}: Magic={p.get('magic')} | {p.get('type')} {p.get('volume')} lots | P&L: ${p.get('profit', 0):+.2f}")
        print()
    else:
        print("\n--- No open positions ---")
    return positions


def main():
    print("=" * 60)
    print("ORDER SENDER - Gửi order riêng lẻ")
    print("=" * 60)
    
    client = OrderClient(port=5556)
    symbol = "GOLD"
    
    print("\nNhập thông tin order (Enter để dùng giá trị mặc định)")
    print("Gõ 'positions' để xem positions hiện tại")
    print("Gõ 'close <ticket>' để đóng position")
    print("Gõ 'quit' hoặc 'exit' để thoát")
    print()
    
    while True:
        print("-" * 40)
        
        # Get order type
        order_type = get_input("Order type (BUY/SELL) [default: BUY]: ", "BUY").upper()
        
        if order_type in ["QUIT", "EXIT", "Q"]:
            break
        
        if order_type == "POSITIONS":
            print_positions(client, symbol)
            continue
        
        if order_type.startswith("CLOSE "):
            try:
                ticket = int(order_type.split()[1])
                resp = close_position(client, ticket)
                if resp and resp.get('success'):
                    print(f"Position #{ticket} đã đóng thành công!")
                else:
                    print(f"Không thể đóng position #{ticket}")
            except:
                print("Lệnh close không hợp lệ. Format: close <ticket>")
            continue
        
        if order_type not in ["BUY", "SELL"]:
            print("Invalid order type! Use BUY or SELL.")
            continue
        
        # Get other parameters
        volume = get_input("Volume [default: 0.01]: ", 0.01, float)
        num_orders = get_input("Số lượng order [default: 1]: ", 1, int)
        pnl_target = get_input("P&L target per order [default: 1.0]: ", 1.0, float)
        
        # Lấy magic cho batch này
        magic = get_next_magic()
        
        # Tạo comment chứa P&L target (để monitor đọc được)
        # Format: "Magic:{magic}|Target:{target}"
        comment = f"Magic:{magic}|Target:{pnl_target}"
        
        print(f"\n[BATCH Magic: {magic}] P&L Target: ${pnl_target}")
        print(f">>> Sending {num_orders} {order_type} order(s): {volume} lots on {symbol}")
        
        success_count = 0
        success_tickets = []
        for i in range(num_orders):
            ticket = send_order(client, symbol, order_type, volume, magic, comment)
            if ticket:
                print(f"  Order {i+1}: SUCCESS! Ticket #{ticket}")
                success_count += 1
                success_tickets.append(ticket)
            else:
                print(f"  Order {i+1}: FAILED")
        
        # Lưu batch info vào file JSON
        if success_tickets:
            save_batch_info(magic, pnl_target, success_tickets)
            print(f"  [Saved] Magic={magic}, Target=${pnl_target}")
        
        print(f"\nCompleted: {success_count}/{num_orders} orders")
        print(f"Magic #{magic} | P&L target: ${pnl_target}")
        print()
    
    print("\nĐóng kết nối...")
    client.close()
    print("Done!")


if __name__ == "__main__":
    main()