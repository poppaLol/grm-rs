# Rust Service Workspace Tutorial

This tutorial uses Rust as a programming surface over GRM's service-backed
workspace storage mode. `GrpcWorkspaceClient` wraps the generated protobuf
client with typed GRM runtime requests and responses.

You will:

- connect to a local service workspace
- define schema through typed requests
- create and find graph data
- close and reopen persisted workspace state

## Start The Service

Pull and run the published service image:

```bash
docker pull lauriebart/grm:latest
docker run --rm --name grm \
  -p 50051:50051 \
  -v grm-workspaces:/workspaces \
  lauriebart/grm:latest
```

This is an insecure local demonstration listening on `127.0.0.1:50051`.
Workspace files persist in the named `grm-workspaces` volume after the
container stops.

Contributors can instead build the current checkout with
`docker compose up --build`. See the
[gRPC Docker quick start](../grpc-quickstart.md) for TLS and mutual TLS setup.

## Run The Checked Client

The repository includes a complete Rust client that covers schema, CRUD,
traversal, batch operations, close, and reopen:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 tutorial-rust
```

The example uses generated protobuf requests where it needs to demonstrate the
wire contract directly. Application code can normally use
`GrpcWorkspaceClient` for a smaller typed API.

## Connect To A Workspace

Add the workspace crates and Tokio to a Rust crate in this workspace:

```toml
[dependencies]
grm-rs = { path = "..", default-features = false }
grm-service-api = { path = "../grm-service-api" }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Create a new service-managed workspace:

```rust
use grm_service_api::{GrpcWorkspaceClient, GrpcWorkspaceMode};

let mut client = GrpcWorkspaceClient::connect(
    "http://127.0.0.1:50051",
    "tutorial-rust",
    GrpcWorkspaceMode::Create,
)
.await?;
```

`Create` rejects an existing workspace ref. Use `GrpcWorkspaceMode::Open` when
the workspace already exists. Binary persistence is the default.

## Define Schema

Define node and link models with the same runtime request types used by the
embedded runtime:

```rust
use grm_rs::{
    DefineEdgeRequest, DefineNodeRequest, FieldSpec, FieldValueType,
};

client
    .define_node(DefineNodeRequest {
        name: "Person".into(),
        id_field: "personId".into(),
        fields: vec![FieldSpec {
            name: "name".into(),
            value_type: FieldValueType::String,
            required: true,
        }],
    })
    .await?;

client
    .define_node(DefineNodeRequest {
        name: "Movie".into(),
        id_field: "movieId".into(),
        fields: vec![FieldSpec {
            name: "title".into(),
            value_type: FieldValueType::String,
            required: true,
        }],
    })
    .await?;

client
    .define_edge(DefineEdgeRequest {
        name: "ACTEDIN".into(),
        from_model: "Person".into(),
        to_model: "Movie".into(),
        id_field: "actedInId".into(),
        fields: vec![FieldSpec {
            name: "role".into(),
            value_type: FieldValueType::String,
            required: true,
        }],
    })
    .await?;
```

## Create And Find Data

```rust
use grm_rs::{EdgeCreateRequest, NodeCreateRequest, NodeFindRequest};
use serde_json::json;

let person = client
    .create_node(NodeCreateRequest {
        model: "Person".into(),
        props: [("name".into(), json!("Keanu Reeves"))]
            .into_iter()
            .collect(),
    })
    .await?;

let movie = client
    .create_node(NodeCreateRequest {
        model: "Movie".into(),
        props: [("title".into(), json!("The Matrix"))]
            .into_iter()
            .collect(),
    })
    .await?;

client
    .create_edge(EdgeCreateRequest {
        model: "ACTEDIN".into(),
        from: person.id,
        to: movie.id,
        props: [("role".into(), json!("Neo"))].into_iter().collect(),
    })
    .await?;

let people = client
    .find_nodes(NodeFindRequest {
        model: "Person".into(),
        id: Some(person.id),
        ..Default::default()
    })
    .await?;

assert_eq!(people.nodes[0].props["name"], json!("Keanu Reeves"));
```

The ergonomic client also exposes typed update/delete, traversal-capable find,
schema listing, batch, explain, and profile operations. They all route through
the workspace-scoped `ExecuteWorkspace` service boundary.

## Close And Reopen

Release the current workspace handle on orderly shutdown:

```rust
client.close().await?;
```

Reconnect to the persisted workspace:

```rust
let mut client = GrpcWorkspaceClient::connect(
    "http://127.0.0.1:50051",
    "tutorial-rust",
    GrpcWorkspaceMode::Open,
)
.await?;

let schema = client.schema_list().await?;
assert_eq!(schema.node_models.len(), 2);
```

The current service target is tested single-writer local persistence. This
tutorial does not imply hosted durability, multi-writer coordination,
authorization/RBAC, or production certificate lifecycle.

## Where To Go Next

- Use the [gRPC Docker quick start](../grpc-quickstart.md) for TLS, mutual TLS,
  CLI, Python, and MCP service configuration.
- Read the checked
  [`local_workspace_client.rs`](../../grm-service-api/examples/local_workspace_client.rs)
  example for the complete generated-protobuf workflow.
- Use the [CLI session tutorial](cli-session.md) for an interactive workflow
  over the same workspace backend.
