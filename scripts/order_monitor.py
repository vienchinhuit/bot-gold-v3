"""Order Monitor: Theo doi va dong lenh theo tung position.

NEU mot position bien mat khoi danh sach (khong phai do dong tay),
co the MT5 da tu dong do TP/SL - se gui notification cho Slack.

Fix: Phat hien positions bien mat va gui Slack notification
"""

import zmq
import json
import time
import threading
import os
from collections import defaultdict
from datetime import datetime

try:
    import MetaTrader5 as mt5
except Exception:
    mt5 = None

# Cau hinh
SYMBOL = "GOLD"
PORT = 5556
CHECK_INTERVAL = 0.2  # Gian

# Slack notification settings (ZMQ PUB)
SLACK_NOTIFY_PORT = 5557  # Port de gui close notifications den Rust engine (0 = disable)


class SlackNotifier:
    """"Gui notification qua ZMQ den Rust engine de forward qua Slack."""
    
    def __init__(self, port=5557):
        self.port = port
        self.context = None
        self.socket = None
        if port > 0:
            try:
                self.context = zmq.Context()
                self.socket = self.context.socket(zmq.PUB)
                # Ensure messages are not queued after close
                self.socket.setsockopt(zmq.LINGER, 0)
                self.socket.bind(f"tcp://*:{port}")
                # Give subscriber a moment to connect (increase to reduce lost messages during startup)
                time.sleep(1.0)
                print(f"Slack notifier: TCP *:{port} (PUB)")
            except Exception as e:
                print(f"Slack notifier: Failed to bind port {port}: {e}")
                self.socket = None
    
    def send_close_notify(self, ticket, direction, volume, price, profit, magic, reason="MANUAL"):
        """Gui notification khi position duoc dong.
        
        Args:
            ticket: Position ticket
            direction: BUY/SELL
            volume: Lot size
            price: Open price
            profit: Realized P&L
            magic: Magic number
            reason: CLOSE_REASON - MANUAL, TP, SL, BATCH
        """
        # Format message
        msg = f"CLOSE_NOTIFY|{ticket}|{direction}|{volume}|{price}|{profit}|{magic}|{reason}"

        # If we have a PUB socket, publish via ZMQ (preferred)
        if self.socket:
            try:
                self.socket.send_string(msg)
                reason_label = "TP" if reason == "TP" else ("SL" if reason == "SL" else ("BATCH" if reason == "BATCH" else "CLOSE"))
                print(f"  [SLACK] Notified #{ticket} {direction} {volume} lots @{price} P&L=${profit:+.2f} [{reason_label}]")
                return
            except Exception as e:
                print(f"  [SLACK] Failed to send ZMQ notify: {e}")

        # Fallback: send directly to Slack webhook if provided via environment variable
        webhook = os.environ.get('SLACK_WEBHOOK', '').strip()
        channel = os.environ.get('SLACK_CHANNEL', '').strip()
        if webhook:
            try:
                # Build simple Slack payload (use default channel if channel not prefixed)
                payload = {
                    "text": f"POSITION CLOSED | #{ticket} {direction} {volume} lots @ {price:.2f} | P&L: ${profit:+.2f} | Reason: {reason}",
                }
                # If channel looks like '#... or @...', include it
                if channel and (channel.startswith('#') or channel.startswith('@')):
                    payload['channel'] = channel
                import requests
                resp = requests.post(webhook, json=payload, timeout=10)
                if resp.status_code >= 200 and resp.status_code < 300:
                    print(f"  [SLACK] Direct webhook sent for #{ticket} ({resp.status_code})")
                else:
                    print(f"  [SLACK] Direct webhook failed: {resp.status_code} {resp.text}")
            except Exception as e:
                print(f"  [SLACK] Direct webhook exception: {e}")
        else:
            print(f"  [SLACK] No ZMQ socket and no SLACK_WEBHOOK env var - cannot notify for #{ticket}")
    
    def close(self):
        if self.socket:
            self.socket.close()
        if self.context:
            self.context.term()


STOP_LOSS = 0  # Dat >0 de tu dong khi lo (VD: 10 = dong khi lo $10), dat 0 de tat
BATCH_PROFIT_TARGET = 1000.0  # Tong PNL dat target nay se dong toan bo batch (dat 0 de tat)
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


