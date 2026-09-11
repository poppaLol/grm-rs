# GRM Secured-Local Bootstrap

Status: helper scaffold for ADR 0011.

This example creates the first passwordless secured-local setup for Admin no.1.
It generates local certificate material, maps the Admin no.1 client certificate
fingerprint to a canonical principal, expands an editable access template into
explicit permissions, renders `security.json`, and can verify the result through
the public gRPC/CLI service surface.

It follows [ADR 0011](../../docs/adr/0011-passwordless-secured-local-bootstrap.md).
There is no default admin password, no password reset, no bearer-token
bootstrap, no browser private-key handling, and no live service-side policy
mutation API in this slice.

For an end-to-end first-time Admin no.1 walkthrough through the local gateway
and flight-deck, including a non-conflicting `50052` secured-service port, see
[First-Time Admin-1 Secured-Local Flight-Deck Onboarding](../../docs/secured-local-admin-1-flight-deck-onboarding.md).

## Profiles

GRM now has three separate local profiles:

- `anonymous_local`: direct loopback developer/test profile with anonymous local
  access. It is selected explicitly by local constructors or
  `GRM_SERVICE_SECURITY_PROFILE=anonymous_local`.
- `docker_local_insecure`: Docker-hostable local demo profile used by
  `docker-compose.yml`. It has no TLS, authentication, or authorization and
  should publish the host port on loopback only.
- `secured` with local mTLS: this helper profile. It uses a local CA, server
  certificate, Admin no.1 client certificate, certificate fingerprint mapping,
  and explicit permission-table authorization.

The secured-local helper is still local demo/operator material, not production
PKI, hosted identity, certificate revocation, multi-user admin lifecycle, or
tamper-evident audit.

## One-Command Flight Deck Demo

From the repository root:

```bash
examples/secured-local/start-flight-deck-demo.sh
```

This starts the secured local service on `127.0.0.1:50052`, starts the trusted
local flight-deck gateway on `127.0.0.1:3001`, starts the Vite flight-deck UI on
`127.0.0.1:8081`, verifies Admin no.1, seeds `flight-deck-demo`, and prints the
browser URL plus log paths.

Use explicit ports when needed:

```bash
examples/secured-local/start-flight-deck-demo.sh \
  --service-port 50052 \
  --gateway-port 3001 \
  --ui-port 8081 \
  --workspace flight-deck-demo
```

The runner prints only safe operator metadata in normal output: browser URL,
gateway URL, upstream endpoint, workspace, identity label, and log paths. Verbose
helper output is captured under `.grm/secured-local/logs/`. Press Ctrl-C to stop
the service, gateway, and UI children.

## Manual Bootstrap

From the repository root:

```bash
examples/secured-local/bootstrap.sh \
  --service-port 50052 \
  --gateway-port 3001 \
  --workspace flight-deck-demo
```

Generated files are written under `.grm/secured-local/`, including:

- `ca.crt` and `ca.key`;
- `server.crt` and `server.key`;
- `admin-1.crt`, `admin-1.key`, and `admin-1.sha256`;
- `security.json`;
- `bootstrap-inputs.json`;
- `service.env`;
- `client.env`;
- `gateway.env`; and
- `flight-deck-profile.json`.

Existing credential/config files are reused by default. Pass `--force` only
when you intentionally want to regenerate or overwrite the local material:

```bash
examples/secured-local/bootstrap.sh --force
```

When generated files already exist, the helper verifies that the requested
issuer, principal subject, access template, and policy version match
`bootstrap-inputs.json`. If they differ, bootstrap stops before reusing the
existing identity/config and asks you to rerun with `--force`.

`bootstrap.sh` only accepts access templates marked as bootstrap-capable for
Admin no.1. The lower-privilege starter templates are for later deployment-local
assignment, not the first-admin setup.

Choose the default Admin no.1 principal subject and issuer explicitly with:

```bash
examples/secured-local/bootstrap.sh \
  --issuer local-admin \
  --principal admin-1 \
  --service-port 50052 \
  --gateway-port 3001 \
  --workspace flight-deck-demo
```

Access-level names are templates only. The service enforces the expanded
`permission_table.assignments` in `security.json` over canonical actions,
resources, and scopes.

## Start The Service

Host process:

```bash
set -a
. .grm/secured-local/service.env
set +a
cargo run -p grm-service-api --bin grm-local-workspace-server -- \
  127.0.0.1:50052 .grm/secured-local/workspaces
```

Docker Compose:

```bash
GRM_SECURED_LOCAL_SERVICE_PORT=50052 \
docker compose -f examples/secured-local/docker-compose.secured-local.yml up --build
```

The Compose service runs the secured profile and publishes
`127.0.0.1:50051` by default. Use
`GRM_SECURED_LOCAL_SERVICE_PORT=50052` when you want Compose to publish a
non-default host port. Run `bootstrap.sh` before starting Compose so
`.grm/secured-local/security.json` and certificates exist.

## Verify

With the secured service running:

```bash
examples/secured-local/scripts/verify-secured-service.sh
```

The verification script checks:

- missing client credentials fail;
- a trusted but unmapped client certificate fails application authentication;
- Admin no.1 succeeds through a bounded public CLI service flow; and
- failure output does not include private key or raw certificate material.

You can ask bootstrap to start Compose and run verification:

```bash
examples/secured-local/bootstrap.sh --start --verify
```

## Flight Deck

Start the trusted local flight-deck connector with:

```bash
examples/secured-local/start-flight-deck-gateway.sh
```

The starter loads `.grm/secured-local/gateway.env`, prints the browser-facing
gateway URL, upstream secured service endpoint, expected principal, and
workspace hint, then runs `grm-flight-deck-gateway`.

`flight-deck-profile.json` contains safe browser-facing profile metadata: the
connector name, gateway URL, upstream endpoint, identity label, workspace, and
selected access template. It does not contain certificate paths, private-key
paths, raw certificates, fingerprints, permission tables, or policy internals.
Use `gateway.env`, `client.env`, the CLI, the local gateway, or another trusted
local process to hold client certificate and key material when a UI needs to
connect through the secured-local profile.

The one-command demo runner configures the Vite `/api` proxy to the selected
gateway port. When running the UI manually with a non-default gateway port, set
`GRM_FLIGHT_DECK_DEV_PROXY_TARGET` before `npm run dev`, or enter the full
gateway URL in the flight-deck connection settings.
