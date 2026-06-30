#!/bin/sh
set -eu

CERT_DIR="${CERT_DIR:-/certs}"
ENDPOINT="${GRM_SERVICE_ENDPOINT:-https://grm-secured:50051}"
WORKSPACE_PREFIX="${WORKSPACE_PREFIX:-cfssl-demo}"
RUN_ID="$(date +%s)"

base_env() {
  export GRM_BACKEND=grpc
  export GRM_SERVICE_ENDPOINT="$ENDPOINT"
  export GRM_SERVICE_TLS_CA_CERT="$CERT_DIR/ca.pem"
  export GRM_SERVICE_TLS_DOMAIN_NAME=localhost
  export GRM_SERVICE_WORKSPACE_FORMAT=binary
}

run_cli() {
  workspace="$1"
  mode="$2"
  cert="$3"
  key="$4"
  script="$5"
  script_file="/tmp/grm-script-$$.grm"
  base_env
  export GRM_WORKSPACE_REF="$workspace"
  export GRM_SERVICE_WORKSPACE_MODE="$mode"
  if [ -n "$cert" ]; then
    export GRM_SERVICE_TLS_CLIENT_CERT="$cert"
    export GRM_SERVICE_TLS_CLIENT_KEY="$key"
  else
    unset GRM_SERVICE_TLS_CLIENT_CERT
    unset GRM_SERVICE_TLS_CLIENT_KEY
  fi
  printf '%s\n' "$script" > "$script_file"
  grm session --script "$script_file"
}

expect_failure() {
  name="$1"
  shift
  output_file="/tmp/grm-${name}.out"
  if "$@" >"$output_file" 2>&1; then
    echo "expected failure for $name, but command succeeded" >&2
    cat "$output_file" >&2
    exit 1
  fi
  if grep -E "BEGIN (RSA |EC |)PRIVATE KEY|BEGIN CERTIFICATE|mapped-client-key|limited-client-key|unmapped-client-key" "$output_file" >/dev/null; then
    echo "failure output leaked certificate or key material for $name" >&2
    cat "$output_file" >&2
    exit 1
  fi
  echo "ok: $name failed as expected"
}

expect_success() {
  name="$1"
  shift
  output_file="/tmp/grm-${name}.out"
  "$@" >"$output_file" 2>&1
  echo "ok: $name succeeded"
}

limited_script='session.describe
session.exit'

full_script='model.define User userId name:string:required
model.define Post postId title:string:required
link.define AUTHORED User Post authoredId year:int:required
let alice = node.create User name=Ada
let post = node.create Post title="Graph Notes"
edge.create AUTHORED from=alice to=post year=2026
session.describe
model.list
node.find User name=Ada
edge.find AUTHORED from=alice
session.exit'

ready_script='session.exit'
for attempt in 1 2 3 4 5 6 7 8 9 10; do
  if run_cli "$WORKSPACE_PREFIX-ready-$RUN_ID-$attempt" create \
    "$CERT_DIR/mapped-client.pem" "$CERT_DIR/mapped-client-key.pem" "$ready_script" \
    >/tmp/grm-ready.out 2>&1; then
    break
  fi
  if [ "$attempt" = "10" ]; then
    echo "secured GRM service did not become ready" >&2
    cat /tmp/grm-ready.out >&2
    exit 1
  fi
  sleep 1
done

expect_failure "no-client-cert" \
  run_cli "$WORKSPACE_PREFIX-no-cert-$RUN_ID" create "" "" "$limited_script"

expect_failure "trusted-unmapped-client-cert" \
  run_cli "$WORKSPACE_PREFIX-unmapped-$RUN_ID" create \
    "$CERT_DIR/unmapped-client.pem" "$CERT_DIR/unmapped-client-key.pem" "$limited_script"

expect_failure "mapped-principal-without-required-permission" \
  run_cli "$WORKSPACE_PREFIX-limited-$RUN_ID" create \
    "$CERT_DIR/limited-client.pem" "$CERT_DIR/limited-client-key.pem" "$limited_script"

expect_success "mapped-principal-with-required-permission" \
  run_cli "$WORKSPACE_PREFIX-full-$RUN_ID" create \
    "$CERT_DIR/mapped-client.pem" "$CERT_DIR/mapped-client-key.pem" "$full_script"

echo "CFSSL mTLS demo smoke checks passed."
