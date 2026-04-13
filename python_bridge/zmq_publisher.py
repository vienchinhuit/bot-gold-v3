"""ZeroMQ publisher/subscriber for Python Bridge."""

import zmq
import json
import threading
from datetime import datetime
from typing import Optional, Callable, Dict

# Support both relative and absolute imports
if __package__:
    from .models import ZMQMessage, MessageType, TickData, OrderRequest, OrderResponse
    from .logger import get_system_logger, get_market_logger, get_order_logger
else:
    from models import ZMQMessage, MessageType, TickData, OrderRequest, OrderResponse
    from logger import get_system_logger, get_market_logger, get_order_logger


class ZMQPublisher:
    """ZeroMQ publisher for market data and order responses."""
    
    def __init__(self, address: str, port: int, name: str = "publisher"):
        self.address = f"{address}:{port}"
        self.port = port
        self.name = name
        self.context = None
        self.socket = None
        self._running = False
        self._lock = threading.Lock()
        self._logger = get_system_logger()
    
    def start(self):
        """Start the publisher (bind)."""
        if self._running:
            return
        
        self._logger.info(f"Starting {self.name} on {self.address}...")
        
        self.context = zmq.Context(io_threads=4)
        self.socket = self.context.socket(zmq.PUB)
        self.socket.setsockopt(zmq.LINGER, 1000)  # 1 second linger on close
        self.socket.setsockopt(zmq.SNDBUF, 1048576)  # 1MB send buffer
        self.socket.setsockopt(zmq.SNDHWM, 1000)  # High water mark
        
        try:
            self.socket.bind(self.address)
            self._running = True
            self._logger.info(f"{self.name} started on {self.address}")
        except zmq.ZMQError as e:
            self._logger.error(f"Failed to bind {self.name}: {e}")
            self.cleanup()
            raise
    
    def stop(self):
        """Stop the publisher."""
        if not self._running:
            return
        
        self._logger.info(f"Stopping {self.name}...")
        self._running = False
        self.cleanup()
        self._logger.info(f"{self.name} stopped")
    
    def cleanup(self):
        """Clean up ZMQ resources."""
        if self.socket:
            try:
                self.socket.close(linger=1000)
            except:
                pass
            self.socket = None
        
        if self.context:
            try:
                self.context.term()
            except:
                pass
            self.context = None
    
    def send(self, message: str):
        """Send a raw string message."""
        if not self._running:
            return
        
        with self._lock:
            try:
                self.socket.send_string(message, flags=zmq.NOBLOCK)
            except zmq.Again:
                self._logger.warning(f"{self.name}: Send queue full, message dropped")
            except zmq.ZMQError as e:
                self._logger.error(f"{self.name} send error: {e}")
    
    def send_json(self, msg_type: str, data: dict):
        """Send a JSON message."""
        message = ZMQMessage(type=msg_type, data=data, timestamp=datetime.now())
        self.send(message.to_json())
    
    def send_tick(self, tick: TickData):
        """Send tick data."""
        self.send_json(MessageType.TICK.value, {
            'symbol': tick.symbol,
            'bid': tick.bid,
            'ask': tick.ask,
            'last': tick.last,
            'volume': tick.volume,
            'spread': tick.spread,
            'spread_points': tick.spread_points,
            'time': tick.time.isoformat(),
            'server_time': tick.server_time.isoformat(),
        })
    
    def send_response(self, response: OrderResponse):
        """Send order response."""
        self.send(response.to_json())
    
    @property
    def is_running(self) -> bool:
        """Check if publisher is running."""
        return self._running


