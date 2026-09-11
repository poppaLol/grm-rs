#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
usage: examples/secured-local/start-flight-deck-demo.sh [options]

Starts a local secured GRM service, verifies Admin no.1, seeds a demo workspace,
starts the flight-deck gateway, and starts the Vite flight-deck UI.

Options:
  --service-port <port>   secured gRPC service port (default: 50052)
  --gateway-port <port>   local flight-deck gateway port (default: 3001)
  --ui-port <port>        flight-deck UI port (default: 8081)
  --workspace <name>      workspace to seed and show (default: flight-deck-demo)
  --issuer <issuer>       Admin no.1 issuer (default: local-admin)
  --principal <subject>   Admin no.1 subject (default: admin-1)
  --force-bootstrap       regenerate secured-local bootstrap material
  --skip-verify           skip secured Admin no.1 smoke verification
  --skip-seed             skip demo workspace seeding
  --help, -h              show this help
EOF
}

SERVICE_PORT=50052
GATEWAY_PORT=3001
UI_PORT=8081
WORKSPACE=flight-deck-demo
ISSUER=local-admin
PRINCIPAL=admin-1
FORCE_BOOTSTRAP=0
SKIP_VERIFY=0
SKIP_SEED=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --service-port)
      shift
      SERVICE_PORT="${1:?missing service port}"
      ;;
    --gateway-port)
      shift
      GATEWAY_PORT="${1:?missing gateway port}"
      ;;
    --ui-port)
      shift
      UI_PORT="${1:?missing UI port}"
      ;;
    --workspace)
      shift
      WORKSPACE="${1:?missing workspace}"
      ;;
    --issuer)
      shift
      ISSUER="${1:?missing issuer}"
      ;;
    --principal)
      shift
      PRINCIPAL="${1:?missing principal}"
      ;;
    --force-bootstrap) FORCE_BOOTSTRAP=1 ;;
    --skip-verify) SKIP_VERIFY=1 ;;
    --skip-seed) SKIP_SEED=1 ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
OUT_DIR="$REPO_ROOT/.grm/secured-local"
LOG_DIR="$OUT_DIR/logs"
PYTHON="${PYTHON:-python3}"
if [ -x "$REPO_ROOT/.venv/bin/python" ]; then
  PYTHON="$REPO_ROOT/.venv/bin/python"
fi

SERVICE_ADDR="127.0.0.1:$SERVICE_PORT"
SERVICE_ENDPOINT="https://$SERVICE_ADDR"
GATEWAY_BIND="127.0.0.1:$GATEWAY_PORT"
GATEWAY_URL="http://$GATEWAY_BIND"
FLIGHT_DECK_URL="http://127.0.0.1:$UI_PORT"

validate_port() {
  name="$1"
  port="$2"
  case "$port" in
    ''|*[!0-9]*)
      echo "invalid $name: $port" >&2
      exit 2
      ;;
  esac
  if [ "$port" -lt 1 ] || [ "$port" -gt 65535 ]; then
    echo "invalid $name: $port" >&2
    exit 2
  fi
}

port_in_use() {
  "$PYTHON" - "$1" "$2" <<'PY'
import socket
import sys

host = sys.argv[1]
port = int(sys.argv[2])
with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
    sock.settimeout(0.2)
    sys.exit(0 if sock.connect_ex((host, port)) == 0 else 1)
PY
}

wait_for_port() {
  name="$1"
  host="$2"
  port="$3"
  timeout="$4"
  "$PYTHON" - "$name" "$host" "$port" "$timeout" <<'PY'
import socket
import sys
import time

name = sys.argv[1]
host = sys.argv[2]
port = int(sys.argv[3])
deadline = time.monotonic() + int(sys.argv[4])

while time.monotonic() < deadline:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.5)
        if sock.connect_ex((host, port)) == 0:
            sys.exit(0)
    time.sleep(1)

print(f"{name} did not become ready on {host}:{port}", file=sys.stderr)
sys.exit(1)
PY
}

ensure_free_port() {
  name="$1"
  port="$2"
  if port_in_use 127.0.0.1 "$port"; then
    echo "$name port is already in use: 127.0.0.1:$port" >&2
    exit 1
  fi
}

require_command() {
  name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "missing required command: $name" >&2
    exit 1
  fi
}

cleanup() {
  status=$?
  trap - INT TERM EXIT
  for pid in ${UI_PID:-} ${GATEWAY_PID:-} ${SERVICE_PID:-}; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  for pid in ${UI_PID:-} ${GATEWAY_PID:-} ${SERVICE_PID:-}; do
    if [ -n "$pid" ]; then
      wait "$pid" 2>/dev/null || true
    fi
  done
  exit "$status"
}

run_logged() {
  name="$1"
  log="$2"
  shift 2
  echo "Running $name; log: $log"
  if ! "$@" >"$log" 2>&1; then
    echo "$name failed; see $log" >&2
    exit 1
  fi
}

