# GRM gRPC Docker Quick Start

This quick start runs the local GRM gRPC workspace shell in Docker and talks to
it through workspace-scoped RPCs.

## Start The Service

```bash
docker compose up --build
```

The service listens on `localhost:50051` and stores local autocommit workspace
files in the `grm-workspaces` Docker volume.

## Rust Client Demo

The most reliable smoke test is the checked-in Rust client example:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 quickstart-demo
```

## grpcurl Setup

The server does not expose reflection yet, so every call must include the
protobuf schema. The most reproducible option is the published `grpcurl`
container, run on the Compose network with the local proto directory mounted
read-only:

```bash
docker pull fullstorydev/grpcurl:latest

grpcurl_docker() {
  docker run --rm \
    --network grm-rs_default \
    -v "$(pwd)/grm-service-api/proto:/protos:ro" \
    fullstorydev/grpcurl:latest \
    -plaintext \
    -import-path /protos \
    -proto grm/service/v1/service.proto \
    "$@"
}

GRPCURL=grpcurl_docker
GRPCURL_ENDPOINT=grm-grpc:50051
```

If you have a native `grpcurl` binary that can read files from the checkout, you
can use the host-published port instead:

```bash
grpcurl_native() {
  grpcurl -plaintext \
    -import-path "$(pwd)/grm-service-api/proto" \
    -proto grm/service/v1/service.proto \
    "$@"
}

GRPCURL=grpcurl_native
GRPCURL_ENDPOINT=localhost:50051
```

## Create A Workspace

```bash
$GRPCURL -d '{
  "mode": "WORKSPACE_CREATE_MODE_LOCAL_AUTOCOMMIT",
  "workspace": {"id": "quickstart-demo"},
  "format": "DURABILITY_FORMAT_JSON"
}' "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/CreateWorkspace
```

Capture a handle:

```bash
HANDLE=$($GRPCURL -d '{
  "snapshot": null,
  "workspace": {"id": "quickstart-demo"},
  "format": "DURABILITY_FORMAT_JSON"
}' "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/OpenWorkspace | jq -r '.handle.id')
```

## Define Schema Through ExecuteWorkspace

```bash
$GRPCURL -d "{
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
}" "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/ExecuteWorkspace
```

## Create And Find A Node

```bash
NODE_ID=$($GRPCURL -d "{
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
}" "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/ExecuteWorkspace | jq -r '.response.createNode.node.id')

$GRPCURL -d "{
  \"handle\": {\"id\": \"$HANDLE\"},
  \"request\": {
    \"findNodes\": {
      \"model\": \"User\",
      \"id\": $NODE_ID
    }
  }
}" "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/ExecuteWorkspace
```

## Close The Workspace

```bash
$GRPCURL -d "{
  \"handle\": {\"id\": \"$HANDLE\"}
}" "$GRPCURL_ENDPOINT" grm.service.v1.GrmService/CloseWorkspace
```

## Stop And Clean Up

```bash
docker compose down
docker compose down -v
```

## Troubleshooting

Check logs:

```bash
docker compose logs grm-grpc
```

Rebuild from scratch:

```bash
docker compose build --no-cache
docker compose up
```

If `grpcurl` reports unknown services or fields, make sure you are passing the
proto import arguments and using the fully qualified method name, for example:

```text
grm.service.v1.GrmService/ExecuteWorkspace
```

If `grpcurl` reports `permission denied` while opening `service.proto`, prefer
the Dockerized `fullstorydev/grpcurl` command above. Some Snap-packaged native
installs are blocked from reading proto files from a development checkout.
