#!/bin/sh
set -eu

ROOT="${ROOT:-.grm/secured-local}"
ENDPOINT="${GRM_SERVICE_ENDPOINT:-https://127.0.0.1:50051}"
WORKSPACE_PREFIX="${WORKSPACE_PREFIX:-secured-local-smoke}"
RUN_ID="$(date +%s)"
SCRIPT_FILE="$(mktemp "${TMPDIR:-/tmp}/grm-secured-local-smoke.XXXXXX.grm")"
trap 'rm -f "$SCRIPT_FILE"' EXIT

run_cli() {
  workspace="$1"
  mode="$2"
  cert="${3:-}"
  key="${4:-}"

  if [ -n "$cert" ]; then
    GRM_BACKEND=grpc \
    GRM_SERVICE_ENDPOINT="$ENDPOINT" \
    GRM_WORKSPACE_REF="$workspace" \
    GRM_SERVICE_WORKSPACE_MODE="$mode" \
    GRM_SERVICE_WORKSPACE_FORMAT=binary \
    GRM_SERVICE_TLS_CA_CERT="$ROOT/ca.crt" \
    GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
    GRM_SERVICE_TLS_CLIENT_CERT="$cert" \
    GRM_SERVICE_TLS_CLIENT_KEY="$key" \
    cargo run -q --bin grm -- session --script "$SCRIPT_FILE" < /dev/null
  else
    GRM_BACKEND=grpc \
    GRM_SERVICE_ENDPOINT="$ENDPOINT" \
    GRM_WORKSPACE_REF="$workspace" \
    GRM_SERVICE_WORKSPACE_MODE="$mode" \
    GRM_SERVICE_WORKSPACE_FORMAT=binary \
    GRM_SERVICE_TLS_CA_CERT="$ROOT/ca.crt" \
    GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
    cargo run -q --bin grm -- session --script "$SCRIPT_FILE" < /dev/null
  fi
}

write_script() {
  printf '%s\n' "$1" > "$SCRIPT_FILE"
}

expect_failure() {
  name="$1"
  shift
  output_file="$(mktemp "${TMPDIR:-/tmp}/grm-secured-local-${name}.XXXXXX.out")"
  if "$@" >"$output_file" 2>&1; then
    echo "expected failure for $name, but command succeeded" >&2
    cat "$output_file" >&2
    rm -f "$output_file"
    exit 1
  fi
  if grep -E "BEGIN (RSA |EC |)PRIVATE KEY|BEGIN CERTIFICATE|admin-1.key|unmapped-client.key|ca.key" "$output_file" >/dev/null; then
    echo "failure output leaked certificate or key material for $name" >&2
    cat "$output_file" >&2
    rm -f "$output_file"
    exit 1
  fi
  rm -f "$output_file"
  echo "ok: $name failed as expected"
}

expect_success() {
  name="$1"
  shift
  output_file="$(mktemp "${TMPDIR:-/tmp}/grm-secured-local-${name}.XXXXXX.out")"
  "$@" >"$output_file" 2>&1
  rm -f "$output_file"
  echo "ok: $name succeeded"
}

write_script 'session.exit'
for attempt in 1 2 3 4 5 6 7 8 9 10; do
  if run_cli "$WORKSPACE_PREFIX-ready-$RUN_ID-$attempt" create "$ROOT/admin-1.crt" "$ROOT/admin-1.key" >/tmp/grm-secured-local-ready.out 2>&1; then
    break
  fi
  if [ "$attempt" = "10" ]; then
    echo "secured GRM service did not become ready" >&2
    cat /tmp/grm-secured-local-ready.out >&2
    exit 1
  fi
  sleep 1
done

write_script 'session.describe
session.exit'
expect_failure "missing-client-certificate" run_cli "$WORKSPACE_PREFIX-no-cert-$RUN_ID" create

if [ ! -f "$ROOT/unmapped-client.crt" ] || [ ! -f "$ROOT/unmapped-client.key" ]; then
  FORCE=1 PREFIX=unmapped-client PRINCIPAL=local-admin/unmapped OUT_DIR="$ROOT" "$(dirname "$0")/generate-admin-client-cert.sh" >/dev/null
fi
expect_failure "unmapped-client-certificate" run_cli "$WORKSPACE_PREFIX-unmapped-$RUN_ID" create "$ROOT/unmapped-client.crt" "$ROOT/unmapped-client.key"

write_script 'model.define User userId name:string:required
let ada = node.create User name=Ada
session.describe
model.list
node.find User name=Ada
session.exit'
expect_success "admin-no-1-public-workspace-flow" run_cli "$WORKSPACE_PREFIX-admin-$RUN_ID" create "$ROOT/admin-1.crt" "$ROOT/admin-1.key"

echo "Secured-local service smoke checks passed."
