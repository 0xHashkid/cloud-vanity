# Build Rust binary
FROM rust:1.88-slim AS builder

WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev
RUN apt update && apt install -y libssl3

COPY . .
RUN cargo build --release && strip target/release/vanity

# Final image
FROM python:3.13-slim-bookworm

RUN apt-get update && apt-get install -y libssl3 && \
    apt-get clean && rm -rf /var/lib/apt/lists/*
# Copy Rust binary
COPY --from=builder /app/target/release/vanity .

# Copy Python files
COPY server.py .
COPY requirements.txt .

# Install Python & deps
RUN python3 -m pip install --no-cache-dir --only-binary=:all: -r requirements.txt --break-system-packages

EXPOSE 8080

CMD ["gunicorn", "--bind", "0.0.0.0:8080", "server:app"]