"""Worker threads for data collection and processing."""

import threading
import time
import json
from datetime import datetime
from typing import Optional, Callable

# Support both relative and absolute imports
if __package__:
    from .mt5_connector import MT5Connector
    from .zmq_publisher import ZMQManager
    from .models import MessageType, OrderRequest, OrderResponse
    from .logger import get_system_logger, get_market_logger, get_order_logger
else:
    from mt5_connector import MT5Connector
    from zmq_publisher import ZMQManager
    from models import MessageType, OrderRequest, OrderResponse
    from logger import get_system_logger, get_market_logger, get_order_logger


class MarketDataWorker:
    """Worker thread for collecting and publishing market data."""
    
    def __init__(self, mt5: MT5Connector, zmq: ZMQManager, symbols: list = None,
                 interval: float = 0.5):
        self.mt5 = mt5
        self.zmq = zmq
        self.symbols = symbols or ["GOLD", "XAUUSD"]
        self.interval = interval  # seconds between updates
        
        self._thread: Optional[threading.Thread] = None
        self._running = False
        self._pause = threading.Event()
        self._pause.set()  # Not paused by default
        
        self._system_logger = get_system_logger()
        self._market_logger = get_market_logger()
    
    def start(self):
        """Start the worker thread."""
        if self._running:
            return
        
        self._system_logger.info(
            f"Starting MarketDataWorker for: {', '.join(self.symbols)}"
        )
        
        self._running = True
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        self._system_logger.info("MarketDataWorker started")
    
    def stop(self):
        """Stop the worker thread."""
        if not self._running:
            return
        
        self._system_logger.info("Stopping MarketDataWorker...")
        self._running = False
        self._pause.set()  # Ensure not paused
        
        if self._thread:
            self._thread.join(timeout=2.0)
        
        self._system_logger.info("MarketDataWorker stopped")
    
    def pause(self):
        """Pause the worker."""
        self._pause.clear()
        self._system_logger.info("MarketDataWorker paused")
    
    def resume(self):
        """Resume the worker."""
        self._pause.set()
        self._system_logger.info("MarketDataWorker resumed")
    
    def _run(self):
        """Main worker loop."""
        self._market_logger.info(
            f"MarketDataWorker running: interval={self.interval}s"
        )
        
        consecutive_errors = 0
        max_consecutive_errors = 10
        
        while self._running:
            self._pause.wait()  # Block if paused
            
            if not self._running:
                break
            
            try:
                # Get ticks for all symbols
                ticks = self.mt5.get_ticks(self.symbols)
                
                if ticks:
                    consecutive_errors = 0
                    
                    # Publish each tick
                    for symbol, tick in ticks.items():
                        if tick and tick.bid > 0 and tick.ask > 0:
                            self.zmq.market_publisher.send_tick(tick)
                            # Log market data
                            self._market_logger.info(
                                f"FEED: {symbol} BID={tick.bid:.2f} ASK={tick.ask:.2f} "
                                f"SPREAD={tick.spread_points:.0f} VOL={tick.volume}"
                            )
                else:
                    consecutive_errors += 1
                    if consecutive_errors % 5 == 0:
                        self._system_logger.warning(
                            f"No ticks received (consecutive: {consecutive_errors})"
                        )
                
            except Exception as e:
                consecutive_errors += 1
                self._system_logger.error(
                    f"MarketDataWorker error: {e} "
                    f"(consecutive: {consecutive_errors})"
                )
            
            # Sleep until next update
            time.sleep(self.interval)
            
            # Check for too many consecutive errors
            if consecutive_errors >= max_consecutive_errors:
                self._system_logger.critical(
                    "Too many consecutive errors, stopping worker"
                )
                self._running = False


