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
CHECK_INTERVAL = 0.2  # Giây
STOP_LOSS = 0  # Đặt >0 để tự đóng khi lỗ (VD: 10 = đóng khi lỗ $10), đặt 0 để tắt
BATCH_PROFIT_TARGET = 10.0  # Tổng PNL đạt target này sẽ đóng toàn bộ batch (đặt 0 để tắt)
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BATCH_INFO_FILE = os.path.join(SCRIPT_DIR, "batch_info.json")


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
        os.makedirs(os.path.dirname(BATCH_INFO_FILE), exist_ok=True)
    except Exception:
        pass

    try:
        with open(BATCH_INFO_FILE, 'w') as f:
            json.dump(batch_info, f, indent=2)
    except Exception:
        pass


def update_batch_closed_info(batch_info, magic, closed_count, closed_pnl):
    """Cập nhật thông tin đã đóng cho batch."""
    magic_str = str(magic)
    if magic_str not in batch_info:
        batch_info[magic_str] = {}
    batch_info[magic_str]['closed_count'] = batch_info[magic_str].get('closed_count', 0) + closed_count
    batch_info[magic_str]['closed_pnl'] = batch_info[magic_str].get('closed_pnl', 0.0) + closed_pnl
    save_batch_info(batch_info)
    return batch_info


def clear_batch_closed_info(batch_info, magic):
    """Xóa thông tin đã đóng của batch."""
    magic_str = str(magic)
    if magic_str in batch_info:
        batch_info[magic_str]['closed_count'] = 0
        batch_info[magic_str]['closed_pnl'] = 0.0
        save_batch_info(batch_info)
    return batch_info


def parse_comment(comment: str):
    """Parse comment string like 'Magic:1000|Target:1.0' -> (magic:int, target:float|str)

    Returns (magic, target). If values can't be parsed, returns defaults (0, '?').
    """
    magic = 0
    target = '?'
    if not comment:
        return magic, target

    try:
        parts = comment.split('|')
        for p in parts:
            if ':' not in p:
                continue
            k, v = p.split(':', 1)
            k = k.strip().lower()
            v = v.strip()
            if k == 'magic':
                try:
                    magic = int(v)
                except Exception:
                    pass
            elif k in ('target', 'pnl_target'):
                try:
                    target = float(v)
                except Exception:
                    target = v
    except Exception:
        pass

    return magic, target


