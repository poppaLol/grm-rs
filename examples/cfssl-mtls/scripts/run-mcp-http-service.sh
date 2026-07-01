#!/bin/sh
set -eu

MAX_ATTEMPTS="${GRM_MCP_HTTP_START_ATTEMPTS:-10}"
ALLOWED_HOSTS="${GRM_MCP_HTTP_ALLOWED_HOSTS:-grm-mcp-http grm-mcp-http:8080}"
attempt=1

set -- grm-mcp \
  --transport http \
  --http-bind "${GRM_MCP_HTTP_BIND:-0.0.0.0:8080}" \
  --http-path "${GRM_MCP_HTTP_PATH:-/mcp}"
for host in $ALLOWED_HOSTS; do
  set -- "$@" --http-allowed-host "$host"
done

while [ "$attempt" -le "$MAX_ATTEMPTS" ]; do
  if "$@"; then
    exit 0
  fi

  status="$?"
  if [ "$attempt" -eq "$MAX_ATTEMPTS" ]; then
    echo "grm-mcp HTTP service failed after $attempt attempts; exiting" >&2
    exit "$status"
  fi

  echo "grm-mcp HTTP service failed to start; retrying for local demo readiness" >&2
  attempt=$((attempt + 1))
  sleep 1
done