class OrderWorker:
    """Worker for processing order commands from ZeroMQ."""
    
    def __init__(self, mt5: MT5Connector, zmq: ZMQManager):
        self.mt5 = mt5
        self.zmq = zmq
        
        self._running = False
        
        self._system_logger = get_system_logger()
        self._order_logger = get_order_logger()
    
    def start(self):
        """Start the order worker."""
        if self._running:
            return
        
        self._system_logger.info("Starting OrderWorker...")
        self._running = True
        
        # Set up message callback
        self.zmq.set_order_callback(self._on_order_message)
        
        self._system_logger.info("OrderWorker started")
    
    def stop(self):
        """Stop the order worker."""
        if not self._running:
            return
        
        self._system_logger.info("Stopping OrderWorker...")
        self._running = False
        self._system_logger.info("OrderWorker stopped")
    
    def _on_order_message(self, msg_type: str, data: dict):
        """Handle incoming order message. Returns response JSON string."""
        if not self._running:
            return json.dumps({"success": False, "error": "Worker stopped"})
        
        try:
            if msg_type == MessageType.ORDER_SEND.value:
                return self._handle_order_send(data)
            elif msg_type == MessageType.POSITION_CLOSE.value:
                return self._handle_position_close(data)
            elif msg_type == MessageType.POSITION_MODIFY.value:
                return self._handle_position_modify(data)
            elif msg_type == MessageType.ORDER_INFO.value:
                return self._handle_order_info(data)
            else:
                self._order_logger.warning(f"Unknown message type: {msg_type}")
                return json.dumps({"success": False, "error": f"Unknown message type: {msg_type}"})
                
        except Exception as e:
            self._system_logger.error(f"Order processing error: {e}")
            return json.dumps({"success": False, "error": str(e)})
    
    def _handle_order_send(self, data: dict):
        """Handle order send request."""
        self._order_logger.info(f"Processing ORDER_SEND: {data}")
        
        request = OrderRequest(
            ticket=None,
            symbol=data.get('symbol', ''),
            volume=float(data.get('volume', 0)),
            order_type=data.get('order_type', ''),
            price=float(data.get('price', 0)),
            stop_loss=float(data.get('stop_loss')) if data.get('stop_loss') else None,
            take_profit=float(data.get('take_profit')) if data.get('take_profit') else None,
            comment=data.get('comment'),
            magic=int(data.get('magic', 0)) if data.get('magic') else None,
            request_id=data.get('request_id', '')
        )
        
        response = self.mt5.send_order(request)
        
        if response.success:
            self._order_logger.info(
                f"ORDER SUCCESS: #{response.ticket} {request.order_type} "
                f"{request.volume} lots {request.symbol}"
            )
        else:
            self._order_logger.error(
                f"ORDER FAILED: {response.error_message}"
            )
        
        return response.to_json()
    
    def _handle_position_close(self, data: dict):
        """Handle position close request."""
        ticket = int(data.get('ticket', 0))
        volume = float(data.get('volume', 0))
        
        self._order_logger.info(f"Processing POSITION_CLOSE: ticket={ticket}")
        
        response = self.mt5.close_position(ticket, volume)
        
        if response.success:
            self._order_logger.info(f"CLOSE SUCCESS: #{ticket}")
        else:
            self._order_logger.error(f"CLOSE FAILED: {response.error_message}")
        
        return response.to_json()
    
    def _handle_position_modify(self, data: dict):
        """Handle position modify request."""
        ticket = int(data.get('ticket', 0))
        stop_loss = float(data.get('stop_loss')) if data.get('stop_loss') else None
        take_profit = float(data.get('take_profit')) if data.get('take_profit') else None
        
        self._order_logger.info(
            f"Processing POSITION_MODIFY: ticket={ticket}, "
            f"SL={stop_loss}, TP={take_profit}"
        )
        
        response = self.mt5.modify_position(ticket, stop_loss, take_profit)
        
        if response.success:
            self._order_logger.info(f"MODIFY SUCCESS: #{ticket}")
        else:
            self._order_logger.error(f"MODIFY FAILED: {response.error_message}")
        
        return response.to_json()
    
    def _handle_order_info(self, data: dict):
        """Handle order info request."""
        symbol = data.get('symbol')
        
        self._order_logger.info(f"Processing ORDER_INFO: symbol={symbol}")
        
        positions = self.mt5.get_positions(symbol)
        orders = self.mt5.get_orders(symbol)
        
        # Convert datetime to string for JSON serialization
        for pos in positions:
            if 'time' in pos and hasattr(pos['time'], 'isoformat'):
                pos['time'] = pos['time'].isoformat()
        
        for order in orders:
            if 'time' in order and hasattr(order['time'], 'isoformat'):
                order['time'] = order['time'].isoformat()
        
        response_data = {
            'success': True,
            'positions': positions,
            'orders': orders,
            'timestamp': datetime.now().isoformat()
        }
        
        self._order_logger.info(
            f"INFO: {len(positions)} positions, {len(orders)} orders"
        )
        
        return json.dumps(response_data)


class HeartbeatWorker:
    """Worker for sending periodic heartbeat messages."""
    
    def __init__(self, zmq: ZMQManager, interval: int = 30):
        self.zmq = zmq
        self.interval = interval  # seconds
        
        self._thread: Optional[threading.Thread] = None
        self._running = False
        
        self._system_logger = get_system_logger()
    
    def start(self):
        """Start the heartbeat worker."""
        if self._running:
            return
        
        self._running = True
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        self._system_logger.info(
            f"HeartbeatWorker started (interval: {self.interval}s)"
        )
    
    def stop(self):
        """Stop the heartbeat worker."""
        if not self._running:
            return
        
        self._running = False
        if self._thread:
            self._thread.join(timeout=2.0)
        self._system_logger.info("HeartbeatWorker stopped")
    
    def _run(self):
        """Main heartbeat loop."""
        while self._running:
            try:
                heartbeat = {
                    'type': MessageType.HEARTBEAT.value,
                    'timestamp': datetime.now().isoformat(),
                    'status': 'alive'
                }
                
                if self.zmq.market_publisher:
                    self.zmq.market_publisher.send_json(
                        MessageType.HEARTBEAT.value, heartbeat
                    )
                
                self._system_logger.debug("Heartbeat sent")
                
            except Exception as e:
                self._system_logger.error(f"Heartbeat error: {e}")
            
            # Sleep in small increments to allow faster shutdown
            for _ in range(self.interval):
                if not self._running:
                    break
                time.sleep(1)