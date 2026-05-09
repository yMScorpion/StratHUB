# syntax=docker/dockerfile:1.7
FROM rust:1.86-bookworm AS builder
WORKDIR /repo
COPY rust-toolchain.toml Cargo.toml ./
COPY apps/exec-rs apps/exec-rs
COPY packages/strategy-spec/rs packages/strategy-spec/rs
COPY packages/strategy-spec/schema packages/strategy-spec/schema
RUN cargo build --release -p exec-rs

FROM debian:12-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates chrony tzdata \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --uid 65532 --home /nonexistent --shell /usr/sbin/nologin nonroot
COPY --from=builder /repo/target/release/exec-rs /usr/local/bin/exec-rs
USER nonroot
ENTRYPOINT ["/usr/local/bin/exec-rs"]
