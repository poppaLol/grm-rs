# CFSSL mTLS Local Demo

Status: local controlled demo

This demo provisions local certificate material with CFSSL, maps exact client
certificate fingerprints to canonical GRM principals, starts the local gRPC
workspace service in the secured profile, and runs CLI commands through
`ExecuteWorkspace`. It also includes an optional `grm-mcp` Streamable HTTP
adapter service that connects to the same secured gRPC workspace service with
the mapped local demo client certificate.

It proves only this local onboarding path:

- TLS server authentication and mTLS client-certificate transport trust are
  configured for the local service.
- GRM computes the SHA-256 fingerprint of the validated client certificate DER.
- The service maps that exact fingerprint to a canonical principal.
- The permission table independently authorizes typed workspace actions.
- The CLI sidecar parses human commands locally and sends typed workspace
  requests through the secured gRPC service.
- The MCP HTTP sidecar serves MCP Streamable HTTP only as a local controlled
  adapter and sends supported MCP tool operations through the secured gRPC
  service.

It does not claim production PKI, certificate lifecycle management, hosted
tenancy, Cloudflare-edge identity, hosted or public HTTP MCP security, browser
auth, OAuth, bearer-token auth, bounded authoritative audit, encryption at rest,
attestation, receipts, state commitments, admin RPCs, policy hot reload, or a
general policy language.

## Files And Artifacts

Run the demo from the repository root with:

```bash
examples/cfssl-mtls/scripts/run-compose-smoke.sh
```

The wrapper starts the secured service and MCP HTTP service detached, runs the
CLI and MCP HTTP smoke sidecars as one-shot containers, returns the first
failing sidecar exit code, and stops the Compose stack without deleting
generated volumes.

The Compose stack creates five named volumes:

- `grm-cfssl-certs`: local demo CA, server certificate, client certificates,
  client private keys, and computed client fingerprint files.
- `grm-cfssl-config`: generated `security.json` consumed at service startup.
- `grm-cfssl-workspaces`: local autocommit GRM workspace files.
- `grm-cfssl-mcp-certs`: narrowed certificate material for the long-running MCP
  HTTP adapter: `ca.pem`, `mapped-client.pem`, and `mapped-client-key.pem`.
- `grm-cfssl-mcp-smoke-certs`: narrowed certificate material for MCP HTTP smoke
  checks: `ca.pem` plus the mapped, limited, and unmapped client certificate
  pairs needed to prove allow and deny paths.

The CFSSL init step writes:

- `ca.pem`, `ca-key.pem`: local demo CA certificate and private key.
- `server.pem`, `server-key.pem`: server certificate/key for `localhost`,
  `grm-secured`, and `127.0.0.1`.
- `mapped-client.pem`, `mapped-client-key.pem`: trusted client certificate/key
  mapped to `local-demo` / `cli/full`.
- `limited-client.pem`, `limited-client-key.pem`: trusted client
  certificate/key mapped to `local-demo` / `cli/limited`.
- `unmapped-client.pem`, `unmapped-client-key.pem`: trusted client
  certificate/key deliberately absent from GRM principal mappings.
- `mapped-client.sha256`, `limited-client.sha256`: fingerprints computed by
  `grm-cert-fingerprint` from the certificate DER.

The generated `security.json` has:

- `certificate_mappings`: exact SHA-256 fingerprints to canonical principals.
- `permission_table.version`: `local-demo-policy-v1`.
- `permission_table.assignments`: explicit permissions for `cli/full` and only
  `workspace.create` for `cli/limited`.

The full principal receives:

- service-scoped `workspace.create`;
- deployment-local workspace permissions for `workspace.open`,
  `workspace.close`, `schema.define`, `schema.inspect`, `node.create`,
  `node.read`, `edge.create`, `edge.read`, `explain`, and `profile`.

The limited principal can create a workspace but cannot inspect schema, so a
CLI `session.describe` fails after authentication.

The Compose file builds narrower Docker runtime targets from one builder stage:

- `grm-rs-service:local`: `grm-local-workspace-server` and
  `grm-cert-fingerprint`.
- `grm-rs-cli:local`: `grm` for CLI smoke sidecars.
- `grm-rs-mcp:local`: `grm-mcp` for the long-running local HTTP MCP adapter.
- `grm-rs-mcp-smoke:local`: `grm-mcp-http-smoke` plus `grm-mcp` for local
  negative-case smoke servers.

