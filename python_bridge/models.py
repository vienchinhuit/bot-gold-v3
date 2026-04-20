"""Data models for Python Bridge application."""

import json
from dataclasses import dataclass, asdict
from datetime import datetime
from enum import Enum
from typing import Optional


class MessageType(Enum):
    """Message types for ZeroMQ communication."""
    # Market Data
    TICK = "TICK"
    OHLC = "OHLC"
    SUBSCRIBE = "SUBSCRIBE"
    UNSUBSCRIBE = "UNSUBSCRIBE"
    
    # Order Commands
    ORDER_SEND = "ORDER_SEND"
    ORDER_CLOSE = "ORDER_CLOSE"
    ORDER_MODIFY = "ORDER_MODIFY"
    ORDER_INFO = "ORDER_INFO"
    POSITION_CLOSE = "POSITION_CLOSE"
    POSITION_CLOSE_BATCH = "POSITION_CLOSE_BATCH"
    POSITION_MODIFY = "POSITION_MODIFY"
    
    # Responses
    RESPONSE_SUCCESS = "RESPONSE_SUCCESS"
    RESPONSE_ERROR = "RESPONSE_ERROR"
    
    # Heartbeat
    HEARTBEAT = "HEARTBEAT"


@dataclass
class TickData:
    """Tick data model for market prices."""
    symbol: str
    bid: float
    ask: float
    last: float
    volume: int
    time: datetime
    server_time: datetime
    
    def to_json(self) -> str:
        """Convert to JSON string."""
        data = asdict(self)
        data['time'] = self.time.isoformat()
        data['server_time'] = self.server_time.isoformat()
        return json.dumps(data, ensure_ascii=False)
    
    @classmethod
    def from_json(cls, json_str: str) -> 'TickData':
        """Create from JSON string."""
        data = json.loads(json_str)
        data['time'] = datetime.fromisoformat(data['time'])
        data['server_time'] = datetime.fromisoformat(data['server_time'])
        return cls(**data)
    
    @property
    def spread(self) -> float:
        """Calculate spread."""
        return self.ask - self.bid
    
    @property
    def spread_points(self) -> float:
        """Calculate spread in points (for GOLD: 1 point = 0.01)."""
        return self.spread / 0.01


@dataclass
class OrderRequest:
    """Order request model."""
    ticket: Optional[int]
    symbol: str
    volume: float
    order_type: str  # BUY, SELL, BUYLIMIT, SELLLIMIT, etc.
    price: float
    stop_loss: Optional[float] = None
    take_profit: Optional[float] = None
    comment: Optional[str] = None
    magic: Optional[int] = None
    request_id: str = ""
    
    def to_json(self) -> str:
        """Convert to JSON string."""
        return json.dumps(asdict(self), ensure_ascii=False)
    
    @classmethod
    def from_json(cls, json_str: str) -> 'OrderRequest':
        """Create from JSON string."""
        return cls(**json.loads(json_str))


@dataclass
class OrderResponse:
    """Order response model."""
    success: bool
    message_type: MessageType
    ticket: Optional[int] = None
    order: Optional[int] = None
    volume: Optional[float] = None
    price: Optional[float] = None
    comment: Optional[str] = None
    error_code: Optional[int] = None
    error_message: Optional[str] = None
    request_id: str = ""
    timestamp: datetime = None
    
    def __post_init__(self):
        if self.timestamp is None:
            self.timestamp = datetime.now()
        if isinstance(self.message_type, str):
            self.message_type = MessageType(self.message_type)
    
    def to_json(self) -> str:
        """Convert to JSON string."""
        data = asdict(self)
        data['message_type'] = self.message_type.value
        data['timestamp'] = self.timestamp.isoformat()
        return json.dumps(data, ensure_ascii=False)
    
    @classmethod
    def from_json(cls, json_str: str) -> 'OrderResponse':
        """Create from JSON string."""
        return cls(**json.loads(json_str))


@dataclass
class ZMQMessage:
    """Generic ZeroMQ message wrapper."""
    type: str
    data: dict
    timestamp: datetime = None
    
    def __post_init__(self):
        if self.timestamp is None:
            self.timestamp = datetime.now()
    
    def to_json(self) -> str:
        """Convert to JSON string."""
        data = {
            'type': self.type,
            'data': self.data,
            'timestamp': self.timestamp.isoformat()
        }
        return json.dumps(data, ensure_ascii=False)
    
    @classmethod
    def from_json(cls, json_str: str) -> 'ZMQMessage':
        """Create from JSON string."""
        data = json.loads(json_str)
        data['timestamp'] = datetime.fromisoformat(data['timestamp'])
        return cls(**data)