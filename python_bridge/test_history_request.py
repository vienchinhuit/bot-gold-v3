#!/usr/bin/env python3
"""
Test client for requesting HISTORY from python_bridge.

Usage:
  python python_bridge/test_history_request.py --addr tcp://127.0.0.1:5556 --symbol GOLD --count 500 --timeout 5

This script connects with a DEALER socket to the bridge OrderReceiver (ROUTER) and sends a
{"type":"HISTORY","data":{"symbol":...,"count":...}} request. It waits for a JSON
response (array of candles) and prints summary.
"""

import zmq
import json
import argparse
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description="Send HISTORY request to python_bridge")
    parser.add_argument('--addr', default='tcp://127.0.0.1:5556', help='Order router address')
    parser.add_argument('--symbol', default='GOLD', help='Symbol to request')
    parser.add_argument('--count', type=int, default=500, help='Number of candles')
    parser.add_argument('--timeout', type=float, default=5.0, help='Timeout seconds to wait for reply')
    parser.add_argument('--save', default='', help='If set, save raw response to this file')
    args = parser.parse_args()

    ctx = zmq.Context()
    sock = ctx.socket(zmq.DEALER)
    # set a random identity for easier debugging
    identity = uuid.uuid4().hex[:8].encode()
    sock.setsockopt(zmq.IDENTITY, identity)
    sock.setsockopt(zmq.RCVTIMEO, int(args.timeout * 1000))
    sock.setsockopt(zmq.SNDTIMEO, int(args.timeout * 1000))

    print(f"Connecting to {args.addr} as {identity.decode()}")
    sock.connect(args.addr)

    req = {"type": "HISTORY", "data": {"symbol": args.symbol, "count": args.count}}
    msg = json.dumps(req)
    print(f"Sending HISTORY request: {msg}")

    try:
        sock.send_string(msg)
    except Exception as e:
        print(f"Send failed: {e}")
        return

    try:
        # Wait for reply
        resp = sock.recv_string()
    except zmq.error.Again:
        print(f"No reply within {args.timeout}s")
        return
    except Exception as e:
        print(f"Recv failed: {e}")
        return

    print("Raw reply:")
    print(resp[:500] + ("..." if len(resp) > 500 else ""))

    if args.save:
        try:
            with open(args.save, 'w', encoding='utf-8') as f:
                f.write(resp)
            print(f"Saved raw response to {args.save}")
        except Exception as e:
            print(f"Failed to save: {e}")

    # Try parse JSON
    try:
        j = json.loads(resp)
    except Exception as e:
        print(f"Failed to parse JSON reply: {e}")
        return

    if isinstance(j, list):
        print(f"Received list of {len(j)} candles")
        if len(j) > 0:
            print("First candle:")
            print(json.dumps(j[0], indent=2, ensure_ascii=False))
            if len(j) > 1:
                print("Last candle:")
                print(json.dumps(j[-1], indent=2, ensure_ascii=False))
    elif isinstance(j, dict):
        print("Received dict response:")
        print(json.dumps(j, indent=2, ensure_ascii=False))
    else:
        print("Received unexpected JSON type:", type(j))


if __name__ == '__main__':
    main()
