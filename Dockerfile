# ---- Build stage ----
FROM rust:1.98-slim-bookworm AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from your actual source. This is a
# multi-crate workspace, so the dummy-source trick needs a stub for every
# member crate, not just one -- otherwise `cargo build` fails resolving the
# workspace before it ever gets to compiling real dependencies.
COPY Cargo.toml Cargo.lock ./
COPY crates/model/Cargo.toml crates/model/Cargo.toml
COPY crates/db/Cargo.toml crates/db/Cargo.toml
COPY crates/connectors/Cargo.toml crates/connectors/Cargo.toml
COPY crates/workflow-engine/Cargo.toml crates/workflow-engine/Cargo.toml
COPY crates/approvals/Cargo.toml crates/approvals/Cargo.toml
COPY crates/server/Cargo.toml crates/server/Cargo.toml
RUN for crate in model db connectors workflow-engine approvals; do \
        mkdir -p crates/$crate/src && echo "// stub" > crates/$crate/src/lib.rs; \
    done \
    && mkdir -p crates/server/src \
    && echo "fn main() {}" > crates/server/src/main.rs \
    && cargo build --release --workspace \
    && rm -rf crates/*/src

COPY crates crates
COPY migrations migrations
# Force cargo to see the real source as newer than the dummy files it
# already compiled above, so `docker compose up -d --build` after a code
# change only recompiles what actually changed, not every dependency.
RUN find crates -name '*.rs' -exec touch {} + \
    && cargo build --release --bin servcat-server

# ---- Runtime stage ----
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /app --shell /usr/sbin/nologin servcat

COPY --from=builder /app/target/release/servcat-server /app/servcat-server

# Migrations are embedded into the binary at compile time by
# sqlx::migrate!() (see crates/db/src/lib.rs) -- nothing to copy at runtime.
RUN mkdir -p /app/data/uploads && chown -R servcat:servcat /app
USER servcat

EXPOSE 8080
CMD ["/app/servcat-server"]