def normalize_position_snapshot(p):
    return {
        'ticket': p.get('ticket'),
        'symbol': p.get('symbol'),
        'type': p.get('type', 'UNKNOWN'),
        'volume': p.get('volume', 0),
        'price_open': p.get('price_open', 0),
        'profit': p.get('profit', 0),
        'magic': p.get('magic', 0),
        'comment': p.get('comment', ''),
        'stop_loss': p.get('stop_loss', 0),
        'take_profit': p.get('take_profit', 0),
        'time': p.get('time'),
    }


def map_reason_from_deal_reason(deal_reason):
    if mt5 is None:
        return "UNKNOWN"

    reason_map = {
        getattr(mt5, "DEAL_REASON_SL", None): "SL",
        getattr(mt5, "DEAL_REASON_TP", None): "TP",
        getattr(mt5, "DEAL_REASON_CLIENT", None): "MANUAL",
        getattr(mt5, "DEAL_REASON_EXPERT", None): "MANUAL",
        getattr(mt5, "DEAL_REASON_MOBILE", None): "MANUAL",
        getattr(mt5, "DEAL_REASON_WEB", None): "MANUAL",
        getattr(mt5, "DEAL_REASON_SO", None): "SL",
    }
    return reason_map.get(deal_reason, "UNKNOWN")


def get_position_history_reason(client, ticket):
    """Attempt to retrieve close reason and PnL for a ticket via python_bridge (OrderClient).
    Falls back to local MetaTrader5 history if python_bridge not responding or not available.
    Returns dict with keys: reason, profit, price, volume or None.
    """
    # Try python_bridge first via OrderClient
    try:
        if client:
            client.send("POSITION_GET", {"ticket": ticket})
            resp = client.get_response(timeout=5)
            if resp and resp.get('success') and resp.get('data'):
                return resp.get('data')
    except Exception:
        pass

    # Fallback to local MT5 module if available
    if mt5 is None:
        return None

    try:
        end_time = time.time() + 60
        start_time = end_time - 7 * 24 * 3600
        deals = mt5.history_deals_get(start_time, end_time)
        if not deals:
            return None

        matched = [d for d in deals if getattr(d, "position_id", None) == ticket or getattr(d, "position", None) == ticket]
        if not matched:
            return None

        matched.sort(key=lambda d: getattr(d, "time", 0))
        for deal in reversed(matched):
            entry = getattr(deal, "entry", None)
            if entry in (getattr(mt5, "DEAL_ENTRY_OUT", None), getattr(mt5, "DEAL_ENTRY_OUT_BY", None)):
                return {
                    'reason': map_reason_from_deal_reason(getattr(deal, "reason", None)),
                    'profit': float(getattr(deal, "profit", 0.0) or 0.0),
                    'price': float(getattr(deal, "price", 0.0) or 0.0),
                    'volume': float(getattr(deal, "volume", 0.0) or 0.0),
                }
    except Exception:
        return None

    return None


def get_position_detail(client, ticket):
    """Query MT5 for position details including profit info.
    
    Returns dict with position info or None if failed.
    """
    client.send("POSITION_GET", {"ticket": ticket})
    resp = client.get_response(timeout=5)
    if resp and resp.get("success"):
        return resp
    return None


def load_batch_info():
    """Doc batch info tu file JSON."""
    try:
        if os.path.exists(BATCH_INFO_FILE):
            with open(BATCH_INFO_FILE, 'r') as f:
                return json.load(f)
    except:
        pass
    return {}


def save_batch_info(batch_info):
    """Luu batch info vao file JSON."""
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
    """Cap nhat thong tin da dong cho batch."""
    magic_str = str(magic)
    if magic_str not in batch_info:
        batch_info[magic_str] = {}
    batch_info[magic_str]['closed_count'] = batch_info[magic_str].get('closed_count', 0) + closed_count
    batch_info[magic_str]['closed_pnl'] = batch_info[magic_str].get('closed_pnl', 0.0) + closed_pnl
    save_batch_info(batch_info)
    return batch_info


