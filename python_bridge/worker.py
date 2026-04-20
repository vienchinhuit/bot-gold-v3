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
    from datetime import date
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
            elif msg_type == MessageType.POSITION_CLOSE_BATCH.value:
                return self._handle_position_close_batch(data)
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
    
    def _handle_position_close_batch(self, data: dict):
        """Handle batch position close request - parallel execution."""
        tickets = data.get('tickets', [])
        max_workers = int(data.get('max_workers', 10))
        save_history = data.get('save_history', True)  # Mặc định lưu history
        
        if not tickets:
            return json.dumps({
                'success': False,
                'error': 'No tickets provided',
                'closed': 0,
                'failed': 0
            })
        
        self._order_logger.info(
            f"Processing POSITION_CLOSE_BATCH: {len(tickets)} positions, "
            f"max_workers={max_workers}"
        )
        
        # Lấy thông tin positions TRƯỚC KHI đóng (để lưu history)
        positions_info = {}
        for ticket in tickets:
            pos_list = self.mt5.get_positions()
            for pos in pos_list:
                if pos['ticket'] == ticket:
                    positions_info[ticket] = {
                        'symbol': pos['symbol'],
                        'type': pos['type'],
                        'volume': pos['volume'],
                        'price_open': pos['price_open'],
                        'profit': pos['profit'],
                        'magic': pos['magic'],
                        'comment': pos['comment']
                    }
                    break
        
        result = self.mt5.close_positions_parallel(tickets, max_workers)
        
        if result['success']:
            self._order_logger.info(
                f"BATCH CLOSE SUCCESS: {result['closed']}/{len(tickets)} closed"
            )
        else:
            self._order_logger.warning(
                f"BATCH CLOSE PARTIAL: {result['closed']} OK, {result['failed']} failed"
            )
        
        # Lưu history cho các position đã đóng thành công
        if save_history and result.get('results'):
            self._save_batch_history(result['results'], positions_info)
        
        return json.dumps(result)
    
    def _save_batch_history(self, results: list, positions_info: dict):
        """Lưu batch close vào history file."""
        try:
            import os
            import json as json_module
            
            # Xác định đường dẫn history file
            script_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
            history_file = os.path.join(script_dir, "scripts", "order_history.json")
            
            # Đọc history hiện tại
            history = []
            if os.path.exists(history_file):
                try:
                    with open(history_file, 'r', encoding='utf-8') as f:
                        history = json_module.load(f)
                        if not isinstance(history, list):
                            history = []
                except (json_module.JSONDecodeError, FileNotFoundError):
                    history = []
            
            # Thêm các records mới
            for res in results:
                if res.get('success'):
                    ticket = res.get('ticket')
                    pos_info = positions_info.get(ticket, {})
                    
                    record = {
                        'id': datetime.now().strftime('%Y%m%d%H%M%S%f'),
                        'ticket': ticket,
                        'symbol': pos_info.get('symbol', 'GOLD'),
                        'type': pos_info.get('type', 'UNKNOWN'),
                        'volume': pos_info.get('volume', 0),
                        'price_open': pos_info.get('price_open', 0),
                        'price_close': res.get('price', 0),
                        'pnl': pos_info.get('profit', 0),
                        'magic': pos_info.get('magic', 0),
                        'comment': pos_info.get('comment', ''),
                        'close_mode': 'batch',
                        'closed_at': datetime.now().isoformat(),
                        'date': date.today().isoformat()
                    }
                    history.insert(0, record)
            
            # Lưu lại
            os.makedirs(os.path.dirname(history_file), exist_ok=True)
            with open(history_file, 'w', encoding='utf-8') as f:
                json_module.dump(history, f, indent=2, ensure_ascii=False)
            
            self._order_logger.info(f"Saved {len([r for r in results if r.get('success')])} records to history")
            
        except Exception as e:
            self._order_logger.error(f"Failed to save history: {e}")
    
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