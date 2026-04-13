"""Main application entry point for Python Bridge."""

import signal
import sys
import time
import threading
from typing import Optional

# Support both relative and absolute imports
if __package__:
    from .config import config, AppConfig
    from .logger import setup_loggers, get_system_logger
    from .mt5_connector import MT5Connector
    from .zmq_publisher import ZMQManager
    from .worker import MarketDataWorker, OrderWorker, HeartbeatWorker
else:
    from config import config, AppConfig
    from logger import setup_loggers, get_system_logger
    from mt5_connector import MT5Connector
    from zmq_publisher import ZMQManager
    from worker import MarketDataWorker, OrderWorker, HeartbeatWorker


class BridgeApplication:
    """Main application class managing all components."""
    
    def __init__(self, app_config: Optional[AppConfig] = None):
        self.config = app_config or config
        self._running = False
        self._shutdown_event = threading.Event()
        
        # Components
        self._mt5: Optional[MT5Connector] = None
        self._zmq: Optional[ZMQManager] = None
        self._market_worker: Optional[MarketDataWorker] = None
        self._order_worker: Optional[OrderWorker] = None
        self._heartbeat_worker: Optional[HeartbeatWorker] = None
        
        # Logger
        self._logger = None
    
    def _setup_signal_handlers(self):
        """Set up signal handlers for graceful shutdown."""
        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)
        
        # Set signal handler for the main thread
        if sys.platform != 'win32':
            signal.signal(signal.SIGHUP, self._signal_handler)
    
    def _signal_handler(self, signum, frame):
        """Handle shutdown signals."""
        signal_name = signal.Signals(signum).name
        self._logger.info(f"Received {signal_name}, initiating shutdown...")
        self._shutdown_event.set()
        self.shutdown()
    
    def start(self) -> bool:
        """Start the application."""
        print("=" * 60)
        print("  PYTHON BRIDGE - MT5 to ZeroMQ Bridge")
        print("=" * 60)
        print()
        
        # Setup logging
        setup_loggers(self.config)
        self._logger = get_system_logger()
        
        self._logger.info("=" * 60)
        self._logger.info("Starting Python Bridge Application")
        self._logger.info("=" * 60)
        
        # Setup signal handlers
        self._setup_signal_handlers()
        
        try:
            # Initialize MT5 connection
            self._logger.info("Step 1/4: Initializing MT5 connection...")
            self._mt5 = MT5Connector(symbols=self.config.mt5.symbols)
            
            if not self._mt5.connect():
                self._logger.critical("Failed to connect to MT5. Exiting.")
                return False
            
            # Initialize ZeroMQ
            self._logger.info("Step 2/4: Initializing ZeroMQ...")
            self._zmq = ZMQManager(self.config)
            self._zmq.start()
            
            # Initialize workers
            self._logger.info("Step 3/4: Starting workers...")
            
            # Market data worker
            self._market_worker = MarketDataWorker(
                mt5=self._mt5,
                zmq=self._zmq,
                symbols=self.config.mt5.symbols,
                interval=0.5  # 500ms update interval
            )
            self._market_worker.start()
            
            # Order worker
            self._order_worker = OrderWorker(
                mt5=self._mt5,
                zmq=self._zmq
            )
            self._order_worker.start()
            
            # Heartbeat worker
            self._heartbeat_worker = HeartbeatWorker(
                zmq=self._zmq,
                interval=30  # 30 second heartbeat
            )
            self._heartbeat_worker.start()
            
            # Application running
            self._logger.info("Step 4/4: Application ready!")
            self._logger.info("=" * 60)
            self._logger.info("APPLICATION STARTED SUCCESSFULLY")
            self._logger.info(f"  - Market Data Port: {self.config.zmq.market_data_port}")
            self._logger.info(f"  - Order Command Port: {self.config.zmq.order_port}")
            self._logger.info(f"  - Symbols: {', '.join(self.config.mt5.symbols)}")
            self._logger.info("=" * 60)
            self._logger.info("Press Ctrl+C to stop...")
            
            self._running = True
            
            # Keep main thread alive
            while self._running and not self._shutdown_event.is_set():
                self._shutdown_event.wait(timeout=1.0)
            
            if self._running:
                self.shutdown()
            
            return True
            
        except Exception as e:
            self._logger.critical(f"Application error: {e}")
            self._logger.exception("Full traceback:")
            self.shutdown()
            return False
    
    def shutdown(self):
        """Shutdown the application gracefully."""
        if not self._running:
            return
        
        self._logger.info("=" * 60)
        self._logger.info("SHUTTING DOWN APPLICATION")
        self._logger.info("=" * 60)
        
        self._running = False
        
        # Stop workers first
        self._logger.info("Stopping workers...")
        
        if self._heartbeat_worker:
            self._heartbeat_worker.stop()
            self._heartbeat_worker = None
        
        if self._market_worker:
            self._market_worker.stop()
            self._market_worker = None
        
        if self._order_worker:
            self._order_worker.stop()
            self._order_worker = None
        
        # Stop ZeroMQ
        self._logger.info("Stopping ZeroMQ...")
        if self._zmq:
            self._zmq.stop()
            self._zmq = None
        
        # Disconnect MT5
        self._logger.info("Disconnecting MT5...")
        if self._mt5:
            self._mt5.disconnect()
            self._mt5 = None
        
        # Small delay to ensure cleanup
        time.sleep(0.5)
        
        self._logger.info("=" * 60)
        self._logger.info("APPLICATION STOPPED")
        self._logger.info("=" * 60)
        
        # Clear logging handlers
        import logging
        logging.shutdown()


def run():
    """Run the application."""
    app = BridgeApplication()
    app.start()


if __name__ == "__main__":
    run()