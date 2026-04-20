"""
Web Dashboard: Hiển thị thông tin orders và positions
Chạy: python dashboard.py
Truy cập: http://localhost:5000
"""

import zmq
import json
import time
import threading
from flask import Flask, render_template_string, jsonify, request
from datetime import datetime, date
from collections import defaultdict

# Import history service
import sys
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from history_service import get_history_service

# Cấu hình
PORT = 5556
REFRESH_INTERVAL = 1

app = Flask(__name__)

# ========== ZMQ Client ==========

class OrderClient:
    def __init__(self, port=5556):
        self.port = port
        self.context = None
        self.socket = None
        self._running = False
        self._thread = None
        self._response_queue = []
        self._lock = threading.Lock()
        
    def connect(self):
        self.context = zmq.Context()
        self.socket = self.context.socket(zmq.DEALER)
        self.socket.connect(f"tcp://localhost:{self.port}")
        self.socket.setsockopt(zmq.RCVTIMEO, 5000)
        self._running = True
        self._thread = threading.Thread(target=self._receive_loop, daemon=True)
        self._thread.start()
        
    def _receive_loop(self):
        while self._running:
            try:
                msg = self.socket.recv_json()
                with self._lock:
                    self._response_queue.append(msg)
            except zmq.Again:
                pass
            except Exception:
                pass
    
    def send(self, msg_type, data):
        message = {"type": msg_type, "data": data}
        self.socket.send_json(message)
    
    def get_response(self, timeout=10):
        start = time.time()
        while time.time() - start < timeout:
            with self._lock:
                if self._response_queue:
                    return self._response_queue.pop(0)
            time.sleep(0.05)
        return None
    
    def close(self):
        self._running = False
        if self._thread:
            self._thread.join(timeout=2)
        if self.socket:
            self.socket.close()
        if self.context:
            self.context.term()


# ========== Global Client ==========

client = OrderClient(port=PORT)


# ========== API Endpoints ==========

@app.route('/')
def index():
    return render_template_string(HTML_TEMPLATE)


@app.route('/api/positions')
def api_positions():
    client.send("ORDER_INFO", {})
    response = client.get_response(timeout=5)
    
    if response and response.get('success'):
        positions = response.get('positions', [])
        total_pnl = sum(p.get('profit', 0) for p in positions)
        total_volume = sum(p.get('volume', 0) for p in positions)
        
        return jsonify({
            'success': True,
            'positions': positions,
            'total_pnl': total_pnl,
            'total_volume': total_volume,
            'count': len(positions),
            'timestamp': datetime.now().isoformat()
        })
    
    return jsonify({'success': False, 'error': 'No response'})


@app.route('/api/close_batch', methods=['POST'])
def api_close_batch():
    data = request.json
    tickets = data.get('tickets', [])
    max_workers = data.get('max_workers', 10)
    
    if not tickets:
        return jsonify({'success': False, 'error': 'No tickets'})
    
    payload = {
        "type": "POSITION_CLOSE_BATCH",
        "data": {
            "tickets": tickets,
            "max_workers": max_workers,
            "save_history": True
        }
    }
    client.send(payload["type"], payload["data"])
    response = client.get_response(timeout=30)
    
    if response:
        return jsonify(response)
    
    return jsonify({'success': False, 'error': 'No response'})


@app.route('/api/close_single', methods=['POST'])
def api_close_single():
    data = request.json
    ticket = data.get('ticket')
    
    if not ticket:
        return jsonify({'success': False, 'error': 'No ticket'})
    
    client.send("POSITION_CLOSE", {"ticket": ticket, "volume": 0})
    response = client.get_response(timeout=10)
    
    if response:
        return jsonify(response)
    
    return jsonify({'success': False, 'error': 'No response'})


# ========== History API ==========

@app.route('/api/history')
def api_history():
    history_svc = get_history_service()
    
    limit = int(request.args.get('limit', 50))
    offset = int(request.args.get('offset', 0))
    date_from = request.args.get('date_from')
    date_to = request.args.get('date_to')
    magic = request.args.get('magic')
    
    result = history_svc.get_history(
        limit=limit, offset=offset,
        date_from=date_from, date_to=date_to,
        magic=int(magic) if magic else None
    )
    
    return jsonify(result)


@app.route('/api/history/summary')
def api_history_summary():
    history_svc = get_history_service()
    return jsonify(history_svc.get_summary(days=7))


