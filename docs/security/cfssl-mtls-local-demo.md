# CFSSL mTLS Local Demo

Status: local controlled demo

This demo provisions local certificate material with CFSSL, maps exact client
certificate fingerprints to canonical GRM principals, starts the local gRPC
workspace service in the secured profile, and runs CLI commands through
`ExecuteWorkspace`.

It proves only this local onboarding path:

- TLS server authentication and mTLS client-certificate transport trust are
  configured for the local service.
- GRM computes the SHA-256 fingerprint of the validated client certificate DER.
- The service maps that exact fingerprint to a canonical principal.
- The permission table independently authorizes typed workspace actions.
- The CLI sidecar parses human commands locally and sends typed workspace
  requests through the secured gRPC service.

It does not claim production PKI, certificate lifecycle management, hosted
tenancy, Cloudflare-edge identity, HTTP streaming MCP, bounded authoritative
audit, encryption at rest, attestation, receipts, state commitments, admin RPCs,
policy hot reload, or a general policy language.

## Files And Artifacts

Run the demo from the repository root with:

```bash
examples/cfssl-mtls/scripts/run-compose-smoke.sh
```

The wrapper starts the secured service detached, runs the smoke sidecar as a
one-shot container, returns the sidecar exit code, and stops the Compose stack
without deleting generated volumes.

The Compose stack creates three named volumes:

- `grm-cfssl-certs`: local demo CA, server certificate, client certificates,
  client private keys, and computed client fingerprint files.
- `grm-cfssl-config`: generated `security.json` consumed at service startup.
- `grm-cfssl-workspaces`: local autocommit GRM workspace files.

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
  `node.read`, `edge.create`, and `edge.read`.

The limited principal can create a workspace but cannot inspect schema, so a
CLI `session.describe` fails after authentication.

## Trust Boundaries

The generated CA and keys are local demo material inside Docker volumes. Any
container or operator with access to those volumes can read them. Do not reuse
them outside this demo.

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

Run just the service:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml up --build grm-secured
```

Run the CLI/smoke sidecar against it:

```bash
docker compose -p grm-cfssl-mtls-demo -f docker-compose.cfssl-mtls.yml --profile smoke run --rm --no-deps grm-secured-smoke
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
