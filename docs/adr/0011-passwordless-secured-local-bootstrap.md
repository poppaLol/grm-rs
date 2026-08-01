# ADR 0011: Use Passwordless Secured-Local Bootstrap With Editable Access Templates

Status: Accepted

Date: 2026-08-01

## Context

GRM now has the foundations for a secured local service profile:

- TLS and mTLS service transport;
- explicit mTLS certificate fingerprint mapping to canonical principals;
- default-deny authorization;
- an exact deterministic permission table over server-derived actions,
  resources, and scopes;
- durable bounded security audit; and
- a first flight-deck UI that can connect through a local gateway.

What is still missing is the first-install administrator experience. The first
administrator, or "Admin no.1", needs to establish the first usable secured
connection and then assign authority to principals without weakening the
security model.

A default password would be convenient, but it would introduce a markedly
weaker and breachable authentication mechanism before GRM has a password,
reset, rotation, lockout, secret-storage, recovery, and audit lifecycle. That
would conflict with the accepted security direction that authentication and
authorization remain explicit, tested, and deployment-profile scoped.

At the same time, asking a first-time operator to hand-author certificate
material, principal mappings, and permission tables is poor UX. GRM should make
the passwordless secured-local path easy through helper scripts, clear docs,
and flight-deck guidance.

## Decision

GRM will use a passwordless-first secured-local bootstrap path.

For the secured-local install path, GRM will not introduce a default admin
password or password-based bootstrap mechanism. Instead, helper tooling will
generate or assemble local certificate material, map the Admin no.1 client
certificate to a canonical principal, and render a service security
configuration that expands an initial access template into explicit
permissions.

The bootstrap flow is:

```text
local credential material
  -> Admin no.1 client certificate fingerprint
  -> canonical principal mapping
  -> selected access template
  -> expanded explicit permissions
  -> service security config
  -> verified secured connection
```

GRM defines the canonical permission contract:

- actions;
- resources;
- scopes;
- default-deny and fail-closed semantics;
- server-derived operation classification;
- audit meaning; and
- reserved administrative boundaries.

Operators may define deployment-local access levels. Access levels are
templates or bundles that expand into explicit permissions. The service must
not treat access-level names such as `Owner`, `Workspace Admin`, or
`Graph Reader` as magic authority. The service enforces only the expanded
permission table.

GRM may ship starter access templates for common local setups, such as:

- Owner / Bootstrap Admin;
- Service Admin;
- Workspace Admin;
- Schema Maintainer;
- Graph Reader;
- Audit Reader; and
- Read Only Explorer.

Those names are user-facing defaults, not canonical security semantics. Admins
may copy, rename, edit, and create custom deployment-local access levels. Any
dangerous reserved/admin permissions must remain visibly separated and require
deliberate selection in helper output and future UI.

## Helper Structure

The first helper scaffold should live under an example/demo-oriented secured
local area rather than inside the core service runtime:

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

The main wrapper should be:

```text
examples/secured-local/bootstrap.sh
```

It should create `.grm/secured-local/`, generate local certificate material,
fingerprint the Admin no.1 certificate, render the service security config,
write a local environment file or Compose override, optionally start the local
secured service, run a smoke verification, and print flight-deck connection
instructions.

The smaller helpers should be usable independently:

```text
generate-local-ca
generate-server-cert
generate-client-cert --issuer local-admin --principal admin-1
fingerprint-client-cert
render-security-config --issuer local-admin --principal admin-1 --access-level owner
verify-admin-connection
write-flight-deck-profile
```

Helper output may include file paths, fingerprints, generated config paths, and
commands needed by the operator. It must not print private keys, raw
credentials, raw certificates beyond explicit certificate files, or policy
internals more broadly than the generated local config files already contain.

## Flight-Deck Relationship

The flight-deck should guide the operator through connection verification and
safe profile reuse, but the browser UI must not handle private keys directly.

Near-term flight-deck behavior should be:

- display selected connection profile mode;
- display whether the profile is anonymous local, Docker local insecure, or
  secured local mTLS;
- restore safe non-secret connection metadata from local browser storage;
- show principal/access-template names only when supplied by safe profile
  metadata or verified service responses; and
- help the operator find and run the local bootstrap helpers.

Future flight-deck admin UX may edit access templates or render policy files,
but service-side policy mutation APIs require a separate accepted design and
public-boundary proof.

## Non-Goals

- No default admin password.
- No password, password-reset, lockout, or password-authentication provider.
- No browser handling of private keys or raw credential secrets.
- No hosted identity management, OIDC/OAuth, or user-password login.
- No live service-side policy mutation API in the first helper slice.
- No claim that access-level names are canonical GRM authority.
- No production PKI, certificate-revocation, or hosted admin claim.
- No weakening of mTLS principal mapping, default-deny authorization, or
  explicit permission-table enforcement.
- No change to the default local startup posture: secured-local is an added
  helper path, not the default service profile.

## Consequences

Positive consequences:

- The easiest secured-local path stays aligned with existing mTLS and
  permission-table foundations.
- GRM avoids introducing a weaker bootstrap password solely for convenience.
- Admin no.1 gets a concrete, repeatable setup path.
- Starter access templates improve UX while preserving explicit permission
  enforcement.
- Future admin UI can build on templates without changing service semantics.

Tradeoffs:

- Local certificate tooling becomes part of the first-install UX.
- Operators must understand that access levels compile to explicit permissions.
- The helper scaffold must be tested carefully to avoid generating overbroad or
  misleading authority.
- Hosted or user-login deployments still need separate identity-provider work.

## Required Proof For The First Helper Slice

The first helper implementation should prove:

- existing `anonymous_local` startup and developer workflows still work;
- existing `docker_local_insecure` Docker local behavior still works;
- bootstrap-generated config is optional and is not required for legacy local
  startup;
- default startup does not change to the secured profile;
- existing flight-deck fixture and local anonymous profiles are not broken;
- generated local cert material can start a secured local service;
- Admin no.1 certificate fingerprint is mapped to the intended canonical
  principal;
- the selected starter access template expands to explicit permissions;
- the rendered service security config is accepted by
  `GRM_SERVICE_SECURITY_CONFIG`;
- the secured service rejects missing or wrong client credentials;
- the Admin no.1 client can complete a bounded smoke operation through the
  public service surface;
- private keys and raw secrets are not printed in ordinary helper output; and
- docs clearly distinguish anonymous local, Docker local insecure, and secured
  local mTLS profiles.

Compatibility rule: ADR 0011 adds an easier secured-local path; it must not turn
secured-local into the default or regress anonymous-local developer workflows.

## Open Questions

- Which certificate generation tool should the helper prefer: OpenSSL, CFSSL,
  `rcgen`, or a small Rust helper binary?
- Should helper-generated access templates support JSON only, or a more
  operator-friendly SOML/YAML layer that renders JSON security config?
- Which exact permissions should the first `Owner / Bootstrap Admin` template
  include?
- What minimum service status endpoint should the flight-deck use to verify
  principal and policy version safely?