def main():
    print("=" * 60)
    print("ORDER MONITOR - Theo dõi và đóng theo từng position")
    print("=" * 60)
    print(f"  Symbol: {SYMBOL}")
    print(f"  Check Interval: {CHECK_INTERVAL}s")
    print("=" * 60)
    print()
    
    client = OrderClient(port=PORT)
    
    # Reset batch info khi khởi chạy
    batch_info = load_batch_info()
    for magic in batch_info:
        batch_info[magic]['closed_count'] = 0
        batch_info[magic]['closed_pnl'] = 0.0
    save_batch_info(batch_info)
    print("Đã reset batch info!")
    
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
    global_closed_pnl = 0.0
    global_closed_count = 0
    # Theo dõi lỗ/lãi riêng
    loss_closed_count = 0
    loss_closed_pnl = 0.0
    profit_closed_count = 0
    profit_closed_pnl = 0.0
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
                
                # Print status - mỗi batch 1 dòng
                print(f"[{elapsed:6.1f}s]")
                total_closed_pnl = 0.0
                total_closed_count = 0
                total_pnl_all = 0.0
                batch_info = load_batch_info()
                batches_to_close = []  # Các batch cần đóng toàn bộ
                
                for magic, pos_list in magic_groups.items():
                    pnl = sum(p.get('profit', 0) for p in pos_list)
                    target = batch_info.get(str(magic), {}).get('pnl_target', '?')
                    closed_count = batch_info.get(str(magic), {}).get('closed_count', 0)
                    closed_pnl = batch_info.get(str(magic), {}).get('closed_pnl', 0.0)
                    total_batch_pnl = pnl + closed_pnl  # Tổng PNL = đang mở + đã đóng
                    total_closed_pnl += closed_pnl
                    total_closed_count += closed_count
                    total_pnl_all += total_batch_pnl
                    print(f"  Magic#{magic}: {len(pos_list)} pos đang mở | PnL: ${pnl:+.2f}/{target} | Đã đóng: {closed_count} pos, PnL: ${closed_pnl:+.2f} | Tổng: ${total_batch_pnl:+.2f}")
                    
                    # Kiểm tra nếu batch đạt target thì đóng toàn bộ
                    if BATCH_PROFIT_TARGET > 0 and total_batch_pnl >= BATCH_PROFIT_TARGET:
                        batches_to_close.append({'magic': magic, 'pos_list': pos_list, 'total_pnl': total_batch_pnl})
                
                # Print 4 dòng tổng hợp
                print(f"  Đang mở: {len(positions)} pos, PnL: ${total_pnl_all:+.2f}")
                print(f"  Đã đóng (lỗ): {loss_closed_count} pos, PnL: ${loss_closed_pnl:+.2f}")
                print(f"  Đã đóng (lãi): {profit_closed_count} pos, PnL: ${profit_closed_pnl:+.2f}")
                print(f"  Tổng đã đóng: {global_closed_count} pos, PnL: ${global_closed_pnl:+.2f}")
                
                # Check từng position trong từng batch
                positions_to_close = []
                positions_to_close_sl = []
                for magic, pos_list in magic_groups.items():
                    target = batch_info.get(str(magic), {}).get('pnl_target', 1.0)
                    for pos in pos_list:
                        pos['target'] = target
                        if pos['profit'] >= target:
                            positions_to_close.append(pos)
                        elif STOP_LOSS > 0 and pos['profit'] <= -STOP_LOSS:
                            positions_to_close_sl.append(pos)
                
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
                            global_closed_pnl += pos['profit']
                            global_closed_count += 1
                            
                            # Cập nhật lãi/lỗ
                            if pos['profit'] >= 0:
                                profit_closed_count += 1
                                profit_closed_pnl += pos['profit']
                            else:
                                loss_closed_count += 1
                                loss_closed_pnl += pos['profit']
                            
                            # Cập nhật thông tin đã đóng cho batch
                            batch_info = update_batch_closed_info(batch_info, pos['magic'], 1, pos['profit'])
                        else:
                            print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                
                # Đóng các positions chạm Stop Loss
                if positions_to_close_sl:
                    print(f"\n>>> Đóng {len(positions_to_close_sl)} position(s) chạm SL (<= -${STOP_LOSS})...")
                    
                    for pos in positions_to_close_sl:
                        print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} <= -${STOP_LOSS} (SL)")
                        resp = close_position(client, pos['ticket'])
                        
                        if resp and resp.get('success'):
                            print(f"    SUCCESS!")
                            total_pnl_achieved += pos['profit']
                            total_closed += 1
                            global_closed_pnl += pos['profit']
                            global_closed_count += 1
                            
                            # Cập nhật lãi/lỗ
                            if pos['profit'] >= 0:
                                profit_closed_count += 1
                                profit_closed_pnl += pos['profit']
                            else:
                                loss_closed_count += 1
                                loss_closed_pnl += pos['profit']
                            
                            # Cập nhật thông tin đã đóng cho batch
                            batch_info = update_batch_closed_info(batch_info, pos['magic'], 1, pos['profit'])
                        else:
                            print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                
                # Đóng toàn bộ batch khi đạt target PNL
                if batches_to_close:
                    for batch in batches_to_close:
                        magic = batch['magic']
                        pos_list = batch['pos_list']
                        total_batch_pnl = batch['total_pnl']
                        print(f"\n>>> BATCH Magic#{magic} đạt target! Tổng PNL: ${total_batch_pnl:+.2f} >= ${BATCH_PROFIT_TARGET}")
                        print(f"    Đóng {len(pos_list)} position(s) còn lại...")
                        
                        closed_batch_pnl = 0.0
                        closed_batch_count = 0
                        for pos in pos_list:
                            print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} (đóng batch)")
                            resp = close_position(client, pos['ticket'])
                            
                            if resp and resp.get('success'):
                                print(f"    SUCCESS!")
                                total_pnl_achieved += pos['profit']
                                total_closed += 1
                                global_closed_pnl += pos['profit']
                                global_closed_count += 1
                                closed_batch_pnl += pos['profit']
                                closed_batch_count += 1
                                
                                # Cập nhật lãi/lỗ
                                if pos['profit'] >= 0:
                                    profit_closed_count += 1
                                    profit_closed_pnl += pos['profit']
                                else:
                                    loss_closed_count += 1
                                    loss_closed_pnl += pos['profit']
                            else:
                                print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                        
                        # Cộng dồn PNL đã đóng của batch vào batch_info
                        batch_info = update_batch_closed_info(batch_info, magic, closed_batch_count, closed_batch_pnl)
                        # Reset batch về từ đầu
                        batch_info = clear_batch_closed_info(batch_info, magic)
                        print(f"    Batch Magic#{magic} đã reset!")
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