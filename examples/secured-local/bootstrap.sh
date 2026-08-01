#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
usage: examples/secured-local/bootstrap.sh [--force] [--start] [--verify] [--access-level <id>] [--issuer <issuer>] [--principal <subject>]

Creates .grm/secured-local local CA, server certificate, Admin no.1 client
certificate, security.json, env files, Compose profile metadata, and optional
public service smoke checks. Existing files are reused unless --force is set.
EOF
}

FORCE=0
START=0
VERIFY=0
ACCESS_LEVEL="owner-bootstrap-admin"
PRINCIPAL="admin-1"
ISSUER="local-admin"
ENDPOINT="https://127.0.0.1:50051"
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

write_file "$OUT_DIR/service.env" "GRM_SERVICE_SECURITY_PROFILE=secured
GRM_SERVICE_SECURITY_CONFIG=$OUT_DIR/security.json
GRM_SERVICE_TLS_SERVER_CERT=$OUT_DIR/server.crt
GRM_SERVICE_TLS_SERVER_KEY=$OUT_DIR/server.key
GRM_SERVICE_TLS_CLIENT_CA_CERT=$OUT_DIR/ca.crt"

write_file "$OUT_DIR/client.env" "GRM_BACKEND=grpc
GRM_SERVICE_ENDPOINT=$ENDPOINT
GRM_SERVICE_TLS_CA_CERT=$OUT_DIR/ca.crt
GRM_SERVICE_TLS_DOMAIN_NAME=localhost
GRM_SERVICE_TLS_CLIENT_CERT=$OUT_DIR/admin-1.crt
GRM_SERVICE_TLS_CLIENT_KEY=$OUT_DIR/admin-1.key
GRM_SERVICE_WORKSPACE_FORMAT=binary"

if [ -e "$OUT_DIR/flight-deck-profile.json" ] && [ "$FORCE" != "1" ]; then
  echo "Reusing existing $OUT_DIR/flight-deck-profile.json"
else
  "$PYTHON" "$SCRIPT_DIR/scripts/write-flight-deck-profile.py" \
    --output "$OUT_DIR/flight-deck-profile.json" \
    --endpoint "$ENDPOINT" \
    --principal "$PRINCIPAL" \
    --access-level "$ACCESS_LEVEL" \
    --security-config "$OUT_DIR/security.json" \
    --ca-cert "$OUT_DIR/ca.crt" \
    $render_force
fi

echo
echo "Secured-local bootstrap material is ready under $OUT_DIR"
echo "Admin no.1 fingerprint: $FINGERPRINT"
echo "Canonical principal: $ISSUER/$PRINCIPAL"
echo "Security config: $OUT_DIR/security.json"
echo
echo "Start service locally:"
echo "  set -a; . $OUT_DIR/service.env; set +a"
echo "  cargo run -p grm-service-api --bin grm-local-workspace-server -- 127.0.0.1:50051 $OUT_DIR/workspaces"
echo
echo "Use Admin no.1 client env:"
echo "  set -a; . $OUT_DIR/client.env; set +a"
echo
echo "Docker Compose profile:"
echo "  docker compose -f examples/secured-local/docker-compose.secured-local.yml up --build"

if [ "$START" = "1" ]; then
  docker compose -f "$SCRIPT_DIR/docker-compose.secured-local.yml" up --build -d
fi

if [ "$VERIFY" = "1" ]; then
  ROOT="$OUT_DIR" GRM_SERVICE_ENDPOINT="$ENDPOINT" "$SCRIPT_DIR/scripts/verify-secured-service.sh"
fi
