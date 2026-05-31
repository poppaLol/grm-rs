#!/usr/bin/env bash

set -euo pipefail

WORKSPACE_ID="${WORKSPACE_ID:-grpc-demo-$(date +%s)}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd)"
PROTO_ROOT="${PROTO_ROOT:-$REPO_ROOT/grm-service-api/proto}"
PROTO_FILE="${PROTO_FILE:-grm/service/v1/service.proto}"
METHOD_PREFIX="grm.service.v1.GrmService"
GRPCURL_IMAGE="${GRPCURL_IMAGE:-fullstorydev/grpcurl:latest}"
GRPCURL_DOCKER_NETWORK="${GRPCURL_DOCKER_NETWORK:-grm-rs_default}"
GRPCURL_MODE="${GRPCURL_MODE:-auto}"

if [[ "$GRPCURL_MODE" == "auto" ]]; then
  if command -v grpcurl >/dev/null 2>&1 && grpcurl --version >/dev/null 2>&1; then
    GRPCURL_MODE="native"
  else
    GRPCURL_MODE="docker"
  fi
fi

if [[ "$GRPCURL_MODE" == "docker" ]]; then
  GRPC_ENDPOINT="${GRPC_ENDPOINT:-grm-grpc:50051}"
elif [[ "$GRPCURL_MODE" == "native" ]]; then
  GRPC_ENDPOINT="${GRPC_ENDPOINT:-localhost:50051}"
else
  echo "GRPCURL_MODE must be auto, native, or docker." >&2
  exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for this demo." >&2
  exit 1
fi

if [[ "$GRPCURL_MODE" == "docker" ]] && ! command -v docker >/dev/null 2>&1; then
  echo "docker is required for GRPCURL_MODE=docker." >&2
  exit 1
fi

grpcurl_call() {
  if [[ "$GRPCURL_MODE" == "docker" ]]; then
    docker run --rm \
      --network "$GRPCURL_DOCKER_NETWORK" \
      -v "$PROTO_ROOT:/protos:ro" \
      "$GRPCURL_IMAGE" \
      -plaintext \
      -import-path /protos \
      -proto "$PROTO_FILE" \
      "$@"
  else
    grpcurl -plaintext \
      -import-path "$PROTO_ROOT" \
      -proto "$PROTO_FILE" \
      "$@"
  fi
}

echo "GRM gRPC workspace demo"
echo "endpoint:  $GRPC_ENDPOINT"
echo "workspace: $WORKSPACE_ID"
echo "grpcurl:   $GRPCURL_MODE"

echo
echo "1. Create local autocommit workspace"
CREATE_RESPONSE=$(grpcurl_call -d "{
  \"mode\": \"WORKSPACE_CREATE_MODE_LOCAL_AUTOCOMMIT\",
  \"workspace\": {\"id\": \"$WORKSPACE_ID\"},
  \"format\": \"DURABILITY_FORMAT_JSON\"
}" "$GRPC_ENDPOINT" "$METHOD_PREFIX/CreateWorkspace")
echo "$CREATE_RESPONSE" | jq .
HANDLE=$(echo "$CREATE_RESPONSE" | jq -r '.handle.id')

echo
echo "2. Define User model through ExecuteWorkspace"
grpcurl_call -d "{
  \"handle\": {\"id\": \"$HANDLE\"},
  \"request\": {
    \"defineNode\": {
      \"name\": \"User\",
      \"idField\": \"userId\",
      \"fields\": [
        {
          \"name\": \"name\",
          \"valueType\": \"FIELD_VALUE_TYPE_STRING\",
          \"required\": true
        }
      ]
    }
  }
}" "$GRPC_ENDPOINT" "$METHOD_PREFIX/ExecuteWorkspace" | jq .

echo
echo "3. Create a user"
CREATE_NODE_RESPONSE=$(grpcurl_call -d "{
  \"handle\": {\"id\": \"$HANDLE\"},
  \"request\": {
    \"createNode\": {
      \"model\": \"User\",
      \"props\": {
        \"properties\": [
          {\"name\": \"name\", \"value\": {\"stringValue\": \"Ada\"}}
        ]
      }
    }
  }
}" "$GRPC_ENDPOINT" "$METHOD_PREFIX/ExecuteWorkspace")
echo "$CREATE_NODE_RESPONSE" | jq .
NODE_ID=$(echo "$CREATE_NODE_RESPONSE" | jq -r '.response.createNode.node.id')

echo
echo "4. Find the user"
grpcurl_call -d "{
  \"handle\": {\"id\": \"$HANDLE\"},
  \"request\": {
    \"findNodes\": {
      \"model\": \"User\",
      \"id\": $NODE_ID
    }
  }
}" "$GRPC_ENDPOINT" "$METHOD_PREFIX/ExecuteWorkspace" | jq .

echo
echo "5. Close workspace"
grpcurl_call -d "{
  \"handle\": {\"id\": \"$HANDLE\"}
}" "$GRPC_ENDPOINT" "$METHOD_PREFIX/CloseWorkspace" | jq .

echo
echo "Done. Reopen workspace '$WORKSPACE_ID' later with OpenWorkspace."