## Trust Boundaries

The generated CA and keys are local demo material inside Docker volumes. Any
container or operator with access to those volumes can read them. Do not reuse
them outside this demo.

The long-running MCP HTTP adapter does not mount the full CFSSL output volume.
It receives only the local CA certificate and the mapped demo client
certificate/key it uses to call the secured gRPC service. The MCP smoke sidecar
uses a separate narrowed volume with the extra limited and unmapped client
certificate pairs required for negative tests. Neither MCP volume contains the
local CA private key, server private key, generated security config, or
fingerprint files.

TLS transport trust and GRM application identity remain separate. A certificate
signed by the local CA only passes the mTLS transport boundary. It becomes a GRM
principal only when its exact fingerprint appears in `security.json`.

Authentication establishes identity only. Permissions come from the versioned
permission table, using server-derived action/resource classification. The CLI
does not send effective principals, permission decisions, or policy scope.

## Smoke Outcomes

The smoke sidecar runs four cases:

- No client certificate: the mTLS transport boundary rejects the client.
- Trusted but unmapped client certificate: mTLS succeeds, but GRM application
  authentication fails because no fingerprint mapping exists.
- Mapped principal without required permission: authentication succeeds for
  `local-demo` / `cli/limited`, workspace creation is allowed, and
  `session.describe` is denied because `schema.inspect` is absent.
- Mapped principal with required permission: `local-demo` / `cli/full` creates
  a workspace and runs `session.describe`, `model.list`, `node.find`, and
  `edge.find` through the secured gRPC workspace service.

The MCP HTTP smoke sidecar runs the analogous adapter checks:

- No client certificate: the MCP adapter cannot connect to the secured gRPC
  service.
- Trusted but unmapped client certificate: mTLS succeeds, but GRM application
  authentication fails because no fingerprint mapping exists.
- Mapped principal without required permission: authentication succeeds for
  `local-demo` / `cli/limited`, workspace creation is allowed, and MCP
  `grm_schema_list` is denied because `schema.inspect` is absent.
- Mapped principal with required permission: `grm-mcp-http-smoke` initializes
  MCP Streamable HTTP, runs `tools/list`, ensures a stable tiny smoke model
  exists, creates and finds one node, and calls `grm_explain` and `grm_profile`
  through the `grm-mcp-http` service and secured gRPC backend. The service
  permission table must allow both the wrapper actions (`explain`, `profile`)
  and the underlying `node.read` resource.

Run just the service:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml up --build grm-secured
```

Run the secured gRPC service plus Compose-internal MCP HTTP adapter:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml --profile mcp up --build grm-secured grm-mcp-http
```

By default this does not publish the MCP HTTP port to the host. The smoke sidecar
talks to `http://grm-mcp-http:8080/mcp` on the Compose network. The adapter uses
`GRM_SERVICE_WORKSPACE_MODE=create-or-open` so retained demo workspace volumes
do not make repeat smoke runs fail with a stale fixed workspace reference. Its
startup retry loop is bounded and intended only to absorb local service
readiness races; persistent configuration, authentication, authorization, or
workspace errors cause the container to exit.

Host publishing is a separate opt-in profile:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml --profile mcp-host up --build grm-secured grm-mcp-http-host
```

The `mcp-host` profile publishes `127.0.0.1:8080` and serves MCP at `/mcp`.
This exposes a local credential-bearing MCP proxy: the adapter holds the mapped
demo mTLS client certificate and acts as `local-demo` / `cli/full` when calling
the secured gRPC service. Any local process that can reach the published
loopback endpoint can use that mapped demo principal through the adapter. Host
publishing is for controlled local inspection only; browser auth, bearer tokens,
OAuth, hosted/public MCP, and multi-user identity are non-goals for this slice.

Run the CLI smoke sidecar against the secured service:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml --profile smoke run --rm --no-deps grm-secured-smoke
```

Run the MCP HTTP smoke sidecar against the adapter:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml --profile smoke run --rm --no-deps grm-mcp-http-smoke
```

Stop the demo:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml down
```

Remove generated demo material:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml down -v
```

Only remove the volumes when you intend to discard the local demo CA, private
keys, security config, and workspace files.
