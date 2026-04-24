# python_bridge/Dockerfile
# Dockerfile for the Python MT5 -> ZeroMQ bridge
# Note: The MetaTrader5 Python package typically requires a local MT5 terminal (Windows) or
# a properly configured environment (Wine) to actually connect to a broker. Building this
# container will install the MetaTrader5 Python package but running it successfully requires
# MT5 being available in the runtime environment. If you plan to run MT5 on the host (Windows)
# then run the bridge on the host too, or use a Windows container.

FROM python:3.11-slim-bullseye

# Install system dependencies needed to build/ run pyzmq and MetaTrader5
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       build-essential pkg-config libzmq3-dev libssl-dev ca-certificates git \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -s /bin/bash bridgeuser
WORKDIR /home/bridgeuser

# Copy requirements and install dependencies
COPY requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

# Copy the python_bridge source into the image
COPY . ./python_bridge
WORKDIR /home/bridgeuser/python_bridge

# Ensure files are owned by non-root user
RUN chown -R bridgeuser:bridgeuser /home/bridgeuser
USER bridgeuser

# Expose default ZeroMQ ports used by the bridge
EXPOSE 5555 5556 5557

# Default environment variables (can be overridden at runtime)
ENV PYTHONUNBUFFERED=1
ENV ZMQ_MARKET_PORT=5555
ENV ZMQ_ORDER_PORT=5556

# Default command runs the bridge main. You can override with alternate args.
ENTRYPOINT ["python", "-u", "main.py"]
CMD []
