FROM rust:1.88.0-slim-trixie AS builder

ENV RUSTUP_TOOLCHAIN=1.88.0

WORKDIR /src

COPY . .

RUN cargo build --release \
    -p grm-service-api --bin grm-local-workspace-server \
    -p grm-service-api --bin grm-cert-fingerprint \
    -p grm-cli --bin grm \
    -p grm-mcp --bin grm-mcp \
    -p grm-mcp --bin grm-mcp-http-smoke

FROM debian:stable-slim AS runtime-base

RUN useradd --system --create-home --home-dir /var/lib/grm grm \
    && mkdir -p /workspaces \
    && chown -R grm:grm /workspaces /var/lib/grm \
    && chmod 700 /workspaces /var/lib/grm

USER grm
WORKDIR /var/lib/grm
ENV GRM_SERVICE_SECURITY_PROFILE=docker_local_insecure

FROM runtime-base AS grm-service-runtime

COPY --from=builder /src/target/release/grm-local-workspace-server /usr/local/bin/grm-local-workspace-server
COPY --from=builder /src/target/release/grm-cert-fingerprint /usr/local/bin/grm-cert-fingerprint

EXPOSE 50051

CMD ["grm-local-workspace-server", "0.0.0.0:50051", "/workspaces"]

FROM runtime-base AS grm-cli-runtime

COPY --from=builder /src/target/release/grm /usr/local/bin/grm

FROM runtime-base AS grm-mcp-runtime

COPY --from=builder /src/target/release/grm-mcp /usr/local/bin/grm-mcp

EXPOSE 8080

CMD ["grm-mcp", "--transport", "http", "--http-bind", "0.0.0.0:8080", "--http-path", "/mcp"]

FROM runtime-base AS grm-mcp-smoke-runtime

COPY --from=builder /src/target/release/grm-mcp /usr/local/bin/grm-mcp
COPY --from=builder /src/target/release/grm-mcp-http-smoke /usr/local/bin/grm-mcp-http-smoke

FROM grm-service-runtime AS default
