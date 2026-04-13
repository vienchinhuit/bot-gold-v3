"""Logging setup for Python Bridge application."""

import logging
import os
import sys
from logging.handlers import RotatingFileHandler
from pathlib import Path
from typing import Optional

# Create logs directory at module import
_logs_dir = Path("logs")
_logs_dir.mkdir(exist_ok=True)


class BridgeLogger:
    """Custom logger for the bridge application."""
    
    def __init__(self, name: str, log_file: str, console_level: str = "INFO", 
                 file_level: str = "DEBUG", max_bytes: int = 10*1024*1024,
                 backup_count: int = 5):
        self.logger = logging.getLogger(name)
        self.logger.setLevel(logging.DEBUG)  # Capture all levels
        
        # Avoid duplicate handlers
        if self.logger.handlers:
            self.logger.handlers.clear()
        
        # Create log directory if not exists
        log_path = Path(log_file)
        log_path.parent.mkdir(parents=True, exist_ok=True)
        
        # Console handler
        console_handler = self._create_console_handler(console_level)
        self.logger.addHandler(console_handler)
        
        # File handler with rotation
        file_handler = self._create_file_handler(
            log_file, file_level, max_bytes, backup_count
        )
        self.logger.addHandler(file_handler)
    
    def _create_console_handler(self, level: str) -> logging.StreamHandler:
        """Create console handler with specified level."""
        handler = logging.StreamHandler(sys.stdout)
        handler.setLevel(getattr(logging, level.upper()))
        
        # Format for console
        formatter = logging.Formatter(
            fmt='%(asctime)s | %(levelname)-8s | %(message)s',
            datefmt='%H:%M:%S'
        )
        handler.setFormatter(formatter)
        return handler
    
    def _create_file_handler(self, log_file: str, level: str, 
                            max_bytes: int, backup_count: int) -> RotatingFileHandler:
        """Create rotating file handler."""
        handler = RotatingFileHandler(
            log_file,
            maxBytes=max_bytes,
            backupCount=backup_count,
            encoding='utf-8'
        )
        handler.setLevel(getattr(logging, level.upper()))
        
        # Format for file
        formatter = logging.Formatter(
            fmt='%(asctime)s | %(levelname)-8s | %(name)s | %(funcName)s:%(lineno)d | %(message)s',
            datefmt='%Y-%m-%d %H:%M:%S'
        )
        handler.setFormatter(formatter)
        return handler
    
    def debug(self, msg: str):
        self.logger.debug(msg)
    
    def info(self, msg: str):
        self.logger.info(msg)
    
    def warning(self, msg: str):
        self.logger.warning(msg)
    
    def error(self, msg: str):
        self.logger.error(msg)
    
    def critical(self, msg: str):
        self.logger.critical(msg)
    
    def exception(self, msg: str):
        self.logger.exception(msg)


# Global logger instances
_market_logger: Optional[BridgeLogger] = None
_order_logger: Optional[BridgeLogger] = None
_system_logger: Optional[BridgeLogger] = None


def setup_loggers(config) -> tuple:
    """Setup all loggers for the application."""
    global _market_logger, _order_logger, _system_logger
    
    # Market data logger
    _market_logger = BridgeLogger(
        name="market",
        log_file=config.logging.log_file.replace("bridge.log", "market.log"),
        console_level=config.logging.console_level,
        file_level=config.logging.file_level,
        max_bytes=config.logging.max_bytes,
        backup_count=config.logging.backup_count
    )
    
    # Order logger
    _order_logger = BridgeLogger(
        name="order",
        log_file=config.logging.log_file.replace("bridge.log", "order.log"),
        console_level=config.logging.console_level,
        file_level=config.logging.file_level,
        max_bytes=config.logging.max_bytes,
        backup_count=config.logging.backup_count
    )
    
    # System logger
    _system_logger = BridgeLogger(
        name="system",
        log_file=config.logging.log_file,
        console_level=config.logging.console_level,
        file_level=config.logging.file_level,
        max_bytes=config.logging.max_bytes,
        backup_count=config.logging.backup_count
    )
    
    return _market_logger, _order_logger, _system_logger


def get_market_logger() -> BridgeLogger:
    """Get market logger instance."""
    return _market_logger


def get_order_logger() -> BridgeLogger:
    """Get order logger instance."""
    return _order_logger


def get_system_logger() -> BridgeLogger:
    """Get system logger instance."""
    return _system_logger