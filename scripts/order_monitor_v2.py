"""Order Monitor v2

Theo dõi liên tục tất cả positions đang mở (không filter theo symbol)
và đóng những position có P&L >= PNL target do người dùng nhập lúc khởi động.

Usage: python scripts/order_monitor_v2.py
"""

import zmq
import json
import time
import threading
import sys
from datetime import datetime

# Config
PORT = 5556
CHECK_INTERVAL = 1.0  # seconds between polls

SCRIPT_DIR = __import__('os').path.dirname(__import__('os').path.abspath(__file__))
BATCH_INFO_FILE = __import__('os').path.join(SCRIPT_DIR, 'batch_info.json')

# Batch info helpers
def load_batch_info():
    try:
        import os, json
        if os.path.exists(BATCH_INFO_FILE):
            with open(BATCH_INFO_FILE, 'r', encoding='utf-8') as f:
                return json.load(f)
    except Exception:
        pass
    return {}


def save_batch_info(batch_info):
    try:
        import os, json
        os.makedirs(os.path.dirname(BATCH_INFO_FILE), exist_ok=True)
        with open(BATCH_INFO_FILE, 'w', encoding='utf-8') as f:
            json.dump(batch_info, f, indent=2)
    except Exception:
        pass


def update_batch_closed_info(batch_info, magic, closed_count, closed_pnl):
    magic_str = str(magic)
    if magic_str not in batch_info:
        batch_info[magic_str] = {}
    batch_info[magic_str]['closed_count'] = batch_info[magic_str].get('closed_count', 0) + closed_count
    batch_info[magic_str]['closed_pnl'] = batch_info[magic_str].get('closed_pnl', 0.0) + closed_pnl
    save_batch_info(batch_info)
    return batch_info


def clear_batch_closed_info(batch_info, magic):
    magic_str = str(magic)
    if magic_str in batch_info:
        batch_info[magic_str]['closed_count'] = 0
        batch_info[magic_str]['closed_pnl'] = 0.0
        save_batch_info(batch_info)
    return batch_info



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
                time.sleep(0.01)
            except Exception:
                # keep receiver alive on transient errors
                time.sleep(0.1)

    def send(self, msg_type, data):
        message = {"type": msg_type, "data": data}
        try:
            self.socket.send_json(message)
        except Exception:
            pass

    def get_response(self, timeout=10):
        start = time.time()
        while time.time() - start < timeout:
            if self._response_queue:
                return self._response_queue.pop(0)
            time.sleep(0.05)
        return None

    def close(self):
        self._running = False
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2)
        try:
            self.socket.close()
        except Exception:
            pass
        try:
            self.context.term()
        except Exception:
            pass


def get_all_positions(client):
    """Request all open positions from bridge (no symbol filter)."""
    client.send("ORDER_INFO", {})
    response = client.get_response(timeout=5)
    if response and response.get("success"):
        return response.get("positions", [])
    return []


def close_position(client, ticket):
    client.send("POSITION_CLOSE", {"ticket": ticket, "volume": 0})
    return client.get_response(timeout=10)


def fmt_pos(p):
    return f"#{p.get('ticket')} {p.get('symbol', '')} {p.get('type')} {p.get('volume')} lots P&L=${p.get('profit', 0):+.2f} Magic={p.get('magic',0)}"


def main():
    print("=" * 60)
    print("ORDER MONITOR v2 - Theo dõi tất cả positions và đóng khi đạt P&L target")
    print("=" * 60)

    # Prompt for PnL target
    try:
        raw = input("Enter P&L target to close each position (default 1.0): ").strip()
        if raw == "":
            pnl_target = float(1.0)
        else:
            pnl_target = float(raw)
    except (KeyboardInterrupt, EOFError):
        print("\nAborted by user")
        return
    except Exception:
        print("Invalid input, using default target = 1.0")
        pnl_target = 1.0

    try:
        raw_int = input(f"Poll interval seconds [default {CHECK_INTERVAL}]: ").strip()
        if raw_int == "":
            interval = CHECK_INTERVAL
        else:
            interval = float(raw_int)
    except Exception:
        interval = CHECK_INTERVAL

    print(f"Monitoring all open positions. Close when P&L >= ${pnl_target:.2f}")
    print("Press Ctrl+C to stop.")

    client = OrderClient(port=PORT)

    total_closed = 0
    total_pnl = 0.0

    # load batch_info to include closed pnl per magic
    batch_info = load_batch_info()

    try:
        while True:
            positions = get_all_positions(client)
            now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

            if positions:

                # Group positions by magic (batch)
                groups = {}
                for p in positions:
                    magic = p.get('magic', 0)
                    groups.setdefault(magic, []).append(p)

                print(f"[{now}] Found {len(positions)} open position(s) in {len(groups)} batch(es)")
                # Print one line per batch summarizing positions
                for magic, grp in groups.items():
                    count = len(grp)
                    total_pnl_batch = 0.0
                    tickets_summary = []
                    for p in grp:
                        try:
                            pf = float(p.get('profit', 0) or 0)
                        except Exception:
                            pf = 0.0
                        total_pnl_batch += pf
                        tickets_summary.append(f"#{p.get('ticket')}:{pf:+.2f}")

                    # read closed pnl/count for this magic from batch_info
                    closed_info = batch_info.get(str(magic), {}) if batch_info else {}
                    closed_count = closed_info.get('closed_count', 0)
                    closed_pnl = closed_info.get('closed_pnl', 0.0)

                    line = (
                        f"  Magic={magic} | count={count} | open_pnl={total_pnl_batch:+.2f} "
                        f"| closed_pnl={closed_pnl:+.2f} closed_count={closed_count} "
                        f"| tickets=[{', '.join(tickets_summary)}]"
                    )
                    print(line)


                    # For any individual position within this batch that meets pnl_target, close it
                    for p in grp:
                        try:
                            profit = float(p.get('profit', 0) or 0)
                        except Exception:
                            profit = 0.0

                        if profit >= pnl_target:
                            ticket = p.get('ticket')
                            print(f"    -> Closing position #{ticket} (P&L={profit:+.2f} >= {pnl_target:.2f})...")
                            resp = close_position(client, ticket)
                            if resp and resp.get('success'):
                                print(f"       CLOSED: #{ticket}")
                                total_closed += 1
                                total_pnl += profit
                                # update batch_info for this magic
                                batch_info = update_batch_closed_info(batch_info, magic, 1, profit)
                            else:
                                err = resp.get('error_message') if resp else 'no response'
                                print(f"       FAILED to close #{ticket}: {err}")

            else:
                print(f"[{now}] No open positions.")


            time.sleep(interval)

    except KeyboardInterrupt:
        print("\nStopped by user")

    finally:
        print("\nSummary:")
        print(f"  Total positions closed: {total_closed}")
        print(f"  Total P&L realized from closed positions: ${total_pnl:+.2f}")
        client.close()


if __name__ == '__main__':
    main()
