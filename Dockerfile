# syntax=docker/dockerfile:1
# Multi-stage build with cargo-chef: dependencies are cached in their own layer,
# so a code-only change rebuilds in seconds instead of minutes.

ARG RUST_VERSION=1
# Name of your binary (see [[bin]] / package name in Cargo.toml)
ARG BIN=aura-api

FROM lukemathwalker/cargo-chef:latest-rust-${RUST_VERSION}-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
ARG BIN
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
# If you use sqlx macros: commit the .sqlx/ folder and keep this offline flag
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bin ${BIN} \
  && cp target/release/${BIN} /app/app

# Minimal runtime: no shell, no package manager, runs as non-root
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=builder /app/app /usr/local/bin/app
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/app"]
