"""MetaTrader 5 connector for Python Bridge."""

import MetaTrader5 as mt5
from datetime import datetime
from typing import Optional, List, Dict, Any
from concurrent.futures import ThreadPoolExecutor, as_completed
import threading
import time

# Support both relative and absolute imports
if __package__:
    from .models import TickData, OrderRequest, OrderResponse, MessageType
    from .logger import get_system_logger, get_market_logger
else:
    from models import TickData, OrderRequest, OrderResponse, MessageType
    from logger import get_system_logger, get_market_logger


class MT5Connector:
    """MetaTrader 5 connection manager."""
    
    def __init__(self, symbols: List[str] = None):
        self.symbols = symbols or ["GOLD", "XAUUSD"]
        self.connected = False
        self._system_logger = get_system_logger()
        self._market_logger = get_market_logger()
    
    def connect(self) -> bool:
        """Initialize MT5 connection."""
        self._system_logger.info("Initializing MT5 connection...")
        
        if not mt5.initialize():
            error_code, error_text = mt5.last_error()
            self._system_logger.error(f"MT5 initialization failed: [{error_code}] {error_text}")
            return False
        
        # Get terminal info
        terminal_info = mt5.terminal_info()
        if terminal_info is None:
            self._system_logger.error("Failed to get terminal info")
            mt5.shutdown()
            return False
        
        account_info = mt5.account_info()
        if account_info is None:
            self._system_logger.error("Failed to get account info")
            mt5.shutdown()
            return False
        
        self.connected = True
        self._system_logger.info(f"MT5 connected successfully")
        self._system_logger.info(f"  Terminal: {terminal_info.name}")
        self._system_logger.info(f"  Account: {account_info.login}")
        self._system_logger.info(f"  Server: {account_info.server}")
        self._system_logger.info(f"  Company: {account_info.company}")
        
        # Initialize symbols
        self._initialize_symbols()
        
        return True
    
    def _initialize_symbols(self):
        """Initialize and configure symbols."""
        for symbol in self.symbols:
            if mt5.symbol_select(symbol, True):
                self._system_logger.info(f"Symbol selected: {symbol}")
            else:
                self._system_logger.warning(f"Failed to select symbol: {symbol}")
    
    def disconnect(self):
        """Shutdown MT5 connection."""
        if self.connected:
            self._system_logger.info("Shutting down MT5 connection...")
            mt5.shutdown()
            self.connected = False
            self._system_logger.info("MT5 disconnected")
    
    def get_tick(self, symbol: str) -> Optional[TickData]:
        """Get current tick for a symbol."""
        tick = mt5.symbol_info_tick(symbol)
        
        if tick is None:
            return None
        
        return TickData(
            symbol=symbol,
            bid=tick.bid,
            ask=tick.ask,
            last=tick.last,
            volume=tick.volume,
            time=datetime.fromtimestamp(tick.time),
            server_time=datetime.now()
        )
    
    def get_ticks(self, symbols: List[str] = None) -> Dict[str, TickData]:
        """Get current ticks for multiple symbols."""
        symbols = symbols or self.symbols
        ticks = {}
        
        for symbol in symbols:
            tick = self.get_tick(symbol)
            if tick:
                ticks[symbol] = tick
        
        return ticks
    
    def get_symbol_info(self, symbol: str) -> Optional[Dict[str, Any]]:
        """Get symbol information."""
        info = mt5.symbol_info(symbol)
        
        if info is None:
            return None
        
        return {
            'symbol': info.name,
            'bid': info.bid,
            'ask': info.ask,
            'last': info.last,
            'volume': info.volume,
            'high': info.high,
            'low': info.low,
            'spread': info.spread,
            'digits': info.digits,
            'point': info.point,
            'tick_value': info.trade_tick_value,
            'tick_size': info.trade_tick_size,
            'contract_size': info.trade_contract_size,
            'volume_min': info.volume_min,
            'volume_max': info.volume_max,
            'volume_step': info.volume_step,
        }
    
    def send_order(self, request: OrderRequest) -> OrderResponse:
        """Send trading order to MT5."""
        # Map order type string to MT5 constant
        order_type_map = {
            'BUY': mt5.ORDER_TYPE_BUY,
            'SELL': mt5.ORDER_TYPE_SELL,
            'BUYLIMIT': mt5.ORDER_TYPE_BUY_LIMIT,
            'SELLLIMIT': mt5.ORDER_TYPE_SELL_LIMIT,
            'BUYSTOP': mt5.ORDER_TYPE_BUY_STOP,
            'SELLSTOP': mt5.ORDER_TYPE_SELL_STOP,
        }
        
        order_type = order_type_map.get(request.order_type.upper())
        
        if order_type is None:
            return OrderResponse(
                success=False,
                message_type=MessageType.ORDER_SEND,
                error_message=f"Invalid order type: {request.order_type}",
                request_id=request.request_id
            )
        
        # Get symbol info for price validation
        symbol_info = mt5.symbol_info(request.symbol)
        if symbol_info is None:
            return OrderResponse(
                success=False,
                message_type=MessageType.ORDER_SEND,
                error_message=f"Symbol not found: {request.symbol}",
                request_id=request.request_id
            )
        
        # Get current price
        tick = mt5.symbol_info_tick(request.symbol)
        if tick is None:
            return OrderResponse(
                success=False,
                message_type=MessageType.ORDER_SEND,
                error_message="Failed to get tick data",
                request_id=request.request_id
            )
        
        # Determine price based on order type
        if 'BUY' in request.order_type.upper():
            price = tick.ask if request.price == 0 else request.price
        else:
            price = tick.bid if request.price == 0 else request.price
        
        # Create trade request as dictionary (MT5 Python API uses dict)
        deviation = 10  # Slippage in points
        
        trade_request = {
            "action": mt5.TRADE_ACTION_DEAL,
            "symbol": request.symbol,
            "volume": request.volume,
            "type": order_type,
            "price": price,
            "deviation": deviation,
            "magic": request.magic or 0,
            "comment": request.comment or "",
            "type_filling": mt5.ORDER_FILLING_IOC,
            "type_time": mt5.ORDER_TIME_GTC,
            "expiration": 0
        }
        
        # Chỉ thêm SL/TP nếu có giá trị hợp lệ
        if request.stop_loss and request.stop_loss > 0:
            trade_request["sl"] = request.stop_loss
        if request.take_profit and request.take_profit > 0:
            trade_request["tp"] = request.take_profit
        
        # Send order
        result = mt5.order_send(trade_request)
        
        if result is None:
            error_code, error_text = mt5.last_error()
            self._system_logger.error(f"Order send failed: [{error_code}] {error_text}")
            return OrderResponse(
                success=False,
                message_type=MessageType.ORDER_SEND,
                error_code=error_code,
                error_message=error_text,
                request_id=request.request_id
            )
        
        if result.retcode == mt5.TRADE_RETCODE_DONE:
            self._system_logger.info(
                f"ORDER SENT: #{result.order} {request.order_type} {request.volume} lots "
                f"{request.symbol} @ {price}"
            )
            return OrderResponse(
                success=True,
                message_type=MessageType.ORDER_SEND,
                ticket=result.order,
                volume=request.volume,
                price=price,
                comment=result.comment,
                request_id=request.request_id
            )
        else:
            self._system_logger.error(
                f"Order failed: [{result.retcode}] {result.comment}"
            )
            return OrderResponse(
                success=False,
                message_type=MessageType.ORDER_SEND,
                error_code=result.retcode,
                error_message=result.comment,
                request_id=request.request_id
            )
    
    def close_position(self, ticket: int, volume: float = 0.0, 
                       order_type: str = None) -> OrderResponse:
        """Close a position by ticket."""
        # Get position info
        positions = mt5.positions_get(ticket=ticket)
        if not positions:
            return OrderResponse(
                success=False,
                message_type=MessageType.POSITION_CLOSE,
                error_message=f"Position not found: #{ticket}",
                request_id=str(ticket)
            )
        
        position = positions[0]
        symbol = position.symbol
        
        # Determine opposite order type
        if position.type == mt5.POSITION_TYPE_BUY:
            close_type = mt5.ORDER_TYPE_SELL
        else:
            close_type = mt5.ORDER_TYPE_BUY
        
        # Get current price
        tick = mt5.symbol_info_tick(symbol)
        if tick is None:
            return OrderResponse(
                success=False,
                message_type=MessageType.POSITION_CLOSE,
                error_message="Failed to get tick data",
                request_id=str(ticket)
            )
        
        price = tick.bid if close_type == mt5.ORDER_TYPE_SELL else tick.ask
        
        # Close with full volume if not specified
        close_volume = volume if volume > 0 else position.volume
        
        # Get profit before closing
        profit = position.profit
        
        # Create trade request as dictionary
        trade_request = {
            "action": mt5.TRADE_ACTION_DEAL,
            "symbol": symbol,
            "volume": close_volume,
            "type": close_type,
            "price": price,
            "deviation": 10,
            "magic": position.magic,
            "comment": f"Close #{ticket}",
            "position": ticket,
            "type_filling": mt5.ORDER_FILLING_IOC
        }
        
        result = mt5.order_send(trade_request)
        
        if result and result.retcode == mt5.TRADE_RETCODE_DONE:
            self._system_logger.info(f"POSITION CLOSED: #{ticket} | P&L: ${profit:.2f}")
            return OrderResponse(
                success=True,
                message_type=MessageType.POSITION_CLOSE,
                ticket=ticket,
                volume=close_volume,
                price=price,
                comment=f"P&L: ${profit:.2f}",
                request_id=str(ticket)
            )
        else:
            error_msg = result.comment if result else "Unknown error"
            self._system_logger.error(f"Close position failed: [{ticket}] {error_msg}")
            return OrderResponse(
                success=False,
                message_type=MessageType.POSITION_CLOSE,
                ticket=ticket,
                error_message=error_msg,
                request_id=str(ticket)
            )
    
    def close_positions_parallel(self, tickets: List[int], max_workers: int = 10) -> Dict[str, Any]:
        """Close multiple positions in parallel using thread pool.
        
        Args:
            tickets: List of position tickets to close
            max_workers: Maximum concurrent close operations (default: 10)
            
        Returns:
            Summary dict with closed/failed counts and detailed results
        """
        if not tickets:
            return {
                'success': True, 
                'closed': 0, 
                'failed': 0, 
                'results': [],
                'errors': []
            }
        
        self._system_logger.info(f"PARALLEL CLOSE: Starting {len(tickets)} positions with {max_workers} workers")
        
        # Thread-local storage for results
        results = []
        results_lock = threading.Lock()
        
        def close_single(ticket: int) -> dict:
            """Close a single position (runs in thread)."""
            try:
                response = self.close_position(ticket, volume=0)
                return {
                    'ticket': ticket,
                    'success': response.success,
                    'price': response.price if response.success else None,
                    'error': response.error_message if not response.success else None
                }
            except Exception as e:
                return {
                    'ticket': ticket,
                    'success': False,
                    'error': f"Thread exception: {str(e)}"
                }
        
        # Use thread pool for parallel execution
        with ThreadPoolExecutor(max_workers=min(max_workers, len(tickets))) as executor:
            # Submit all tasks
            futures = {executor.submit(close_single, ticket): ticket for ticket in tickets}
            
            # Collect results as they complete
            for future in as_completed(futures):
                ticket = futures[future]
                try:
                    result = future.result()
                    with results_lock:
                        results.append(result)
                    
                    if result['success']:
                        self._system_logger.info(f"  PARALLEL CLOSE OK: #{ticket} @ {result.get('price')}")
                    else:
                        self._system_logger.error(f"  PARALLEL CLOSE FAIL: #{ticket} - {result.get('error')}")
                        
                except Exception as e:
                    with results_lock:
                        results.append({
                            'ticket': ticket,
                            'success': False,
                            'error': f"Future exception: {str(e)}"
                        })
        
        # Summarize
        closed = sum(1 for r in results if r.get('success'))
        failed = len(results) - closed
        errors = [r for r in results if not r.get('success')]
        
        self._system_logger.info(
            f"PARALLEL CLOSE COMPLETE: {closed}/{len(tickets)} OK, {failed} failed"
        )
        
        return {
            'success': failed == 0,
            'closed': closed,
            'failed': failed,
            'results': results,
            'errors': errors,
            'timestamp': datetime.now().isoformat()
        }
    
    def modify_position(self, ticket: int, stop_loss: float = None,
                        take_profit: float = None) -> OrderResponse:
        """Modify a position's SL/TP."""
        positions = mt5.positions_get(ticket=ticket)
        if not positions:
            return OrderResponse(
                success=False,
                message_type=MessageType.POSITION_MODIFY,
                error_message=f"Position not found: #{ticket}",
                request_id=str(ticket)
            )
        
        position = positions[0]
        
        # Use existing values if not specified
        new_sl = stop_loss if stop_loss is not None else position.sl
        new_tp = take_profit if take_profit is not None else position.tp
        
        # Create trade request as dictionary
        trade_request = {
            "action": mt5.TRADE_ACTION_SLTP,
            "position": ticket,
            "sl": new_sl if new_sl else 0,
            "tp": new_tp if new_tp else 0,
            "deviation": 0,
            "type_filling": mt5.ORDER_FILLING_IOC
        }
        
        result = mt5.order_send(trade_request)
        
        if result and result.retcode == mt5.TRADE_RETCODE_DONE:
            self._system_logger.info(
                f"POSITION MODIFIED: #{ticket} SL={new_sl} TP={new_tp}"
            )
            return OrderResponse(
                success=True,
                message_type=MessageType.POSITION_MODIFY,
                ticket=ticket,
                price=new_sl,
                comment=f"SL={new_sl}, TP={new_tp}",
                request_id=str(ticket)
            )
        else:
            error_msg = result.comment if result else "Unknown error"
            return OrderResponse(
                success=False,
                message_type=MessageType.POSITION_MODIFY,
                ticket=ticket,
                error_message=error_msg,
                request_id=str(ticket)
            )
    
    def get_positions(self, symbol: str = None) -> List[Dict[str, Any]]:
        """Get open positions."""
        if symbol:
            positions = mt5.positions_get(symbol=symbol)
        else:
            positions = mt5.positions_get()
        
        result = []
        for pos in positions:
            result.append({
                'ticket': pos.ticket,
                'symbol': pos.symbol,
                'type': 'BUY' if pos.type == mt5.POSITION_TYPE_BUY else 'SELL',
                'volume': pos.volume,
                'price_open': pos.price_open,
                'price_current': pos.price_current,
                'stop_loss': pos.sl,
                'take_profit': pos.tp,
                'profit': pos.profit,
                'magic': pos.magic,
                'comment': pos.comment,
                'time': datetime.fromtimestamp(pos.time),
            })
        
        return result
    
    def get_orders(self, symbol: str = None) -> List[Dict[str, Any]]:
        """Get pending orders."""
        if symbol:
            orders = mt5.orders_get(symbol=symbol)
        else:
            orders = mt5.orders_get()
        
        result = []
        type_map = {
            mt5.ORDER_TYPE_BUY_LIMIT: 'BUYLIMIT',
            mt5.ORDER_TYPE_SELL_LIMIT: 'SELLLIMIT',
            mt5.ORDER_TYPE_BUY_STOP: 'BUYSTOP',
            mt5.ORDER_TYPE_SELL_STOP: 'SELLSTOP',
        }
        
        for order in orders:
            result.append({
                'ticket': order.ticket,
                'symbol': order.symbol,
                'type': type_map.get(order.type, 'UNKNOWN'),
                'volume': order.volume_current,
                'price': order.price_open,
                'stop_loss': order.sl,
                'take_profit': order.tp,
                'magic': order.magic,
                'comment': order.comment,
                'time': datetime.fromtimestamp(order.time_setup),
            })
        
        return result