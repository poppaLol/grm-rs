#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
usage: examples/secured-local/bootstrap.sh [--force] [--start] [--verify] [--access-level <id>] [--issuer <issuer>] [--principal <subject>] [--service-port <port>] [--gateway-port <port>] [--workspace <name>]

Creates .grm/secured-local local CA, server certificate, Admin no.1 client
certificate, security.json, env files, gateway connector env, safe
flight-deck profile metadata, and optional public service smoke checks.
Existing files are reused unless --force is set.
EOF
}

FORCE=0
START=0
VERIFY=0
ACCESS_LEVEL="owner-bootstrap-admin"
PRINCIPAL="admin-1"
ISSUER="local-admin"
SERVICE_PORT="50051"
GATEWAY_PORT="3001"
WORKSPACE="flight-deck-demo"
POLICY_VERSION="secured-local-policy-v1"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --force) FORCE=1 ;;
    --start) START=1 ;;
    --verify) VERIFY=1 ;;
    --access-level)
      shift
      ACCESS_LEVEL="${1:?missing access level}"
      ;;
    --principal)
      shift
      PRINCIPAL="${1:?missing principal subject}"
      ;;
    --issuer)
      shift
      ISSUER="${1:?missing principal issuer}"
      ;;
    --service-port)
      shift
      SERVICE_PORT="${1:?missing service port}"
      ;;
    --gateway-port)
      shift
      GATEWAY_PORT="${1:?missing gateway port}"
      ;;
    --workspace)
      shift
      WORKSPACE="${1:?missing workspace}"
      ;;
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

case "$SERVICE_PORT" in
  ''|*[!0-9]*)
    echo "invalid --service-port: $SERVICE_PORT" >&2
    exit 2
    ;;
esac
case "$GATEWAY_PORT" in
  ''|*[!0-9]*)
    echo "invalid --gateway-port: $GATEWAY_PORT" >&2
    exit 2
    ;;
esac
if [ "$SERVICE_PORT" -lt 1 ] || [ "$SERVICE_PORT" -gt 65535 ]; then
  echo "invalid --service-port: $SERVICE_PORT" >&2
  exit 2
fi
if [ "$GATEWAY_PORT" -lt 1 ] || [ "$GATEWAY_PORT" -gt 65535 ]; then
  echo "invalid --gateway-port: $GATEWAY_PORT" >&2
  exit 2
fi

ENDPOINT="https://127.0.0.1:$SERVICE_PORT"
GATEWAY_BIND="127.0.0.1:$GATEWAY_PORT"
GATEWAY_URL="http://$GATEWAY_BIND"

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
OUT_DIR="$REPO_ROOT/.grm/secured-local"
TEMPLATE_DIR="$SCRIPT_DIR/templates"
PYTHON="${PYTHON:-python3}"
if [ -x "$REPO_ROOT/.venv/bin/python" ]; then
  PYTHON="$REPO_ROOT/.venv/bin/python"
fi
mkdir -p "$OUT_DIR"

