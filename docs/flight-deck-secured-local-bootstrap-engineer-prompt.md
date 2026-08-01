# GRM Secured-Local Bootstrap Engineer Prompt

## Goal

Create the first passwordless secured-local bootstrap helper scaffold for GRM.

The helper path should make Admin no.1 setup approachable without introducing a
default password or weaker authentication provider. It should generate local
certificate material, map the first admin certificate to a canonical principal,
expand a starter access template into explicit permissions, render the service
security config, and verify a secured connection through the public service
surface.

## Why This Matters

GRM already has mTLS certificate-to-principal mapping and deterministic
permission-table authorization. The missing piece is first-install UX. A new
operator should not need to hand-author every certificate, fingerprint, and
permission table entry before they can make the first secured local connection.

The helper must preserve the security model:

```text
credential evidence -> canonical principal -> access template -> explicit permissions -> service enforcement
```

Access-level names are UI/helper templates only. The service remains
default-deny and enforces explicit permissions over canonical actions,
resources, and scopes.

## Starting Point

- Read ADR 0011:
  `docs/adr/0011-passwordless-secured-local-bootstrap.md`.
- Reuse existing security docs and demos, especially:
  `docs/security/cfssl-mtls-local-demo.md`,
  `docs/grpc-quickstart.md`, and
  `docs/grpc-docker-service.md`.
- Reuse existing service config inputs:
  `GRM_SERVICE_SECURITY_PROFILE=secured`,
  `GRM_SERVICE_SECURITY_CONFIG`,
  `GRM_SERVICE_TLS_SERVER_CERT`,
  `GRM_SERVICE_TLS_SERVER_KEY`, and
  `GRM_SERVICE_TLS_CLIENT_CA_CERT`.
- Do not add password auth.

## Scope

### 1. Helper Layout

Create a secured-local helper area:

```text
examples/secured-local/
  README.md
  bootstrap.sh
  docker-compose.secured-local.yml
  templates/
    access-levels.json
    security-config.template.json
  scripts/
    generate-ca.sh
    generate-server-cert.sh
    generate-admin-client-cert.sh
    fingerprint-cert.sh
    render-security-config.py
    verify-secured-service.sh
    write-flight-deck-profile.py
```

If an existing CFSSL demo structure is a better fit, adapt the paths while
keeping the same helper responsibilities.

### 2. Bootstrap Wrapper

Implement `bootstrap.sh` as the happy path:

- create `.grm/secured-local/`;
- generate or reuse local CA material;
- generate server cert for `localhost`, `127.0.0.1`, and relevant Compose
  service names;
- generate Admin no.1 client cert;
- fingerprint Admin no.1 certificate;
- render `security.json`;
- expand `Owner / Bootstrap Admin` access template into explicit permissions;
- write `.env` or Compose override values;
- optionally start Docker Compose when requested;
- optionally run a smoke verification; and
- print flight-deck connection instructions.

Make the script idempotent enough for local iteration. Avoid overwriting
existing credential material unless an explicit `--force` flag is supplied.

### 3. Access Templates

Create starter access templates as editable deployment-local bundles.

At minimum include:

- `owner-bootstrap-admin`;
- `workspace-admin`;
- `schema-maintainer`;
- `graph-reader`;
- `audit-reader`;
- `read-only-explorer`.

Templates must expand to explicit permission objects accepted by the current
service security config parser. Do not make template names service authority.

Keep dangerous or reserved/admin permissions visibly separate. If the current
service has no implemented admin operation for a permission, do not imply it is
usable.

### 4. Security Config Rendering

Render a config compatible with `ServiceSecurityConfig::secured_from_config_file`:

- `certificate_mappings`;
- `permission_table.version`;
- `permission_table.assignments`;
- principal issuer/subject;
- scope;
- explicit permissions.

Use the current JSON config shape unless there is a separate accepted decision
to introduce SOML/YAML input. A SOML/YAML-friendly source template can be
deferred.

### 5. Verification

Add a smoke verification that proves:

- secured service starts with generated TLS and security config;
- missing or wrong client credentials fail;
- Admin no.1 can complete a small allowed operation;
- service errors do not print private keys or raw secrets; and
- generated config is accepted by the public service path.

Prefer existing public gRPC client examples or a small helper that calls the
same public service surface.

## Non-Goals

- No default password.
- No password-authentication provider.
- No hosted identity management.
- No browser private-key handling.
- No live policy mutation API.
- No production CA/revocation lifecycle claim.
- No new canonical role/access-level semantics.
- No broad flight-deck admin console.

## Constraints

- Authentication and authorization stay separate.
- mTLS trust and certificate mapping establish principal identity, not
  permission.
- Access templates expand to explicit permissions.
- Secured profile remains default-deny and fail-closed.
- Secured-local is additive. Do not make bootstrap-generated config required,
  do not change default startup to `secured`, and do not regress existing
  `anonymous_local`, `docker_local_insecure`, or flight-deck anonymous fixture
  workflows.
- Helper output must not leak private keys, raw credentials, bearer tokens, or
  policy internals beyond generated local config files.
- Tests should exercise public service/helper behavior, not private runtime
  shortcuts.

## Acceptance Criteria

- ADR 0011 is referenced from the helper README.
- The secured-local helper scaffold exists with a documented happy path.
- Running the bootstrap creates local credential material and `security.json`
  under `.grm/secured-local/`.
- Admin no.1 is represented as a canonical principal mapped from the generated
  client certificate fingerprint.
- The selected starter access template renders to explicit permissions.
- Existing files are not overwritten without explicit force.
- A smoke verification proves the generated secured config works through the
  public service surface.
- Existing anonymous-local startup still works, Docker local insecure behavior
  still works, bootstrap-generated config remains optional, default startup does
  not switch to secured, and existing flight-deck fixture/local anonymous
  profiles are not broken.
- Docs explain anonymous local, Docker local insecure, and secured local mTLS
  profiles without overstating production or hosted guarantees.

## Expected Checks

- Shell/script lint if available.
- Unit tests for config rendering and template expansion.
- Public service smoke test for generated secured-local config.
- `cargo test -p grm-service-api` or the narrow relevant service tests.
- Any existing CFSSL/demo smoke checks that remain applicable.
