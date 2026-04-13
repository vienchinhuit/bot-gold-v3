"""Test script to subscribe to market data from port 5555."""

import zmq
import json
import sys


def main():
    # Connect to market data publisher (port 5555)
    context = zmq.Context()
    socket = context.socket(zmq.SUB)
    socket.connect("tcp://localhost:5555")
    
    # Subscribe to all messages
    socket.setsockopt(zmq.SUBSCRIBE, b"")
    
    print("=" * 50)
    print("MARKET DATA SUBSCRIBER")
    print("=" * 50)
    print()
    print("Listening for market data on tcp://localhost:5555")
    print("Press Ctrl+C to stop")
    print()
    
    try:
        message_count = 0
        while True:
            message = socket.recv_string()
            message_count += 1
            
            try:
                data = json.loads(message)
                msg_type = data.get('type', 'UNKNOWN')
                
                if msg_type == 'TICK':
                    tick = data.get('data', {})
                    print(
                        f"[{message_count}] TICK | {tick.get('symbol', 'N/A'):6} | "
                        f"BID: {tick.get('bid', 0):.2f} | "
                        f"ASK: {tick.get('ask', 0):.2f} | "
                        f"SPREAD: {tick.get('spread_points', 0):.0f} pts"
                    )
                elif msg_type == 'HEARTBEAT':
                    print(f"[{message_count}] HEARTBEAT | {data.get('timestamp', '')}")
                else:
                    print(f"[{message_count}] {msg_type} | {data}")
                    
            except json.JSONDecodeError:
                print(f"[{message_count}] RAW: {message[:100]}...")
                
    except KeyboardInterrupt:
        print()
        print()
        print("=" * 50)
        print(f"Received {message_count} messages")
        print("Subscriber stopped")
        print("=" * 50)
    finally:
        socket.close()
        context.term()


if __name__ == "__main__":
    main()