write_file() {
  path="$1"
  content="$2"
  if [ -e "$path" ] && [ "$FORCE" != "1" ]; then
    echo "Reusing existing $path"
    return 0
  fi
  printf '%s\n' "$content" > "$path"
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

preflight_output="$(mktemp "${TMPDIR:-/tmp}/grm-secured-local-preflight.XXXXXX.json")"
if ! "$PYTHON" "$SCRIPT_DIR/scripts/render-security-config.py" \
  --access-levels "$TEMPLATE_DIR/access-levels.json" \
  --template "$TEMPLATE_DIR/security-config.template.json" \
  --output "$preflight_output" \
  --fingerprint "0000000000000000000000000000000000000000000000000000000000000000" \
  --issuer "$ISSUER" \
  --principal "$PRINCIPAL" \
  --access-level "$ACCESS_LEVEL" \
  --policy-version "secured-local-policy-v1" \
  --require-bootstrap-admin \
  --force >/dev/null; then
  rm -f "$preflight_output"
  exit 1
fi
rm -f "$preflight_output"

render_force=
if [ "$FORCE" = "1" ]; then
  render_force=--force
fi

if [ "$FORCE" != "1" ] && [ ! -e "$OUT_DIR/bootstrap-inputs.json" ]; then
  if [ -e "$OUT_DIR/security.json" ] || [ -e "$OUT_DIR/flight-deck-profile.json" ] || [ -e "$OUT_DIR/admin-1.crt" ]; then
    echo "existing secured-local bootstrap material has no bootstrap-inputs.json; rerun with --force to verify and replace generated local material" >&2
    exit 1
  fi
fi

"$PYTHON" "$SCRIPT_DIR/scripts/write-bootstrap-inputs.py" \
  --output "$OUT_DIR/bootstrap-inputs.json" \
  --issuer "$ISSUER" \
  --principal "$PRINCIPAL" \
  --access-level "$ACCESS_LEVEL" \
  --policy-version "$POLICY_VERSION" \
  --service-port "$SERVICE_PORT" \
  --gateway-port "$GATEWAY_PORT" \
  --workspace "$WORKSPACE" \
  $render_force

FORCE="$FORCE" OUT_DIR="$OUT_DIR" "$SCRIPT_DIR/scripts/generate-ca.sh"
FORCE="$FORCE" OUT_DIR="$OUT_DIR" "$SCRIPT_DIR/scripts/generate-server-cert.sh"
FORCE="$FORCE" OUT_DIR="$OUT_DIR" PRINCIPAL="$PRINCIPAL" "$SCRIPT_DIR/scripts/generate-admin-client-cert.sh"

FINGERPRINT="$("$SCRIPT_DIR/scripts/fingerprint-cert.sh" "$OUT_DIR/admin-1.crt")"
printf '%s\n' "$FINGERPRINT" > "$OUT_DIR/admin-1.sha256"

if [ -e "$OUT_DIR/security.json" ] && [ "$FORCE" != "1" ]; then
  echo "Reusing existing $OUT_DIR/security.json"
else
  "$PYTHON" "$SCRIPT_DIR/scripts/render-security-config.py" \
    --access-levels "$TEMPLATE_DIR/access-levels.json" \
    --template "$TEMPLATE_DIR/security-config.template.json" \
    --output "$OUT_DIR/security.json" \
    --fingerprint "$FINGERPRINT" \
    --issuer "$ISSUER" \
    --principal "$PRINCIPAL" \
    --access-level "$ACCESS_LEVEL" \
    --policy-version "$POLICY_VERSION" \
    --require-bootstrap-admin \
    $render_force
fi

write_file "$OUT_DIR/service.env" "GRM_SERVICE_SECURITY_PROFILE=$(shell_quote secured)
GRM_SERVICE_SECURITY_CONFIG=$(shell_quote "$OUT_DIR/security.json")
GRM_SERVICE_TLS_SERVER_CERT=$(shell_quote "$OUT_DIR/server.crt")
GRM_SERVICE_TLS_SERVER_KEY=$(shell_quote "$OUT_DIR/server.key")
GRM_SERVICE_TLS_CLIENT_CA_CERT=$(shell_quote "$OUT_DIR/ca.crt")"

write_file "$OUT_DIR/client.env" "GRM_BACKEND=$(shell_quote grpc)
GRM_SERVICE_ENDPOINT=$(shell_quote "$ENDPOINT")
GRM_SERVICE_TLS_CA_CERT=$(shell_quote "$OUT_DIR/ca.crt")
GRM_SERVICE_TLS_DOMAIN_NAME=$(shell_quote localhost)
GRM_SERVICE_TLS_CLIENT_CERT=$(shell_quote "$OUT_DIR/admin-1.crt")
GRM_SERVICE_TLS_CLIENT_KEY=$(shell_quote "$OUT_DIR/admin-1.key")
GRM_SERVICE_WORKSPACE_FORMAT=$(shell_quote binary)
GRM_WORKSPACE_REF=$(shell_quote "$WORKSPACE")"

write_file "$OUT_DIR/gateway.env" "GRM_SERVICE_ENDPOINT=$(shell_quote "$ENDPOINT")
GRM_SERVICE_TLS_CA_CERT=$(shell_quote "$OUT_DIR/ca.crt")
GRM_SERVICE_TLS_DOMAIN_NAME=$(shell_quote localhost)
GRM_SERVICE_TLS_CLIENT_CERT=$(shell_quote "$OUT_DIR/admin-1.crt")
GRM_SERVICE_TLS_CLIENT_KEY=$(shell_quote "$OUT_DIR/admin-1.key")
GRM_FLIGHT_DECK_GATEWAY_BIND=$(shell_quote "$GATEWAY_BIND")
GRM_FLIGHT_DECK_GATEWAY_URL=$(shell_quote "$GATEWAY_URL")
GRM_FLIGHT_DECK_GATEWAY_WORKSPACE_HINT=$(shell_quote "$WORKSPACE")
GRM_FLIGHT_DECK_GATEWAY_PRINCIPAL_HINT=$(shell_quote "$ISSUER/$PRINCIPAL")"

if [ -e "$OUT_DIR/flight-deck-profile.json" ] && [ "$FORCE" != "1" ]; then
  echo "Reusing existing $OUT_DIR/flight-deck-profile.json"
else
  "$PYTHON" "$SCRIPT_DIR/scripts/write-flight-deck-profile.py" \
    --output "$OUT_DIR/flight-deck-profile.json" \
    --gateway-url "$GATEWAY_URL" \
    --upstream-endpoint "$ENDPOINT" \
    --workspace "$WORKSPACE" \
    --issuer "$ISSUER" \
    --principal "$PRINCIPAL" \
    --access-level "$ACCESS_LEVEL" \
    $render_force
fi

echo
echo "Secured-local bootstrap material is ready under $OUT_DIR"
echo "Admin no.1 fingerprint: $FINGERPRINT"
echo "Canonical principal: $ISSUER/$PRINCIPAL"
echo "Security config: $OUT_DIR/security.json"
echo "Gateway URL: $GATEWAY_URL"
echo "Upstream endpoint: $ENDPOINT"
echo "Workspace: $WORKSPACE"
echo
echo "Start service locally:"
echo "  set -a; . $OUT_DIR/service.env; set +a"
echo "  cargo run -p grm-service-api --bin grm-local-workspace-server -- 127.0.0.1:$SERVICE_PORT $OUT_DIR/workspaces"
echo
echo "Start flight-deck gateway:"
echo "  examples/secured-local/start-flight-deck-gateway.sh"
echo
echo "Docker Compose profile:"
if [ "$SERVICE_PORT" = "50051" ]; then
  echo "  docker compose -f examples/secured-local/docker-compose.secured-local.yml up --build"
else
  echo "  GRM_SECURED_LOCAL_SERVICE_PORT=$SERVICE_PORT docker compose -f examples/secured-local/docker-compose.secured-local.yml up --build"
fi

if [ "$START" = "1" ]; then
  GRM_SECURED_LOCAL_SERVICE_PORT="$SERVICE_PORT" docker compose -f "$SCRIPT_DIR/docker-compose.secured-local.yml" up --build -d
fi

if [ "$VERIFY" = "1" ]; then
  ROOT="$OUT_DIR" GRM_SERVICE_ENDPOINT="$ENDPOINT" "$SCRIPT_DIR/scripts/verify-secured-service.sh"
fi
