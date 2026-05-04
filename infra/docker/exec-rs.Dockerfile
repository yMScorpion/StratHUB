# syntax=docker/dockerfile:1.7
FROM rust:1.86-bookworm AS builder
WORKDIR /repo
COPY rust-toolchain.toml Cargo.toml ./
COPY apps/exec-rs apps/exec-rs
COPY packages/strategy-spec/rs packages/strategy-spec/rs
COPY packages/strategy-spec/schema packages/strategy-spec/schema
RUN cargo build --release -p exec-rs

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /repo/target/release/exec-rs /usr/local/bin/exec-rs
USER nonroot
ENTRYPOINT ["/usr/local/bin/exec-rs"]
