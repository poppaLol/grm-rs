FROM rust:1.88.0-slim-bookworm AS builder

WORKDIR /src

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY . .

RUN cargo build -p grm-service-api --release --example local_workspace_server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /src/target/release/examples/local_workspace_server /usr/local/bin/grm-local-workspace-server

RUN useradd --system --create-home --home-dir /var/lib/grm grm \
    && mkdir -p /workspaces \
    && chown -R grm:grm /workspaces /var/lib/grm

USER grm
WORKDIR /var/lib/grm

EXPOSE 50051

CMD ["grm-local-workspace-server", "0.0.0.0:50051", "/workspaces"]
