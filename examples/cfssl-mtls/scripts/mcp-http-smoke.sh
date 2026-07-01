#!/bin/sh
set -eu

CERT_DIR="${CERT_DIR:-/certs}"
ENDPOINT="${GRM_SERVICE_ENDPOINT:-https://grm-secured:50051}"
MCP_ENDPOINT="${GRM_MCP_HTTP_ENDPOINT:-http://grm-mcp-http:8080/mcp}"
WORKSPACE_PREFIX="${WORKSPACE_PREFIX:-cfssl-mcp-http}"
RUN_ID="$(date +%s)"

expect_no_secret_leak() {
  name="$1"
  output_file="$2"
  if grep -E "BEGIN (RSA |EC |)PRIVATE KEY|BEGIN CERTIFICATE|mapped-client-key|limited-client-key|unmapped-client-key" "$output_file" >/dev/null; then
    echo "failure output leaked certificate or key material for $name" >&2
    exit 1
  fi
}

print_checked_output() {
  name="$1"
  output_file="$2"
  expect_no_secret_leak "$name" "$output_file"
  cat "$output_file" >&2
}

expect_limited_permission_denied() {
  name="mapped-principal-without-required-permission"
  endpoint="http://127.0.0.1:18083/mcp"
  output_file="/tmp/grm-mcp-${name}.out"
  assert_output_file="/tmp/grm-mcp-limited-assert.out"
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    if ! kill -0 "$limited_pid" 2>/dev/null; then
      wait "$limited_pid" 2>/dev/null || true
      echo "limited-principal MCP server exited before permission-denied assertion" >&2
      print_checked_output "$name" "$output_file"
      exit 1
    fi
    if grm-mcp-http-smoke --expect-schema-list-permission-denied "$endpoint" >"$assert_output_file" 2>&1; then
      echo "ok: $name failed schema inspection with permission denied"
      return 0
    fi
    sleep 1
  done

  echo "limited-principal MCP permission-denied assertion did not pass" >&2
  print_checked_output "$name assertion" "$assert_output_file"
  print_checked_output "$name" "$output_file"
  exit 1
}

wait_for_mcp() {
  endpoint="$1"
  name="$2"
  output_file="/tmp/grm-mcp-ready.out"
  for attempt in 1 2 3 4 5 6 7 8 9 10; do
    if grm-mcp-http-smoke "$endpoint" >"$output_file" 2>&1; then
      return 0
    fi
    if [ "$attempt" = "10" ]; then
      echo "MCP HTTP endpoint did not become ready for $name" >&2
      print_checked_output "$name readiness" "$output_file"
      return 1
    fi
    sleep 1
  done
}

start_temp_mcp() {
  name="$1"
  port="$2"
  cert="$3"
  key="$4"
  workspace="$5"
  output_file="/tmp/grm-mcp-${name}.out"
  export GRM_BACKEND=grpc
  export GRM_SERVICE_ENDPOINT="$ENDPOINT"
  export GRM_WORKSPACE_REF="$workspace"
  export GRM_SERVICE_WORKSPACE_MODE=create
  export GRM_SERVICE_WORKSPACE_FORMAT=binary
  export GRM_SERVICE_TLS_CA_CERT="$CERT_DIR/ca.pem"
  export GRM_SERVICE_TLS_DOMAIN_NAME=localhost
  if [ -n "$cert" ]; then
    export GRM_SERVICE_TLS_CLIENT_CERT="$cert"
    export GRM_SERVICE_TLS_CLIENT_KEY="$key"
  else
    unset GRM_SERVICE_TLS_CLIENT_CERT
    unset GRM_SERVICE_TLS_CLIENT_KEY
  fi
  grm-mcp --transport http --http-bind "127.0.0.1:$port" --http-path /mcp >"$output_file" 2>&1 &
  echo "$!"
}

expect_startup_failure() {
  name="$1"
  port="$2"
  cert="$3"
  key="$4"
  pid="$(start_temp_mcp "$name" "$port" "$cert" "$key" "$WORKSPACE_PREFIX-$name-$RUN_ID")"
  output_file="/tmp/grm-mcp-${name}.out"
  for _ in 1 2 3 4 5; do
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid" || true
      expect_no_secret_leak "$name" "$output_file"
      echo "ok: $name failed as expected"
      return 0
    fi
    sleep 1
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "expected startup failure for $name, but MCP HTTP server kept running" >&2
  print_checked_output "$name" "$output_file"
  exit 1
}

expect_startup_failure "no-client-cert" 18081 "" ""
expect_startup_failure "trusted-unmapped-client-cert" 18082 \
  "$CERT_DIR/unmapped-client.pem" "$CERT_DIR/unmapped-client-key.pem"

limited_pid="$(start_temp_mcp "mapped-principal-without-required-permission" 18083 \
  "$CERT_DIR/limited-client.pem" "$CERT_DIR/limited-client-key.pem" \
  "$WORKSPACE_PREFIX-limited-$RUN_ID")"
trap 'kill "$limited_pid" 2>/dev/null || true' EXIT INT TERM
expect_limited_permission_denied
kill "$limited_pid" 2>/dev/null || true
wait "$limited_pid" 2>/dev/null || true
trap - EXIT INT TERM

wait_for_mcp "$MCP_ENDPOINT" "mapped-principal-with-required-permission"

echo "CFSSL mTLS MCP HTTP smoke checks passed."