def clear_batch_closed_info(batch_info, magic):
    """Xoa thong tin da dong cua batch."""
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
    print("ORDER MONITOR v3 - Theo doi va dong theo tung position")
    print("=" * 60)
    print(f"  Symbol: {SYMBOL}")
    print(f"  Check Interval: {CHECK_INTERVAL}s")
    print(f"  Slack Notify Port: {SLACK_NOTIFY_PORT}")
    print("=" * 60)
    print()
    
    client = OrderClient(port=PORT)
    slack_notifier = SlackNotifier(port=SLACK_NOTIFY_PORT)
    
    # Reset batch info khi khoi chay
    batch_info = load_batch_info()
    for magic in batch_info:
        batch_info[magic]['closed_count'] = 0
        batch_info[magic]['closed_pnl'] = 0.0
    save_batch_info(batch_info)
    print("Da reset batch info!")
    
    # Lay positions hien tai
    print("Dang lay positions hien tai...")
    positions = get_all_positions(client, SYMBOL)
    
    # === TRACKING VARIABLES ===
    # Track known positions to detect when they disappear (MT5 TP/SL auto-close)
    known_tickets = set()  # Tickets we've seen
    confirmed_closed_tickets = set()  # Tickets we explicitly closed (for TP/SL detection)
    
    if not positions:
        print("Khong co positions nao!")
        print("Dang cho positions moi...")
        print("Nhan Ctrl+C de thoat")
    else:
        print(f"Tim thay {len(positions)} positions:")
        for p in positions:
            magic, target = parse_comment(p.get('comment', ''))
            ticket = p.get('ticket')
            known_tickets.add(ticket)
            print(f"  #{ticket}: Magic={magic} | Target=${target} | P&L: ${p.get('profit', 0):+.2f}")
    
    print()
    print("Bat dau theo doi... (Ctrl+C de dung)")
    print("-" * 60)
    
    # Keep a copy of tickets seen previously to detect NEW positions
    last_seen_tickets = set(known_tickets)

    total_pnl_achieved = 0.0
    total_closed = 0
    global_closed_pnl = 0.0
    global_closed_count = 0
    known_positions = {}
    processed_closed_tickets = set()
    # Theo doi lo/lai rieng
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
            
            current_tickets = set()
            for p in positions:
                ticket = p.get('ticket')
                current_tickets.add(ticket)
                known_tickets.add(ticket)
                known_positions[ticket] = normalize_position_snapshot(p)

            missing_tickets = known_tickets - current_tickets - confirmed_closed_tickets - processed_closed_tickets

            if missing_tickets:
                print(f"\n!!! DETECTED {len(missing_tickets)} MISSING POSITION(S):")
                for ticket in sorted(missing_tickets):
                    snapshot = known_positions.get(ticket, {})
                    hist = get_position_history_reason(client, ticket)
                    reason = hist.get('reason') if hist else None
                    profit = hist.get('profit') if hist and hist.get('profit') is not None else float(snapshot.get('profit', 0) or 0)
                    price = hist.get('price') if hist and hist.get('price') is not None else float(snapshot.get('price_open', 0) or 0)
                    volume = hist.get('volume') if hist and hist.get('volume') is not None else float(snapshot.get('volume', 0) or 0)
                    direction = snapshot.get('type', 'UNKNOWN')
                    magic = snapshot.get('magic', 0)

                    if not reason:
                        if snapshot.get('take_profit'):
                            reason = 'TP' if profit >= 0 else 'SL'
                        elif snapshot.get('stop_loss'):
                            reason = 'SL' if profit <= 0 else 'TP'
                        else:
                            reason = 'UNKNOWN'

                    print(f"  #{ticket}: Auto-closed as {reason} | P&L: ${profit:+.2f}")

                    slack_notifier.send_close_notify(
                        ticket=ticket,
                        direction=direction,
                        volume=volume,
                        price=price,
                        profit=profit,
                        magic=magic,
                        reason=reason
                    )

                    processed_closed_tickets.add(ticket)
                    known_positions.pop(ticket, None)
                    global_closed_pnl += profit
                    global_closed_count += 1
                    if profit >= 0:
                        profit_closed_count += 1
                        profit_closed_pnl += profit
                    else:
                        loss_closed_count += 1
                        loss_closed_pnl += profit
            
            # === END MISSING POSITION DETECTION ===
            
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
                
                # Doc batch info tu file JSON
                batch_info = load_batch_info()
                
                # Build batch/group aggregates (no periodic printing)
                batch_info = load_batch_info()
                total_closed_pnl = 0.0
                total_closed_count = 0
                total_pnl_all = 0.0
                batches_to_close = []  # Cac batch can dong toan bo
                
                for magic, pos_list in magic_groups.items():
                    pnl = sum(p.get('profit', 0) for p in pos_list)
                    target = batch_info.get(str(magic), {}).get('pnl_target', '?')
                    closed_count = batch_info.get(str(magic), {}).get('closed_count', 0)
                    closed_pnl = batch_info.get(str(magic), {}).get('closed_pnl', 0.0)
                    total_batch_pnl = pnl + closed_pnl  # Tong PNL = dang mo + da dong
                    total_closed_pnl += closed_pnl
                    total_closed_count += closed_count
                    total_pnl_all += total_batch_pnl
                    
                    # Kiem tra neu batch dat target thi dong toan bo
                    if BATCH_PROFIT_TARGET > 0 and total_batch_pnl >= BATCH_PROFIT_TARGET:
                        batches_to_close.append({'magic': magic, 'pos_list': pos_list, 'total_pnl': total_batch_pnl})

                # If new positions appeared, print concise info about them
                new_positions = current_tickets - last_seen_tickets
                if new_positions:
                    print(f"\n+++ New positions detected: {len(new_positions)}")
                    for t in sorted(new_positions):
                        p = known_positions.get(t, {})
                        try:
                            print(f"  NEW #{t}: {p.get('type','?')} {p.get('volume',0)} lots @ {p.get('price_open',0):.2f} | P&L: ${p.get('profit',0):+.2f} | Magic: {p.get('magic',0)}")
                        except Exception:
                            print(f"  NEW #{t}: details unavailable")
                    last_seen_tickets.update(new_positions)
                
                # Check tung position trong tung batch
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
                
                # Dong cac positions dat target
                if positions_to_close:
                    print(f"\n>>> Dong {len(positions_to_close)} position(s) dat target...")
                    
                    for pos in positions_to_close:
                        print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} >= Target ${pos['target']:.2f}")
                        resp = close_position(client, pos['ticket'])
                        
                        if resp and resp.get('success'):
                            print(f"    SUCCESS!")
                            # Gui Slack notification
                            slack_notifier.send_close_notify(
                                ticket=pos['ticket'],
                                direction=pos.get('type', 'UNKNOWN'),
                                volume=pos.get('volume', 0),
                                price=pos.get('price_open', 0),
                                profit=pos['profit'],
                                magic=pos.get('magic', 0),
                                reason="MANUAL"
                            )
                            
                            # Track as explicitly closed
                            confirmed_closed_tickets.add(pos['ticket'])
                            processed_closed_tickets.add(pos['ticket'])
                            
                            total_pnl_achieved += pos['profit']
                            total_closed += 1
                            global_closed_pnl += pos['profit']
                            global_closed_count += 1
                            
                            # Cap nhat lai/lo
                            if pos['profit'] >= 0:
                                profit_closed_count += 1
                                profit_closed_pnl += pos['profit']
                            else:
                                loss_closed_count += 1
                                loss_closed_pnl += pos['profit']
                            
                            # Cap nhat thong tin da dong cho batch
                            batch_info = update_batch_closed_info(batch_info, pos['magic'], 1, pos['profit'])
                        else:
                            print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                
                # Dong cac positions cham Stop Loss
                if positions_to_close_sl:
                    print(f"\n>>> Dong {len(positions_to_close_sl)} position(s) cham SL (<= -${STOP_LOSS})...")
                    
                    for pos in positions_to_close_sl:
                        print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} <= -${STOP_LOSS} (SL)")
                        resp = close_position(client, pos['ticket'])
                        
                        if resp and resp.get('success'):
                            print(f"    SUCCESS!")
                            # Gui Slack notification
                            slack_notifier.send_close_notify(
                                ticket=pos['ticket'],
                                direction=pos.get('type', 'UNKNOWN'),
                                volume=pos.get('volume', 0),
                                price=pos.get('price_open', 0),
                                profit=pos['profit'],
                                magic=pos.get('magic', 0),
                                reason="SL"
                            )
                            
                            # Track as explicitly closed
                            confirmed_closed_tickets.add(pos['ticket'])
                            processed_closed_tickets.add(pos['ticket'])
                            
                            total_pnl_achieved += pos['profit']
                            total_closed += 1
                            global_closed_pnl += pos['profit']
                            global_closed_count += 1
                            
                            # Cap nhat lai/lo
                            if pos['profit'] >= 0:
                                profit_closed_count += 1
                                profit_closed_pnl += pos['profit']
                            else:
                                loss_closed_count += 1
                                loss_closed_pnl += pos['profit']
                            
                            # Cap nhat thong tin da dong cho batch
                            batch_info = update_batch_closed_info(batch_info, pos['magic'], 1, pos['profit'])
                        else:
                            print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                
                # Dong toan bo batch khi dat target PNL
                if batches_to_close:
                    for batch in batches_to_close:
                        magic = batch['magic']
                        pos_list = batch['pos_list']
                        total_batch_pnl = batch['total_pnl']
                        print(f"\n>>> BATCH Magic#{magic} dat target! Tong PNL: ${total_batch_pnl:+.2f} >= ${BATCH_PROFIT_TARGET}")
                        print(f"    Dong {len(pos_list)} position(s) con lai...")
                        
                        closed_batch_pnl = 0.0
                        closed_batch_count = 0
                        for pos in pos_list:
                            print(f"  Position #{pos['ticket']}: P&L ${pos['profit']:.2f} (dong batch)")
                            resp = close_position(client, pos['ticket'])
                            
                            if resp and resp.get('success'):
                                print(f"    SUCCESS!")
                                # Gui Slack notification
                                slack_notifier.send_close_notify(
                                    ticket=pos['ticket'],
                                    direction=pos.get('type', 'UNKNOWN'),
                                    volume=pos.get('volume', 0),
                                    price=pos.get('price_open', 0),
                                    profit=pos['profit'],
                                    magic=pos.get('magic', 0),
                                    reason="BATCH"
                                )
                                
                                # Track as explicitly closed
                                confirmed_closed_tickets.add(pos['ticket'])
                                processed_closed_tickets.add(pos['ticket'])

                                total_pnl_achieved += pos['profit']
                                total_closed += 1
                                global_closed_pnl += pos['profit']
                                global_closed_count += 1
                                closed_batch_pnl += pos['profit']
                                closed_batch_count += 1
                                
                                # Cap nhat lai/lo
                                if pos['profit'] >= 0:
                                    profit_closed_count += 1
                                    profit_closed_pnl += pos['profit']
                                else:
                                    loss_closed_count += 1
                                    loss_closed_pnl += pos['profit']
                            else:
                                print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                        
                        # Cong don PNL da dong cua batch vao batch_info
                        batch_info = update_batch_closed_info(batch_info, magic, closed_batch_count, closed_batch_pnl)
                        # Reset batch ve tu dau
                        batch_info = clear_batch_closed_info(batch_info, magic)
                        print(f"    Batch Magic#{magic} da reset!")
            else:
                # No positions currently - quiet mode: only notify on events
                pass
            
            time.sleep(CHECK_INTERVAL)
    
    except KeyboardInterrupt:
        print("\n\nDung theo doi!")
        
        # Dong cac positions con lai
        positions = get_all_positions(client, SYMBOL)
        if positions:
            print("Dong cac positions con lai...")
            for p in positions:
                ticket = p.get('ticket')
                resp = close_position(client, ticket)
                if resp and resp.get('success'):
                    print(f"  #{ticket}: CLOSED")
                    confirmed_closed_tickets.add(ticket)
                    processed_closed_tickets.add(ticket)
                else:
                    print(f"  #{ticket}: FAILED")
    
    print("\n" + "=" * 60)
    print(f"Tong P&L da dat: ${total_pnl_achieved:+.2f}")
    print(f"So positions da dong: {total_closed}")
    print(f"Tong P&L tat ca (bao gom TP/SL): ${global_closed_pnl:+.2f}")
    print(f"Tong so positions: {global_closed_count}")
    print("=" * 60)
    
    client.close()
    slack_notifier.close()


if __name__ == "__main__":
    main()
