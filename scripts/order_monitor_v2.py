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

    try:
        while True:
            positions = get_all_positions(client)
            now = datetime.now().strftime("%Y-%m-%d %H:%M:%S")

            if positions:
                print(f"[{now}] Found {len(positions)} open position(s)")
                for p in positions:
                    try:
                        profit = float(p.get('profit', 0) or 0)
                    except Exception:
                        profit = 0.0

                    print("  ", fmt_pos(p))

                    if profit >= pnl_target:
                        ticket = p.get('ticket')
                        print(f"    -> Closing position #{ticket} (P&L={profit:+.2f} >= {pnl_target:.2f})...")
                        resp = close_position(client, ticket)
                        if resp and resp.get('success'):
                            print(f"       CLOSED: #{ticket}")
                            total_closed += 1
                            total_pnl += profit
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
