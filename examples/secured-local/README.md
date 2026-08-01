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

## Bootstrap

From the repository root:

```bash
examples/secured-local/bootstrap.sh
```

Generated files are written under `.grm/secured-local/`, including:

- `ca.crt` and `ca.key`;
- `server.crt` and `server.key`;
- `admin-1.crt`, `admin-1.key`, and `admin-1.sha256`;
- `security.json`;
- `bootstrap-inputs.json`;
- `service.env`;
- `client.env`; and
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
examples/secured-local/bootstrap.sh --issuer local-admin --principal admin-1
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
  127.0.0.1:50051 .grm/secured-local/workspaces
```

Docker Compose:

```bash
docker compose -f examples/secured-local/docker-compose.secured-local.yml up --build
```

The Compose service runs the secured profile and publishes
`127.0.0.1:50051`. Run `bootstrap.sh` before starting Compose so
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

`flight-deck-profile.json` contains safe profile metadata, the endpoint, the
principal subject, the selected access template, the CA certificate path, and a
gateway hint. It does not contain client certificate or private-key paths. Use
`client.env`, the CLI, the local gateway, or another trusted local process to
hold client certificate and key material when a UI needs to connect through the
secured-local profile.
