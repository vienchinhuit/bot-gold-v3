FROM python:3.12-slim

WORKDIR /app

# Install dependencies
RUN pip install --no-cache-dir zeromq

# Copy application files
COPY . /app

# Default command - shows help
CMD ["python", "-c", "print('Engine-rust help: python engine.py --help')"]