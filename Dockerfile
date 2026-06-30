FROM rust:1.88.0-slim-trixie AS builder

ENV RUSTUP_TOOLCHAIN=1.88.0

WORKDIR /src

COPY . .

RUN cargo build --release \
    -p grm-service-api --bin grm-local-workspace-server \
    -p grm-service-api --bin grm-cert-fingerprint \
    -p grm-cli --bin grm

FROM debian:stable-slim

COPY --from=builder /src/target/release/grm-local-workspace-server /usr/local/bin/grm-local-workspace-server
COPY --from=builder /src/target/release/grm-cert-fingerprint /usr/local/bin/grm-cert-fingerprint
COPY --from=builder /src/target/release/grm /usr/local/bin/grm

RUN useradd --system --create-home --home-dir /var/lib/grm grm \
    && mkdir -p /workspaces \
    && chown -R grm:grm /workspaces /var/lib/grm \
    && chmod 700 /workspaces /var/lib/grm

USER grm
WORKDIR /var/lib/grm
ENV GRM_SERVICE_SECURITY_PROFILE=docker_local_insecure

EXPOSE 50051

CMD ["grm-local-workspace-server", "0.0.0.0:50051", "/workspaces"]
