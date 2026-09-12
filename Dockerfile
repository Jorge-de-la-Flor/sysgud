# syntax=docker/dockerfile:1
FROM rust:1.98.1-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src
RUN cargo build --locked --release --bin sysgud

# Standalone HTTP smoke runner; contains no project credentials or runtime data.
FROM python:3.13-slim-bookworm AS demo
WORKDIR /demo
COPY scripts/smoke-container.py ./smoke-container.py
USER 10001:10001
ENV PYTHONDONTWRITEBYTECODE=1 PYTHONUNBUFFERED=1
ENTRYPOINT ["python", "/demo/smoke-container.py"]

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --no-install-recommends -y ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 sysgud \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin sysgud \
    && mkdir -p /app/.data \
    && chown 10001:10001 /app/.data
WORKDIR /app
COPY --from=builder /build/target/release/sysgud /usr/local/bin/sysgud
ENV SYSGUD_LOAD_DOTENV=false \
    SYSGUD_API_HOST=0.0.0.0 \
    SYSGUD_API_PORT=3000 \
    SYSGUD_DATABASE=/app/.data/sysgud.sqlite \
    SYSGUD_MONITOR_ENABLED=false
USER 10001:10001
EXPOSE 3000
HEALTHCHECK --interval=5s --timeout=3s --start-period=10s --retries=12 \
    CMD curl --fail --silent --show-error http://127.0.0.1:3000/health || exit 1
ENTRYPOINT ["/usr/local/bin/sysgud"]
