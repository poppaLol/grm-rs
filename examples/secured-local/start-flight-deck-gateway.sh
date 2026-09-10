#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
REPO_ROOT="$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)"
ENV_FILE="${GRM_FLIGHT_DECK_GATEWAY_ENV:-$REPO_ROOT/.grm/secured-local/gateway.env}"

if [ ! -f "$ENV_FILE" ]; then
  echo "missing gateway env: $ENV_FILE" >&2
  echo "run examples/secured-local/bootstrap.sh first" >&2
  exit 1
fi

set -a
. "$ENV_FILE"
set +a

BIND="${GRM_FLIGHT_DECK_GATEWAY_BIND:-${GRM_FLIGHT_DECK_HTTP_BIND:-127.0.0.1:3001}}"
GATEWAY_URL="${GRM_FLIGHT_DECK_GATEWAY_URL:-http://$BIND}"
UPSTREAM="${GRM_SERVICE_ENDPOINT:-unknown}"
PRINCIPAL_HINT="${GRM_FLIGHT_DECK_GATEWAY_PRINCIPAL_HINT:-unknown}"
WORKSPACE_HINT="${GRM_FLIGHT_DECK_GATEWAY_WORKSPACE_HINT:-flight-deck-demo}"

echo "Starting GRM flight-deck local connector"
echo "Gateway URL: $GATEWAY_URL"
echo "Upstream endpoint: $UPSTREAM"
echo "Expected principal: $PRINCIPAL_HINT"
echo "Workspace hint: $WORKSPACE_HINT"
echo

cd "$REPO_ROOT"
exec cargo run -p grm-flight-deck-gateway
