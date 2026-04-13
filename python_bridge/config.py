"""Configuration settings for Python Bridge application."""

from dataclasses import dataclass
from typing import List


@dataclass
class ZMQConfig:
    """ZeroMQ configuration."""
    market_data_port: int = 5555
    order_port: int = 5556
    bind_address: str = "tcp://*"


@dataclass
class MT5Config:
    """MetaTrader 5 configuration."""
    symbols: List[str] = None  # Default: ["GOLD", "XAUUSD"]
    
    def __post_init__(self):
        if self.symbols is None:
            self.symbols = ["GOLD", "XAUUSD"]


@dataclass
class LoggingConfig:
    """Logging configuration."""
    log_file: str = "logs/bridge.log"
    console_level: str = "INFO"  # DEBUG, INFO, WARNING, ERROR
    file_level: str = "DEBUG"
    max_bytes: int = 10 * 1024 * 1024  # 10MB
    backup_count: int = 5


@dataclass
class AppConfig:
    """Main application configuration."""
    zmq: ZMQConfig = None
    mt5: MT5Config = None
    logging: LoggingConfig = None
    
    def __post_init__(self):
        if self.zmq is None:
            self.zmq = ZMQConfig()
        if self.mt5 is None:
            self.mt5 = MT5Config()
        if self.logging is None:
            self.logging = LoggingConfig()


# Global configuration instance
config = AppConfig()