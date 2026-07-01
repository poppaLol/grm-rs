#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
COMPOSE_FILE="$REPO_ROOT/docker-compose.cfssl-mtls.yml"
PROJECT_NAME="${COMPOSE_PROJECT_NAME:-grm-cfssl-mtls-demo}"

compose() {
  docker compose -p "$PROJECT_NAME" -f "$COMPOSE_FILE" "$@"
}

cleanup() {
  status=$?
  compose --profile smoke down
  exit "$status"
}
trap cleanup EXIT INT TERM

compose --profile smoke up --build -d grm-secured grm-mcp-http
compose --profile smoke run --rm --no-deps grm-secured-smoke
compose --profile smoke run --rm --no-deps grm-mcp-http-smoke
