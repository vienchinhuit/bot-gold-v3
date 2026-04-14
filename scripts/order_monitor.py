"""Order Monitor: Theo dõi và đóng lệnh theo từng position."""

import zmq
import json
import time
import threading
import os
from collections import defaultdict

# Cấu hình
SYMBOL = "GOLD"
PORT = 5556
CHECK_INTERVAL = 0.5  # Giây
BATCH_INFO_FILE = "scripts/batch_info.json"


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


def load_batch_info():
    """Đọc batch info từ file JSON."""
    try:
        if os.path.exists(BATCH_INFO_FILE):
            with open(BATCH_INFO_FILE, 'r') as f:
                return json.load(f)
    except:
        pass
    return {}


def save_batch_info(batch_info):
    """Lưu batch info vào file JSON."""
    try:
        with open(BATCH_INFO_FILE, 'w') as f:
            json.dump(batch_info, f, indent=2)
    except:
        pass


def main():
    print("=" * 60)
    print("ORDER MONITOR - Theo dõi và đóng theo từng position")
    print("=" * 60)
    print(f"  Symbol: {SYMBOL}")
    print(f"  Check Interval: {CHECK_INTERVAL}s")
    print("=" * 60)
    print()
    
    client = OrderClient(port=PORT)
    
    # Lấy positions hiện tại
    print("Đang lấy positions hiện tại...")
    positions = get_all_positions(client, SYMBOL)
    
    if not positions:
        print("Không có positions nào!")
        print("Đang chờ positions mới...")
        print("Nhấn Ctrl+C để thoát")
    else:
        print(f"Tìm thấy {len(positions)} positions:")
        for p in positions:
            magic, target = parse_comment(p.get('comment', ''))
            print(f"  #{p.get('ticket')}: Magic={magic} | Target=${target} | P&L: ${p.get('profit', 0):+.2f}")
    
    print()
    print("Bắt đầu theo dõi... (Ctrl+C để dừng)")
    print("-" * 60)
    
    total_pnl_achieved = 0.0
    total_closed = 0
    i = 0
    
    try:
        while True:
            positions = get_all_positions(client, SYMBOL)
            
            i += 1
            elapsed = round(i * CHECK_INTERVAL, 1)
            
            if positions:
                # Group positions theo magic number
                magic_groups = defaultdict(list)
                for p in positions:
                    magic = p.get('magic', 0)
                    magic_groups[magic].append({
                        'ticket': p.get('ticket'),
                        'profit': p.get('profit', 0),
                        'magic': magic,
                        'type': p.get('type'),
                        'volume': p.get('volume')
                    })
                
                # Đọc batch info từ file JSON
                batch_info = load_batch_info()
                
                # Print status
                status_parts = []
                for magic, pos_list in magic_groups.items():
                    pnl = sum(p.get('profit', 0) for p in pos_list)
                    target = batch_info.get(str(magic), {}).get('pnl_target', '?')
                    status_parts.append(f"Magic#{magic}({len(pos_list)} pos, ${pnl:+.2f}/${target})")
                
                print(f"[{elapsed:6.1f}s] {' | '.join(status_parts)}")
                
                # Check từng position trong từng batch
                positions_to_close = []
                for magic, pos_list in magic_groups.items():
                    target = batch_info.get(str(magic), {}).get('pnl_target', 1.0)
                    for pos in pos_list:
                        pos['target'] = target
                        if pos['profit'] >= target:
                            positions_to_close.append(pos)
                
                # Đóng các positions đạt target
                if positions_to_close:
                    print(f"\n>>> Đóng {len(positions_to_close)} position(s) đạt target...")
                    
                    for pos in positions_to_close:
                        print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} >= Target ${pos['target']:.2f}")
                        resp = close_position(client, pos['ticket'])
                        
                        if resp and resp.get('success'):
                            print(f"    SUCCESS!")
                            total_pnl_achieved += pos['profit']
                            total_closed += 1
                            
                            # Xóa ticket khỏi batch_info
                            magic_str = str(pos['magic'])
                            if magic_str in batch_info:
                                tickets = batch_info[magic_str].get('tickets', [])
                                if pos['ticket'] in tickets:
                                    tickets.remove(pos['ticket'])
                                    batch_info[magic_str]['tickets'] = tickets
                                    save_batch_info(batch_info)
                        else:
                            print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
            else:
                print(f"[{elapsed:6.1f}s] Chờ positions mới...")
            
            time.sleep(CHECK_INTERVAL)
    
    except KeyboardInterrupt:
        print("\n\nDừng theo dõi!")
        
        # Đóng các positions còn lại
        positions = get_all_positions(client, SYMBOL)
        if positions:
            print("Đóng các positions còn lại...")
            for p in positions:
                ticket = p.get('ticket')
                resp = close_position(client, ticket)
                if resp and resp.get('success'):
                    print(f"  #{ticket}: CLOSED")
                else:
                    print(f"  #{ticket}: FAILED")
    
    print("\n" + "=" * 60)
    print(f"Tổng P&L đã đạt: ${total_pnl_achieved:+.2f}")
    print(f"Số positions đã đóng: {total_closed}")
    print("=" * 60)
    
    client.close()


if __name__ == "__main__":
    main()