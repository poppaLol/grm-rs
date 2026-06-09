FROM rust:1.88.0-slim-bookworm AS builder

ENV RUSTUP_TOOLCHAIN=1.88.0

WORKDIR /src

COPY . .

RUN cargo build -p grm-service-api --release --bin grm-local-workspace-server

FROM debian:bookworm-slim

COPY --from=builder /src/target/release/grm-local-workspace-server /usr/local/bin/grm-local-workspace-server

RUN useradd --system --create-home --home-dir /var/lib/grm grm \
    && mkdir -p /workspaces \
    && chown -R grm:grm /workspaces /var/lib/grm

USER grm
WORKDIR /var/lib/grm

EXPOSE 50051

CMD ["grm-local-workspace-server", "0.0.0.0:50051", "/workspaces"]
