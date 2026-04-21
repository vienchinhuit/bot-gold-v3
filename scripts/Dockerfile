# ------------------------------------------------------------------
# Docker image chỉ chứa order_monitor.py (Python + ZeroMQ)
# ------------------------------------------------------------------
# Dựa trên Python 3.11 slim – nhẹ, ổn định
FROM python:3.11-slim AS base

# ----------- 1. Cài đặt hệ thống cần thiết ----------
# ZeroMQ (libzmq) và trình biên dịch gói python‑zmq
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        libzmq5 libzmq-dev gcc && \
    rm -rf /var/lib/apt/lists/*

# ----------- 2. Cài đặt thư viện Python ----------
# các package được dùng trong order_monitor.py
COPY requirements.txt /app/requirements.txt
WORKDIR /app
RUN pip install --no-cache-dir -r requirements.txt

# ----------- 3. Sao chép mã nguồn ----------
COPY scripts/order_monitor.py /app/order_monitor.py

# ----------- 4. Thiết lập biến môi trường ----------
ENV PYTHONUNBUFFERED=1    
ENV TZ=UTC                

# ----------- 5. Cổng ZMQ để gửi thông báo tới engine ----------
EXPOSE 5557

# ----------- 6. Entry point ----------
# Khi chạy container sẽ thực thi script
ENTRYPOINT ["python", "/app/order_monitor.py"]