class ZMQCommandReceiver:
    """ZeroMQ command receiver using ROUTER socket for bidirectional communication."""
    
    def __init__(self, address: str, port: int, name: str = "CommandReceiver"):
        self.address = f"{address}:{port}"
        self.port = port
        self.name = name
        self.context = None
        self.socket = None
        self._running = False
        self._thread: Optional[threading.Thread] = None
        self._message_callback: Optional[Callable] = None
        self._pending_responses: Dict[bytes, str] = {}  # identity -> response JSON
        self._logger = get_system_logger()
        self._order_logger = get_order_logger()
    
    def set_message_callback(self, callback: Callable):
        """Set callback for received messages."""
        self._message_callback = callback
    
    def start(self):
        """Start the command receiver (bind)."""
        if self._running:
            return
        
        self._logger.info(f"Starting {self.name} on {self.address}...")
        
        self.context = zmq.Context(io_threads=2)
        self.socket = self.context.socket(zmq.ROUTER)
        self.socket.setsockopt(zmq.LINGER, 1000)
        self.socket.setsockopt(zmq.RCVBUF, 1048576)
        self.socket.setsockopt(zmq.RCVHWM, 1000)
        self.socket.setsockopt(zmq.RCVTIMEO, 100)  # 100ms timeout for recv
        
        try:
            self.socket.bind(self.address)
            self._running = True
            
            # Start receiver thread
            self._thread = threading.Thread(target=self._receive_loop, daemon=True)
            self._thread.start()
            
            self._logger.info(f"{self.name} started on {self.address}")
        except zmq.ZMQError as e:
            self._logger.error(f"Failed to bind {self.name}: {e}")
            self.cleanup()
            raise
    
    def send_response(self, response_json: str):
        """Send response to client. Must be called right after receiving."""
        # Get the last client identity from the queue
        try:
            # For ROUTER, we need to keep track of identity
            # We'll use a simpler approach: client sends, we reply immediately
            pass
        except Exception as e:
            self._logger.error(f"{self.name} send response error: {e}")
    
    def _receive_loop(self):
        """Main receive loop running in thread."""
        self._logger.info(f"{self.name} receiver thread started")
        
        while self._running:
            try:
                # Receive message with identity (ROUTER adds identity frame as first part)
                msg_parts = self.socket.recv_multipart()
                
                if len(msg_parts) >= 2:
                    identity = msg_parts[0]
                    msg_content = msg_parts[1].decode()
                    
                    # Process message and send response
                    self._process_message(msg_content, identity)
                
            except zmq.Again:
                # No message available, continue
                pass
            except zmq.ZMQError as e:
                if self._running:
                    self._logger.error(f"{self.name} receive error: {e}")
                break
    
    def _process_message(self, message: str, identity: bytes):
        """Process received message and send response."""
        try:
            data = json.loads(message)
            msg_type = data.get('type', 'UNKNOWN')
            
            # Log incoming order commands
            if msg_type in [MessageType.ORDER_SEND.value, 
                           MessageType.POSITION_CLOSE.value,
                           MessageType.POSITION_MODIFY.value]:
                self._order_logger.info(f"RECV: {msg_type} - {data.get('data', {})}")
            
            # Call callback - OrderWorker will process and return response via send_to_client
            if self._message_callback:
                result = self._message_callback(msg_type, data.get('data', {}))
                
                # If callback returns a response, send it back
                if result is not None:
                    self._send_to_client(identity, result)
                
        except json.JSONDecodeError as e:
            self._logger.warning(f"{self.name}: Invalid JSON received: {e}")
            # Send error response
            self._send_to_client(identity, json.dumps({
                "success": False,
                "error": f"Invalid JSON: {e}"
            }))
        except Exception as e:
            self._logger.error(f"{self.name}: Error processing message: {e}")
            self._send_to_client(identity, json.dumps({
                "success": False,
                "error": str(e)
            }))
    
    def _send_to_client(self, identity: bytes, response: str):
        """Send response back to client."""
        try:
            self.socket.send_multipart([identity, response.encode()])
        except zmq.ZMQError as e:
            self._logger.error(f"{self.name} send to client error: {e}")
    
    def stop(self):
        """Stop the command receiver."""
        if not self._running:
            return
        
        self._logger.info(f"Stopping {self.name}...")
        self._running = False
        
        # Wait for thread to finish
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2.0)
        
        self.cleanup()
        self._logger.info(f"{self.name} stopped")
    
    def cleanup(self):
        """Clean up ZMQ resources."""
        if self.socket:
            try:
                self.socket.close(linger=1000)
            except:
                pass
            self.socket = None
        
        if self.context:
            try:
                self.context.term()
            except:
                pass
            self.context = None
    
    @property
    def is_running(self) -> bool:
        """Check if command receiver is running."""
        return self._running


class ZMQManager:
    """Manager for all ZeroMQ connections."""
    
    def __init__(self, config):
        self.config = config
        self.market_publisher: Optional[ZMQPublisher] = None
        self.order_receiver: Optional[ZMQCommandReceiver] = None
        self._logger = get_system_logger()
    
    def start(self):
        """Start all ZeroMQ connections."""
        self._logger.info("=" * 50)
        self._logger.info("Starting ZeroMQ connections...")
        self._logger.info("=" * 50)
        
        # Market data publisher on port 5555
        self.market_publisher = ZMQPublisher(
            address=self.config.zmq.bind_address,
            port=self.config.zmq.market_data_port,
            name="MarketPublisher"
        )
        self.market_publisher.start()
        
        # Order command receiver on port 5556 (ROUTER - bidirectional)
        # This handles both receiving commands and sending responses
        self.order_receiver = ZMQCommandReceiver(
            address=self.config.zmq.bind_address,
            port=self.config.zmq.order_port,
            name="OrderReceiver"
        )
        self.order_receiver.start()
        
        self._logger.info("ZeroMQ connections started successfully")
    
    def stop(self):
        """Stop all ZeroMQ connections."""
        self._logger.info("Stopping ZeroMQ connections...")
        
        if self.order_receiver:
            self.order_receiver.stop()
            self.order_receiver = None
        
        if self.market_publisher:
            self.market_publisher.stop()
            self.market_publisher = None
        
        self._logger.info("ZeroMQ connections stopped")
    
    def set_order_callback(self, callback: Callable):
        """Set callback for order commands."""
        if self.order_receiver:
            self.order_receiver.set_message_callback(callback)