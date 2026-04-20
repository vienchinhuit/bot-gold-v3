"""
History Service: Lưu và quản lý lịch sử các lệnh đã đóng
"""

import json
import os
from datetime import datetime, date
from typing import List, Dict, Any, Optional
from collections import defaultdict


SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
HISTORY_FILE = os.path.join(SCRIPT_DIR, "order_history.json")


class HistoryService:
    """Service để lưu và truy vấn lịch sử orders"""
    
    def __init__(self, history_file: str = HISTORY_FILE):
        self.history_file = history_file
        self._ensure_file_exists()
    
    def _ensure_file_exists(self):
        """Tạo file history nếu chưa tồn tại"""
        if not os.path.exists(self.history_file):
            try:
                os.makedirs(os.path.dirname(self.history_file), exist_ok=True)
            except Exception:
                pass
            self._save([])
    
    def _load(self) -> List[Dict]:
        """Đọc lịch sử từ file"""
        try:
            with open(self.history_file, 'r', encoding='utf-8') as f:
                data = json.load(f)
                return data if isinstance(data, list) else []
        except (json.JSONDecodeError, FileNotFoundError):
            return []
    
    def _save(self, history: List[Dict]):
        """Lưu lịch sử vào file"""
        try:
            os.makedirs(os.path.dirname(self.history_file), exist_ok=True)
            with open(self.history_file, 'w', encoding='utf-8') as f:
                json.dump(history, f, indent=2, ensure_ascii=False)
        except Exception as e:
            print(f"Lỗi lưu history: {e}")
    
    def add_record(self, ticket: int, symbol: str, type_: str, volume: float,
                  price_open: float, price_close: float, pnl: float,
                  magic: int = 0, comment: str = "", close_mode: str = "manual") -> Dict:
        """Thêm 1 record vào lịch sử"""
        record = {
            'id': self._generate_id(),
            'ticket': ticket,
            'symbol': symbol,
            'type': type_,
            'volume': volume,
            'price_open': price_open,
            'price_close': price_close,
            'pnl': pnl,
            'magic': magic,
            'comment': comment,
            'close_mode': close_mode,  # manual, batch, target, sl
            'closed_at': datetime.now().isoformat(),
            'date': date.today().isoformat()
        }
        
        history = self._load()
        history.insert(0, record)  # Thêm vào đầu danh sách
        self._save(history)
        
        return record
    
    def add_batch_records(self, results: List[Dict], close_mode: str = "batch") -> List[Dict]:
        """Thêm nhiều records từ kết quả batch close"""
        records = []
        for res in results:
            if res.get('success'):
                record = self.add_record(
                    ticket=res.get('ticket'),
                    symbol=res.get('symbol', 'GOLD'),
                    type_=res.get('type', 'UNKNOWN'),
                    volume=res.get('volume', 0),
                    price_open=res.get('price_open', 0),
                    price_close=res.get('price_close', res.get('price', 0)),
                    pnl=res.get('pnl', 0),
                    magic=res.get('magic', 0),
                    comment=res.get('comment', ''),
                    close_mode=close_mode
                )
                records.append(record)
        return records
    
    def _generate_id(self) -> str:
        """Tạo ID duy nhất"""
        return f"{datetime.now().strftime('%Y%m%d%H%M%S%f')}"
    
    def get_history(self, limit: int = 100, offset: int = 0,
                     date_from: str = None, date_to: str = None,
                     magic: int = None, symbol: str = None) -> Dict[str, Any]:
        """Lấy lịch sử với filter"""
        history = self._load()
        
        # Apply filters
        if date_from:
            history = [h for h in history if h.get('date', '') >= date_from]
        if date_to:
            history = [h for h in history if h.get('date', '') <= date_to]
        if magic is not None:
            history = [h for h in history if h.get('magic') == magic]
        if symbol:
            history = [h for h in history if h.get('symbol') == symbol]
        
        total = len(history)
        paginated = history[offset:offset + limit]
        
        return {
            'records': paginated,
            'total': total,
            'limit': limit,
            'offset': offset,
            'has_more': offset + limit < total
        }
    
    def get_summary(self, days: int = 7) -> Dict[str, Any]:
        """Lấy tóm tắt PnL trong N ngày gần nhất"""
        history = self._load()
        today = date.today().isoformat()
        
        # Filter by days
        cutoff_dates = []
        for i in range(days):
            d = date.today() - __import__('datetime').timedelta(days=i)
            cutoff_dates.append(d.isoformat())
        
        recent = [h for h in history if h.get('date') in cutoff_dates]
        today_records = [h for h in history if h.get('date') == today]
        
        # Calculate stats
        total_pnl = sum(h.get('pnl', 0) for h in recent)
        today_pnl = sum(h.get('pnl', 0) for h in today_records)
        total_count = len(recent)
        today_count = len(today_records)
        
        profit_count = len([h for h in recent if h.get('pnl', 0) > 0])
        loss_count = len([h for h in recent if h.get('pnl', 0) < 0])
        
        # Group by date
        by_date = defaultdict(lambda: {'count': 0, 'pnl': 0})
        for h in recent:
            d = h.get('date', '')
            by_date[d]['count'] += 1
            by_date[d]['pnl'] += h.get('pnl', 0)
        
        # Group by magic
        by_magic = defaultdict(lambda: {'count': 0, 'pnl': 0})
        for h in recent:
            m = h.get('magic', 0)
            by_magic[m]['count'] += 1
            by_magic[m]['pnl'] += h.get('pnl', 0)
        
        return {
            'total_pnl': total_pnl,
            'today_pnl': today_pnl,
            'total_count': total_count,
            'today_count': today_count,
            'profit_count': profit_count,
            'loss_count': loss_count,
            'win_rate': (profit_count / total_count * 100) if total_count > 0 else 0,
            'avg_pnl': total_pnl / total_count if total_count > 0 else 0,
            'by_date': dict(by_date),
            'by_magic': dict(by_magic),
            'days': days
        }
    
    def clear_old_records(self, keep_days: int = 30):
        """Xóa records cũ hơn N ngày"""
        history = self._load()
        cutoff = (date.today() - __import__('datetime').timedelta(days=keep_days)).isoformat()
        filtered = [h for h in history if h.get('date', '') >= cutoff]
        removed = len(history) - len(filtered)
        self._save(filtered)
        return removed
    
    def export_csv(self, filepath: str = None) -> str:
        """Export history ra CSV"""
        if filepath is None:
            filepath = os.path.join(SCRIPT_DIR, f"history_export_{date.today().isoformat()}.csv")
        
        history = self._load()
        
        import csv
        with open(filepath, 'w', newline='', encoding='utf-8') as f:
            writer = csv.DictWriter(f, fieldnames=[
                'id', 'ticket', 'symbol', 'type', 'volume', 
                'price_open', 'price_close', 'pnl', 'magic',
                'comment', 'close_mode', 'closed_at', 'date'
            ])
            writer.writeheader()
            for h in history:
                writer.writerow(h)
        
        return filepath


# Singleton instance
_history_service = None

def get_history_service() -> HistoryService:
    global _history_service
    if _history_service is None:
        _history_service = HistoryService()
    return _history_service