@app.route('/api/history/stats')
def api_history_stats():
    history_svc = get_history_service()
    today = date.today().isoformat()
    history = history_svc._load()
    today_records = [h for h in history if h.get('date') == today]
    today_pnl = sum(h.get('pnl', 0) for h in today_records)
    summary = history_svc.get_summary(days=7)
    
    return jsonify({
        'today_pnl': today_pnl,
        'today_count': len(today_records),
        'week_pnl': summary['total_pnl'],
        'week_count': summary['total_count'],
        'win_rate': summary['win_rate'],
        'profit_count': summary['profit_count'],
        'loss_count': summary['loss_count']
    })


@app.route('/api/history/export')
def api_history_export():
    history_svc = get_history_service()
    filepath = history_svc.export_csv()
    return jsonify({'success': True, 'file': filepath})


# ========== HTML Template ==========

HTML_TEMPLATE = '''
<!DOCTYPE html>
<html lang="vi">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Trading Dashboard</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: #eee;
            min-height: 100vh;
            padding: 20px;
        }
        .container { max-width: 1400px; margin: 0 auto; }
        header {
            display: flex; justify-content: space-between; align-items: center;
            padding: 20px; background: rgba(255,255,255,0.05);
            border-radius: 15px; margin-bottom: 20px;
        }
        h1 { color: #00d4ff; font-size: 1.8em; }
        .status { display: flex; align-items: center; gap: 10px; }
        .status-dot {
            width: 12px; height: 12px; border-radius: 50%;
            background: #4ade80; animation: pulse 2s infinite;
        }
        .status-dot.disconnected { background: #ef4444; animation: none; }
        @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.5; } }
        
        .tabs { display: flex; gap: 10px; margin-bottom: 20px; }
        .tab-btn {
            padding: 12px 24px; background: rgba(255,255,255,0.05);
            border: none; border-radius: 10px 10px 0 0;
            color: #888; cursor: pointer; font-size: 1em;
        }
        .tab-btn.active { background: rgba(0, 212, 255, 0.15); color: #00d4ff; }
        .tab-btn:hover { background: rgba(255,255,255,0.1); }
        .tab-content { display: none; }
        .tab-content.active { display: block; }
        
        .stats-grid {
            display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 15px; margin-bottom: 20px;
        }
        .stat-card {
            background: rgba(255,255,255,0.08); border-radius: 12px;
            padding: 20px; text-align: center;
        }
        .stat-label { font-size: 0.85em; color: #888; margin-bottom: 5px; }
        .stat-value { font-size: 1.8em; font-weight: bold; }
        .stat-value.positive { color: #4ade80; }
        .stat-value.negative { color: #ef4444; }
        .stat-value.neutral { color: #00d4ff; }
        
        .section {
            background: rgba(255,255,255,0.05); border-radius: 15px;
            padding: 20px; margin-bottom: 20px;
        }
        .section-header {
            display: flex; justify-content: space-between;
            align-items: center; margin-bottom: 15px;
        }
        .section-title { font-size: 1.2em; color: #00d4ff; }
        
        .btn {
            padding: 10px 20px; border: none; border-radius: 8px;
            cursor: pointer; font-size: 0.9em; font-weight: 600;
            transition: all 0.3s;
        }
        .btn-primary { background: #00d4ff; color: #1a1a2e; }
        .btn-primary:hover { background: #00b4d8; }
        .btn-danger { background: #ef4444; color: white; }
        .btn-danger:hover { background: #dc2626; }
        .btn-success { background: #4ade80; color: #1a1a2e; }
        .btn-success:hover { background: #22c55e; }
        .btn-small { padding: 5px 10px; font-size: 0.8em; }
        .btn:disabled { opacity: 0.5; cursor: not-allowed; }
        
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 12px; text-align: left; border-bottom: 1px solid rgba(255,255,255,0.1); }
        th { color: #888; font-weight: 600; font-size: 0.85em; }
        tr:hover { background: rgba(255,255,255,0.05); }
        
        .type-badge {
            padding: 4px 10px; border-radius: 4px;
            font-size: 0.8em; font-weight: 600;
        }
        .type-badge.BUY { background: rgba(74, 222, 128, 0.2); color: #4ade80; }
        .type-badge.SELL { background: rgba(239, 68, 68, 0.2); color: #ef4444; }
        
        .pnl-value { font-weight: 600; }
        .pnl-value.positive { color: #4ade80; }
        .pnl-value.negative { color: #ef4444; }
        
        .magic-group {
            background: rgba(0, 212, 255, 0.1); padding: 10px 15px;
            border-radius: 8px; margin-bottom: 15px; border-left: 4px solid #00d4ff;
        }
        .magic-header {
            display: flex; justify-content: space-between;
            align-items: center; margin-bottom: 10px;
        }
        .magic-title { font-weight: 600; color: #00d4ff; }
        .magic-stats { font-size: 0.85em; color: #888; }
        
        .empty-state { text-align: center; padding: 40px; color: #888; }
        .loading { text-align: center; padding: 20px; color: #888; }
        .last-updated { font-size: 0.8em; color: #666; text-align: center; margin-top: 20px; }
        
        .history-card {
            background: rgba(255,255,255,0.05); border-radius: 10px;
            padding: 15px; text-align: center;
        }
        .history-card-label { font-size: 0.8em; color: #888; margin-bottom: 5px; }
        .history-card-value { font-size: 1.4em; font-weight: bold; }
        
        .mode-badge { padding: 3px 8px; border-radius: 4px; font-size: 0.75em; font-weight: 600; }
        .mode-badge.manual { background: rgba(100, 116, 139, 0.3); color: #94a3b8; }
        .mode-badge.batch { background: rgba(34, 197, 94, 0.2); color: #4ade80; }
        
        .chart-container { background: rgba(255,255,255,0.03); border-radius: 10px; padding: 15px; margin: 20px 0; }
        .chart-bar { display: flex; align-items: flex-end; gap: 4px; height: 100px; margin-top: 10px; }
        .chart-bar-item { flex: 1; border-radius: 4px 4px 0 0; min-width: 20px; transition: all 0.3s; }
        .chart-bar-item.positive { background: #4ade80; }
        .chart-bar-item.negative { background: #ef4444; }
        .chart-label { font-size: 0.7em; color: #666; text-align: center; margin-top: 5px; }
        
        .pagination { display: flex; justify-content: center; gap: 10px; margin-top: 15px; align-items: center; }
        .pagination button { padding: 8px 15px; background: rgba(255,255,255,0.1); border: none; border-radius: 6px; color: #fff; cursor: pointer; }
        
        .filters { display: flex; gap: 10px; margin-bottom: 15px; flex-wrap: wrap; align-items: center; }
        .filter-group { display: flex; align-items: center; gap: 5px; }
        .filter-group label { font-size: 0.85em; color: #888; }
        .filter-group input, .filter-group select {
            padding: 8px 12px; background: rgba(255,255,255,0.1);
            border: 1px solid rgba(255,255,255,0.2); border-radius: 6px;
            color: #fff; font-size: 0.9em;
        }
    </style>
</head>
<body>
    <div class="container">
        <header>
            <h1>📊 Trading Dashboard</h1>
            <div class="status">
                <div class="status-dot" id="statusDot"></div>
                <span id="statusText">Connecting...</span>
            </div>
        </header>
        
        <div class="tabs">
            <button class="tab-btn active" onclick="switchTab('positions')">📋 Open Positions</button>
            <button class="tab-btn" onclick="switchTab('history')">📜 History</button>
        </div>
        
        <!-- Positions Tab -->
        <div class="tab-content active" id="tab-positions">
            <div class="stats-grid">
                <div class="stat-card">
                    <div class="stat-label">Tổng Positions</div>
                    <div class="stat-value neutral" id="totalPositions">-</div>
                </div>
                <div class="stat-card">
                    <div class="stat-label">Tổng Volume</div>
                    <div class="stat-value neutral" id="totalVolume">-</div>
                </div>
                <div class="stat-card">
                    <div class="stat-label">Unrealized PnL</div>
                    <div class="stat-value neutral" id="totalPnl">-</div>
                </div>
                <div class="stat-card">
                    <div class="stat-label">Hôm nay đã đóng</div>
                    <div class="stat-value neutral" id="closedToday">-</div>
                </div>
            </div>
            
            <div class="section">
                <div class="section-header">
                    <div class="section-title">📋 Open Positions</div>
                    <div>
                        <button class="btn btn-danger" id="btnCloseSelected" onclick="closeSelected()" disabled>🗑️ Đóng Selected</button>
                        <button class="btn btn-success" onclick="closeAll()">✅ Đóng Tất Cả</button>
                        <button class="btn btn-primary" onclick="refreshData()">🔄 Refresh</button>
                    </div>
                </div>
                <div id="positionsContainer">
                    <div class="loading">Đang tải dữ liệu...</div>
                </div>
            </div>
        </div>
        
        <!-- History Tab -->
        <div class="tab-content" id="tab-history">
            <div class="stats-grid">
                <div class="history-card">
                    <div class="history-card-label">Hôm nay PnL</div>
                    <div class="history-card-value" id="histTodayPnl">-</div>
                </div>
                <div class="history-card">
                    <div class="history-card-label">Hôm nay đã đóng</div>
                    <div class="history-card-value" id="histTodayCount">-</div>
                </div>
                <div class="history-card">
                    <div class="history-card-label">7 ngày PnL</div>
                    <div class="history-card-value" id="histWeekPnl">-</div>
                </div>
                <div class="history-card">
                    <div class="history-card-label">Win Rate</div>
                    <div class="history-card-value" id="histWinRate">-</div>
                </div>
            </div>
            
            <div class="chart-container">
                <div class="section-title">📈 PnL 7 ngày gần nhất</div>
                <div class="chart-bar" id="chartBar">
                    <div style="text-align: center; color: #666; width: 100%;">Đang tải...</div>
                </div>
            </div>
            
            <div class="section">
                <div class="section-header">
                    <div class="section-title">📜 Lịch sử đã đóng</div>
                    <div>
                        <button class="btn btn-primary btn-small" onclick="loadHistory()">🔄 Refresh</button>
                        <button class="btn btn-success btn-small" onclick="exportHistory()">📥 Export</button>
                    </div>
                </div>
                
                <div class="filters">
                    <div class="filter-group">
                        <label>Từ:</label>
                        <input type="date" id="filterDateFrom">
                    </div>
                    <div class="filter-group">
                        <label>Đến:</label>
                        <input type="date" id="filterDateTo">
                    </div>
                    <div class="filter-group">
                        <label>Magic:</label>
                        <input type="number" id="filterMagic" placeholder="All" style="width: 80px;">
                    </div>
                    <button class="btn btn-primary btn-small" onclick="loadHistory()">Lọc</button>
                    <button class="btn btn-small" onclick="clearFilters()">Reset</button>
                </div>
                
                <div id="historyContainer">
                    <div class="loading">Đang tải lịch sử...</div>
                </div>
                
                <div class="pagination">
                    <button id="btnPrev" onclick="prevPage()">◀ Trước</button>
                    <span id="pageInfo">Trang 1</span>
                    <button id="btnNext" onclick="nextPage()">Sau ▶</button>
                </div>
            </div>
        </div>
        
        <div class="last-updated">
            Last updated: <span id="lastUpdated">-</span>
        </div>
    </div>
    
    <script>
        // ========== Global State ==========
        let positions = [];
        let selectedTickets = new Set();
        let historyOffset = 0;
        const historyLimit = 30;
        let historyHasMore = false;
        
        // ========== Utility Functions ==========
        function fmt(num, dec = 2) {
            if (num === null || num === undefined || isNaN(num)) return '0.00';
            return num.toFixed(dec).replace(/\B(?=(\d{3})+(?!\d))/g, ',');
        }
        
        function fmtPnl(value) {
            if (value === null || value === undefined || isNaN(value)) value = 0;
            const cls = value >= 0 ? 'positive' : 'negative';
            const sign = value >= 0 ? '+' : '';
            return '<span class="pnl-value ' + cls + '">' + sign + '$' + fmt(Math.abs(value)) + '</span>';
        }
        
        // ========== Tab Switching ==========
        function switchTab(tabName) {
            document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
            document.querySelectorAll('.tab-content').forEach(c => c.classList.remove('active'));
            document.querySelector('.tab-btn[onclick="switchTab(\'' + tabName + '\')"]').classList.add('active');
            document.getElementById('tab-' + tabName).classList.add('active');
            
            if (tabName === 'history') {
                loadHistorySummary();
                loadHistory();
            }
        }
        
        // ========== Positions Functions ==========
        function refreshData() {
            fetch('/api/positions')
                .then(r => r.json())
                .then(data => {
                    if (data.success) {
                        positions = data.positions || [];
                        updateStats(data);
                        renderPositions(positions);
                        document.getElementById('statusDot').classList.remove('disconnected');
                        document.getElementById('statusText').textContent = 'Connected';
                    } else {
                        showDisconnected();
                    }
                })
                .catch(err => {
                    console.error('Error:', err);
                    showDisconnected();
                });
        }
        
        function showDisconnected() {
            document.getElementById('statusDot').classList.add('disconnected');
            document.getElementById('statusText').textContent = 'Disconnected';
            document.getElementById('positionsContainer').innerHTML = 
                '<div class="empty-state">❌ Không thể kết nối đến Bridge<br>Hãy chắc chắn Python Bridge đang chạy!</div>';
        }
        
        function updateStats(data) {
            document.getElementById('totalPositions').textContent = data.count || 0;
            document.getElementById('totalVolume').textContent = fmt(data.total_volume) + ' lots';
            
            const pnlEl = document.getElementById('totalPnl');
            pnlEl.innerHTML = fmtPnl(data.total_pnl);
            pnlEl.className = 'stat-value ' + (data.total_pnl >= 0 ? 'positive' : 'negative');
            
            document.getElementById('lastUpdated').textContent = new Date().toLocaleTimeString('vi-VN');
            
            fetch('/api/history/stats')
                .then(r => r.json())
                .then(s => {
                    document.getElementById('closedToday').textContent = s.today_count || 0;
                });
        }
        
        function renderPositions(positions) {
            const container = document.getElementById('positionsContainer');
            
            if (!positions || positions.length === 0) {
                container.innerHTML = '<div class="empty-state">🎉 Không có positions nào đang mở!</div>';
                return;
            }
            
            const groups = {};
            positions.forEach(p => {
                const magic = p.magic || 0;
                if (!groups[magic]) groups[magic] = [];
                groups[magic].push(p);
            });
            
            let html = '';
            for (const [magic, posList] of Object.entries(groups)) {
                const groupPnl = posList.reduce((sum, p) => sum + (p.profit || 0), 0);
                const pnlStr = groupPnl >= 0 ? '+$' + fmt(groupPnl) : '-$' + fmt(Math.abs(groupPnl));
                
                html += '<div class="magic-group">' +
                    '<div class="magic-header">' +
                        '<div class="magic-title">Magic #' + magic + '</div>' +
                        '<div class="magic-stats">' +
                            posList.length + ' pos | PnL: ' + pnlStr +
                            ' <button class="btn btn-success btn-small" onclick="closeGroup(' + magic + ')">Đóng Group</button>' +
                        '</div>' +
                    '</div>' +
                    '<table><thead><tr>' +
                        '<th><input type="checkbox" onchange="toggleSelectAll(' + magic + ', this)"></th>' +
                        '<th>Ticket</th><th>Symbol</th><th>Type</th><th>Volume</th>' +
                        '<th>Entry</th><th>Current</th><th>PnL</th><th>Action</th>' +
                    '</tr></thead><tbody>';
                
                posList.forEach(p => {
                    html += '<tr>' +
                        '<td><input type="checkbox" class="pos-cb" data-magic="' + magic + '" data-ticket="' + p.ticket + '" onchange="toggleTicket(' + p.ticket + ')"></td>' +
                        '<td>#' + p.ticket + '</td>' +
                        '<td>' + p.symbol + '</td>' +
                        '<td><span class="type-badge ' + p.type + '">' + p.type + '</span></td>' +
                        '<td>' + fmt(p.volume) + '</td>' +
                        '<td>' + fmt(p.price_open) + '</td>' +
                        '<td>' + fmt(p.price_current) + '</td>' +
                        '<td>' + fmtPnl(p.profit) + '</td>' +
                        '<td><button class="btn btn-danger btn-small" onclick="closeSingle(' + p.ticket + ')">Close</button></td>' +
                    '</tr>';
                });
                
                html += '</tbody></table></div>';
            }
            
            container.innerHTML = html;
        }
        
        // ========== Selection Functions ==========
        function toggleTicket(ticket) {
            const cb = document.querySelector('.pos-cb[data-ticket="' + ticket + '"]');
            if (cb.checked) {
                selectedTickets.add(ticket);
            } else {
                selectedTickets.delete(ticket);
            }
            updateSelectButtons();
        }
        
        function toggleSelectAll(magic, masterCb) {
            document.querySelectorAll('.pos-cb[data-magic="' + magic + '"]').forEach(cb => {
                cb.checked = masterCb.checked;
                if (masterCb.checked) {
                    selectedTickets.add(parseInt(cb.dataset.ticket));
                } else {
                    selectedTickets.delete(parseInt(cb.dataset.ticket));
                }
            });
            updateSelectButtons();
        }
        
        function updateSelectButtons() {
            const btn = document.getElementById('btnCloseSelected');
            if (btn) {
                btn.disabled = selectedTickets.size === 0;
                btn.textContent = '🗑️ Đóng Selected (' + selectedTickets.size + ')';
            }
        }
        
        // ========== Close Functions ==========
        function closeSingle(ticket) {
            if (!confirm('Đóng position #' + ticket + '?')) return;
            
            fetch('/api/close_single', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({ticket: ticket})
            })
            .then(r => r.json())
            .then(data => {
                if (data.success) {
                    alert('Đã đóng #' + ticket + '!');
                } else {
                    alert('Lỗi: ' + (data.error_message || 'Unknown'));
                }
                refreshData();
            })
            .catch(err => alert('Lỗi: ' + err));
        }
        
        function closeGroup(magic) {
            const tickets = positions.filter(p => p.magic === magic).map(p => p.ticket);
            if (!confirm('Đóng ' + tickets.length + ' positions trong Magic #' + magic + '?')) return;
            
            fetch('/api/close_batch', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({tickets: tickets})
            })
            .then(r => r.json())
            .then(data => {
                if (data.success) {
                    alert('Đã đóng ' + data.closed + '/' + tickets.length + ' positions!');
                } else {
                    alert('Đóng: ' + data.closed + ', Thất bại: ' + data.failed);
                }
                selectedTickets.clear();
                refreshData();
            })
            .catch(err => alert('Lỗi: ' + err));
        }
        
        function closeSelected() {
            if (selectedTickets.size === 0) {
                alert('Chọn ít nhất 1 position!');
                return;
            }
            if (!confirm('Đóng ' + selectedTickets.size + ' positions đã chọn?')) return;
            
            fetch('/api/close_batch', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({tickets: Array.from(selectedTickets)})
            })
            .then(r => r.json())
            .then(data => {
                if (data.success) {
                    alert('Đã đóng ' + data.closed + '/' + selectedTickets.size + ' positions!');
                } else {
                    alert('Đóng: ' + data.closed + ', Thất bại: ' + data.failed);
                }
                selectedTickets.clear();
                refreshData();
            })
            .catch(err => alert('Lỗi: ' + err));
        }
        
        function closeAll() {
            if (positions.length === 0) {
                alert('Không có positions nào!');
                return;
            }
            if (!confirm('Đóng TẤT CẢ ' + positions.length + ' positions?')) return;
            
            const tickets = positions.map(p => p.ticket);
            fetch('/api/close_batch', {
                method: 'POST',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({tickets: tickets})
            })
            .then(r => r.json())
            .then(data => {
                if (data.success) {
                    alert('Đã đóng ' + data.closed + '/' + tickets.length + ' positions!');
                } else {
                    alert('Đóng: ' + data.closed + ', Thất bại: ' + data.failed);
                }
                selectedTickets.clear();
                refreshData();
            })
            .catch(err => alert('Lỗi: ' + err));
        }
        
        // ========== History Functions ==========
        function loadHistorySummary() {
            fetch('/api/history/summary?days=7')
                .then(r => r.json())
                .then(data => {
                    document.getElementById('histTodayPnl').innerHTML = fmtPnl(data.today_pnl || 0);
                    document.getElementById('histTodayCount').textContent = data.today_count || 0;
                    document.getElementById('histWeekPnl').innerHTML = fmtPnl(data.total_pnl || 0);
                    document.getElementById('histWinRate').textContent = (data.win_rate || 0).toFixed(1) + '%';
                    drawChart(data.by_date || {});
                });
        }
        
        function drawChart(byDate) {
            const container = document.getElementById('chartBar');
            const dates = Object.keys(byDate).sort().slice(-7);
            
            if (dates.length === 0) {
                container.innerHTML = '<div style="text-align: center; color: #666; width: 100%;">Chưa có dữ liệu</div>';
                return;
            }
            
            const pnls = dates.map(d => byDate[d].pnl);
            const maxAbs = Math.max(...pnls.map(p => Math.abs(p)), 1);
            
            let html = '';
            dates.forEach((d, i) => {
                const pnl = pnls[i];
                const height = Math.max(Math.abs(pnl) / maxAbs * 80, 5);
                const cls = pnl >= 0 ? 'positive' : 'negative';
                const label = new Date(d).toLocaleDateString('vi-VN', {weekday: 'short', day: 'numeric'});
                html += '<div style="flex: 1; text-align: center;">' +
                    '<div class="chart-bar-item ' + cls + '" style="height: ' + height + 'px;" title="' + label + ': $' + fmt(pnl) + '"></div>' +
                    '<div class="chart-label">' + label + '</div>' +
                '</div>';
            });
            container.innerHTML = html;
        }
        
        function loadHistory() {
            const dateFrom = document.getElementById('filterDateFrom').value;
            const dateTo = document.getElementById('filterDateTo').value;
            const magic = document.getElementById('filterMagic').value;
            
            let url = '/api/history?limit=' + historyLimit + '&offset=' + historyOffset;
            if (dateFrom) url += '&date_from=' + dateFrom;
            if (dateTo) url += '&date_to=' + dateTo;
            if (magic) url += '&magic=' + magic;
            
            fetch(url)
                .then(r => r.json())
                .then(data => {
                    historyHasMore = data.has_more || false;
                    renderHistoryTable(data.records || []);
                    updatePagination(data);
                });
        }
        
        function renderHistoryTable(records) {
            const container = document.getElementById('historyContainer');
            
            if (!records || records.length === 0) {
                container.innerHTML = '<div class="empty-state">📭 Chưa có lịch sử nào!</div>';
                return;
            }
            
            let html = '<table><thead><tr>' +
                '<th>Time</th><th>Ticket</th><th>Symbol</th><th>Type</th>' +
                '<th>Volume</th><th>Entry</th><th>Close</th><th>PnL</th>' +
                '<th>Magic</th><th>Mode</th>' +
            '</tr></thead><tbody>';
            
            records.forEach(r => {
                const time = new Date(r.closed_at).toLocaleString('vi-VN');
                html += '<tr>' +
                    '<td>' + time + '</td>' +
                    '<td>#' + r.ticket + '</td>' +
                    '<td>' + r.symbol + '</td>' +
                    '<td><span class="type-badge ' + r.type + '">' + r.type + '</span></td>' +
                    '<td>' + fmt(r.volume) + '</td>' +
                    '<td>' + fmt(r.price_open) + '</td>' +
                    '<td>' + fmt(r.price_close) + '</td>' +
                    '<td>' + fmtPnl(r.pnl) + '</td>' +
                    '<td>#' + (r.magic || 0) + '</td>' +
                    '<td><span class="mode-badge ' + r.close_mode + '">' + r.close_mode + '</span></td>' +
                '</tr>';
            });
            
            html += '</tbody></table>';
            container.innerHTML = html;
        }
        
        function updatePagination(data) {
            const currentPage = Math.floor((data.offset || 0) / historyLimit) + 1;
            document.getElementById('pageInfo').textContent = 
                'Trang ' + currentPage + ' (' + ((data.offset || 0) + 1) + '-' + Math.min((data.offset || 0) + historyLimit, data.total || 0) + ' of ' + (data.total || 0) + ')';
            document.getElementById('btnPrev').disabled = (data.offset || 0) === 0;
            document.getElementById('btnNext').disabled = !historyHasMore;
        }
        
        function prevPage() {
            if (historyOffset > 0) {
                historyOffset -= historyLimit;
                loadHistory();
            }
        }
        
        function nextPage() {
            if (historyHasMore) {
                historyOffset += historyLimit;
                loadHistory();
            }
        }
        
        function clearFilters() {
            document.getElementById('filterDateFrom').value = '';
            document.getElementById('filterDateTo').value = '';
            document.getElementById('filterMagic').value = '';
            historyOffset = 0;
            loadHistory();
        }
        
        function exportHistory() {
            fetch('/api/history/export')
                .then(r => r.json())
                .then(data => {
                    if (data.success) alert('Đã export: ' + data.file);
                });
        }
        
        // ========== Auto Refresh ==========
        setInterval(refreshData, 2000);
        
        // ========== Initial Load ==========
        refreshData();
    </script>
</body>
</html>
'''


# ========== Main ==========

if __name__ == '__main__':
    print("=" * 50)
    print("🚀 Trading Dashboard")
    print("=" * 50)
    print("  Kết nối ZMQ port:", PORT)
    print("  Truy cập: http://localhost:5000")
    print("  Nhấn Ctrl+C để dừng")
    print("=" * 50)
    
    client.connect()
    app.run(host='0.0.0.0', port=5000, debug=False)
    client.close()