run_seed_logged() {
  log="$1"
  echo "Running demo workspace seed; log: $log"
  if seed_workspace >"$log" 2>&1; then
    return 0
  fi
  if grep "code: AlreadyExists" "$log" >/dev/null 2>&1 &&
    grep "workspace '$WORKSPACE' already exists; use open mode" "$log" >/dev/null 2>&1; then
    echo "Demo workspace already exists; continuing with existing workspace." >>"$log"
    echo "Demo workspace already exists; continuing with existing workspace."
    return 0
  fi
  echo "demo workspace seed failed; see $log" >&2
  exit 1
}

seed_workspace() {
  (
    cd "$REPO_ROOT"
    set -a
    . "$OUT_DIR/client.env"
    set +a
    cargo run -p grm-service-api --example local_workspace_client -- "$SERVICE_ENDPOINT" "$WORKSPACE"
  )
}

validate_port "--service-port" "$SERVICE_PORT"
validate_port "--gateway-port" "$GATEWAY_PORT"
validate_port "--ui-port" "$UI_PORT"

if [ "$SERVICE_PORT" = "$GATEWAY_PORT" ] || [ "$SERVICE_PORT" = "$UI_PORT" ] || [ "$GATEWAY_PORT" = "$UI_PORT" ]; then
  echo "service, gateway, and UI ports must be distinct" >&2
  exit 2
fi

require_command cargo
require_command npm

mkdir -p "$LOG_DIR"

ensure_free_port "secured service" "$SERVICE_PORT"
ensure_free_port "flight-deck gateway" "$GATEWAY_PORT"
ensure_free_port "flight-deck UI" "$UI_PORT"

BOOTSTRAP_LOG="$LOG_DIR/bootstrap.log"
SERVICE_LOG="$LOG_DIR/service.log"
VERIFY_LOG="$LOG_DIR/verify.log"
SEED_LOG="$LOG_DIR/seed.log"
GATEWAY_LOG="$LOG_DIR/gateway.log"
UI_LOG="$LOG_DIR/flight-deck-ui.log"

bootstrap_args=
if [ "$FORCE_BOOTSTRAP" = "1" ]; then
  bootstrap_args=--force
fi

run_logged "secured-local bootstrap" "$BOOTSTRAP_LOG" \
  "$SCRIPT_DIR/bootstrap.sh" \
    $bootstrap_args \
    --service-port "$SERVICE_PORT" \
    --gateway-port "$GATEWAY_PORT" \
    --workspace "$WORKSPACE" \
    --issuer "$ISSUER" \
    --principal "$PRINCIPAL"

trap cleanup INT TERM EXIT

(
  cd "$REPO_ROOT"
  set -a
  . "$OUT_DIR/service.env"
  set +a
  exec cargo run -p grm-service-api --bin grm-local-workspace-server -- "$SERVICE_ADDR" "$OUT_DIR/workspaces"
) >"$SERVICE_LOG" 2>&1 &
SERVICE_PID="$!"
echo "Starting secured service; log: $SERVICE_LOG"
wait_for_port "secured service" 127.0.0.1 "$SERVICE_PORT" 180

if [ "$SKIP_VERIFY" != "1" ]; then
  run_logged "Admin no.1 verification" "$VERIFY_LOG" \
    env ROOT="$OUT_DIR" GRM_SERVICE_ENDPOINT="$SERVICE_ENDPOINT" "$SCRIPT_DIR/scripts/verify-secured-service.sh"
fi

if [ "$SKIP_SEED" != "1" ]; then
  run_seed_logged "$SEED_LOG"
fi

"$SCRIPT_DIR/start-flight-deck-gateway.sh" >"$GATEWAY_LOG" 2>&1 &
GATEWAY_PID="$!"
echo "Starting flight-deck gateway; log: $GATEWAY_LOG"
wait_for_port "flight-deck gateway" 127.0.0.1 "$GATEWAY_PORT" 120

(
  cd "$REPO_ROOT/grm-flight-deck"
  GRM_FLIGHT_DECK_DEV_PROXY_TARGET="$GATEWAY_URL" \
  exec npm run dev -- --host 127.0.0.1 --port "$UI_PORT" --strictPort
) >"$UI_LOG" 2>&1 &
UI_PID="$!"
echo "Starting flight-deck UI; log: $UI_LOG"
wait_for_port "flight-deck UI" 127.0.0.1 "$UI_PORT" 120

cat <<EOF

GRM secured-local Flight Deck is ready.

Flight Deck: $FLIGHT_DECK_URL
Gateway URL: $GATEWAY_URL
Upstream service: $SERVICE_ENDPOINT
Workspace: $WORKSPACE
Identity: $ISSUER/$PRINCIPAL

In Flight Deck:
  Fixture: off
  Connection kind: Secured
  Gateway URL: blank, or $GATEWAY_URL
  Workspace: $WORKSPACE

Logs:
  Bootstrap: $BOOTSTRAP_LOG
  Service: $SERVICE_LOG
  Verify: $VERIFY_LOG
  Seed: $SEED_LOG
  Gateway: $GATEWAY_LOG
  Flight Deck UI: $UI_LOG

Press Ctrl-C to stop the service, gateway, and UI.
EOF

wait "$SERVICE_PID" "$GATEWAY_PID" "$UI_PID"
