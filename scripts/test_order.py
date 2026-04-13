"""Test script: Open multiple orders and track all positions."""

import zmq
import json
import time
import uuid
import threading


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


def send_order(client, symbol, order_type, volume, comment=""):
    """Send order and return ticket if success."""
    request_id = str(uuid.uuid4())[:8]
    
    client.send("ORDER_SEND", {
        "symbol": symbol,
        "volume": volume,
        "order_type": order_type,
        "price": 0,
        "stop_loss": None,
        "take_profit": None,
        "comment": comment or f"Test {request_id}",
        "magic": 12345,
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


def main():
    print("=" * 60)
    print("MULTI-ORDER TRACKER")
    print("=" * 60)
    
    # Input parameters
    print("\nConfiguration:")
    order_type = get_input("  Order type (BUY/SELL) [default: BUY]: ", "BUY").upper()
    num_orders = get_input("  Number of orders [default: 5]: ", 5, int)
    volume = get_input("  Volume per order [default: 0.01]: ", 0.01, float)
    pnl_target = get_input("  P&L target per order [default: 1.0]: ", 1.0, float)
    
    # Validate
    if order_type not in ["BUY", "SELL"]:
        print("Invalid order type! Use BUY or SELL.")
        return
    
    print("\n" + "=" * 60)
    print(f"Opening {num_orders} {order_type} orders @ {volume} lots each")
    print(f"Will close when P&L > ${pnl_target}")
    print("=" * 60)
    
    client = OrderClient(port=5556)
    symbol = "GOLD"
    check_interval = 0.5
    
    # Open orders
    print("\n[1] Opening orders...\n")
    
    open_tickets = []
    
    for i in range(num_orders):
        ticket = send_order(client, symbol, order_type, volume, f"Order {i+1}")
        if ticket:
            open_tickets.append(ticket)
            print(f"  Order {i+1}: {order_type} {volume} lots -> Ticket #{ticket}")
        else:
            print(f"  Order {i+1}: FAILED")
    
    if not open_tickets:
        print("\nNo orders opened! Exiting.")
        client.close()
        return
    
    print(f"\n  Opened {len(open_tickets)} orders: {open_tickets}")
    print(f"\n[2] Monitoring P&L (Press Ctrl+C to stop)...\n")
    
    # Track positions
    i = 0
    closed_tickets = []
    total_pnl_achieved = 0.0  # Track total P&L achieved
    
    try:
        while True:
            positions = get_all_positions(client, symbol)
            
            # Filter only our tickets
            active_positions = [p for p in positions if p.get('ticket') in open_tickets]
            
            if not active_positions:
                print("\nAll positions closed!")
                break
            
            i += 1
            elapsed = round(i * check_interval, 1)
            
            # Calculate current P&L
            total_pnl = sum(p.get('profit', 0) for p in active_positions)
            
            # Print individual positions on one line
            status_parts = []
            for pos in active_positions:
                t = pos.get('ticket')
                pnl = pos.get('profit', 0)
                status_parts.append(f"#{t}: ${pnl:+.2f}")
            
            print(f"[{elapsed:6.1f}s] {', '.join(status_parts)}")
            print(f"         Total P&L: ${total_pnl:+.2f}")
            
            # Check if any position has P&L > target
            positions_to_close = [p for p in active_positions if p.get('profit', 0) > pnl_target]
            
            if positions_to_close:
                print(f"\n>>> Closing positions with P&L > ${pnl_target}...\n")
                
                for pos in positions_to_close:
                    ticket = pos.get('ticket')
                    pnl = pos.get('profit', 0)
                    
                    print(f"  Closing #{ticket} (P&L: ${pnl:.2f})")
                    resp = close_position(client, ticket)
                    
                    if resp and resp.get('success'):
                        print(f"    SUCCESS! {resp.get('comment', '')}")
                        closed_tickets.append(ticket)
                        total_pnl_achieved += pnl  # Add to total P&L
                    else:
                        print(f"    FAILED: {resp.get('error_message', 'Unknown') if resp else 'No response'}")
                
                # Remove closed tickets from list
                for t in closed_tickets:
                    if t in open_tickets:
                        open_tickets.remove(t)
                closed_tickets = []
                
                if not open_tickets:
                    break
            
            time.sleep(check_interval)
    
    except KeyboardInterrupt:
        print("\n\nInterrupted by user!")
        print("\nClosing all remaining positions...")
        
        for ticket in open_tickets:
            resp = close_position(client, ticket)
            if resp and resp.get('success'):
                # Try to get P&L from response
                comment = resp.get('comment', '')
                if 'P&L:' in str(comment):
                    pnl_str = comment.split('P&L: $')[-1].strip()
                    try:
                        total_pnl_achieved += float(pnl_str)
                    except:
                        pass
                print(f"  #{ticket}: CLOSED")
            else:
                print(f"  #{ticket}: FAILED")
    
    print("\n" + "=" * 60)
    print("TRACKING COMPLETED")
    print("=" * 60)
    print(f"\nTotal P&L Achieved: ${total_pnl_achieved:+.2f}")
    print(f"Orders Closed: {num_orders - len(open_tickets)}/{num_orders}")
    print("=" * 60)
    
    client.close()


if __name__ == "__main__":
    main()