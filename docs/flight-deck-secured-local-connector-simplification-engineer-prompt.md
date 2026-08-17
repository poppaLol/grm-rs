# GRM Flight-Deck Secured-Local Connector Simplification Engineer Prompt

## Goal

Make the self-hosted secured-local Admin-1 flight-deck setup less
configuration-heavy and less mismatch-prone.

The current secure path works, but it exposes too much plumbing: the browser
talks to a local HTTP gateway, the gateway talks to the secured gRPC service,
and Admin-1 TLS material is held by the gateway or CLI environment. That is the
right security boundary, but the operator experience should be a named local
connector instead of a bundle of manually coordinated environment variables.

## Why This Matters

Cloud-hosted GRM can hide certificates, upstream service URLs, and gateway
routing behind one managed HTTPS endpoint. Self-hosted secured-local cannot put
client private keys in the browser, so it needs a local connector process.

The product problem is not the gateway itself. The product problem is that the
current UI and docs make the operator reason about:

- service port versus gateway port;
- browser URL versus upstream gRPC endpoint;
- `http://` versus `https://`;
- TLS CA and domain name;
- Admin-1 certificate and private-key paths;
- workspace references; and
- safe profile metadata.

This is a trust-building onboarding surface. A first-time Admin-1 should be
able to verify "I am connected as local-admin/admin-1 to this workspace through
this local connector" without becoming a GRM deployment specialist.

## Starting Point

- Work in `/home/laurie/source/grm-rs`.
- Read:
  - `docs/adr/0011-passwordless-secured-local-bootstrap.md`
  - `docs/secured-local-admin-1-flight-deck-onboarding.md`
  - `examples/secured-local/README.md`
  - `examples/secured-local/bootstrap.sh`
  - `grm-flight-deck/README.md`
  - `grm-flight-deck-gateway/src/main.rs`
  - `grm-flight-deck/src/App.tsx`
  - `grm-flight-deck/src/graphStore.ts`
- Preserve the current security boundary: the browser stores only safe
  non-secret metadata; the gateway or CLI holds client cert/key paths.

## Scope

### 1. Bootstrap Port And Endpoint Options

Extend `examples/secured-local/bootstrap.sh` so a self-hosted operator can
choose non-conflicting local ports without hand-editing generated files.

Useful options:

```text
--service-port 50052
--gateway-port 3001
--workspace flight-deck-demo
```

The generated `client.env`, `flight-deck-profile.json`, and any new
gateway-specific env file should agree on the selected upstream endpoint.

Do not change the default local secured profile or make secured-local the
default startup mode.

### 2. Gateway Environment

Generate a safe, reusable gateway environment file such as:

```text
.grm/secured-local/gateway.env
```

It may include local process paths needed by the trusted gateway:

```text
GRM_SERVICE_ENDPOINT=https://127.0.0.1:50052
GRM_SERVICE_TLS_CA_CERT=...
GRM_SERVICE_TLS_DOMAIN_NAME=localhost
GRM_SERVICE_TLS_CLIENT_CERT=...
GRM_SERVICE_TLS_CLIENT_KEY=...
GRM_FLIGHT_DECK_GATEWAY_BIND=127.0.0.1:3001
```

Do not expose `gateway.env` through browser JSON responses.

### 3. One-Command Local Connector Starter

Add a helper script such as:

```text
examples/secured-local/start-flight-deck-gateway.sh
```

It should load `.grm/secured-local/gateway.env` and start
`grm-flight-deck-gateway`.

Normal output should identify:

- gateway URL;
- upstream endpoint;
- expected principal if available from safe metadata; and
- workspace hint if available.

Normal output must not print private-key material, raw certificates, or rendered
permission tables.

### 4. Flight-Deck UI Language

Rename or clarify the current `Service base URL` field so it is clearly the
browser-facing gateway URL.

Preferred language:

```text
Gateway URL
```

The UI should not imply the browser connects directly to the secured gRPC
service. When feasible, display safe profile metadata as:

```text
Connector: Admin-1 secured local
Gateway: http://127.0.0.1:3001
Upstream: https://127.0.0.1:50052
Identity: local-admin/admin-1
Workspace: flight-deck-demo
```

Only display values that are safe to store or returned by the read-only status
surface.

### 5. Documentation

Update the Admin-1 onboarding runbook and secured-local README so the happy path
uses the new port-aware bootstrap and gateway starter. Keep the older manual
commands as troubleshooting or advanced detail.

## Non-Goals

- No browser private-key, client-certificate, bearer-token, password, or raw
  credential handling.
- No production PKI, certificate revocation, hosted identity, OIDC/OAuth, or
  user-password login.
- No live service-side user, permission, role, access-level, or policy mutation
  API.
- No broad admin plane.
- No permission-table viewer.
- No hosted durability, multi-writer coordination, or production security
  claim.
- No new public query language.

## Constraints

- Keep the browser-to-gateway and gateway-to-gRPC layers distinct.
- Keep mTLS transport identity distinct from authenticated application
  principal and authorization.
- Preserve exact permission-table enforcement and default-deny secured profile
  semantics.
- Preserve anonymous_local and docker_local_insecure compatibility.
- Keep all new outputs redacted and bounded.
- Do not treat access-template names as canonical authority; only expanded
  permissions are enforced.

## Acceptance Criteria

- A first-time operator can bootstrap secured-local on a non-default service
  port such as `50052` without editing generated files.
- A generated gateway env file starts the local gateway with Admin-1 client
  credentials.
- A one-command helper starts the gateway/connector and prints safe connection
  status hints.
- Flight-deck labels the browser-facing URL as a gateway URL or otherwise makes
  the two-layer connection model explicit.
- The Admin-1 onboarding doc uses the simplified path.
- Existing anonymous local and Docker-local flows still work.
- No browser response or persisted profile includes private-key paths, raw certs,
  raw keys, certificate fingerprints, permission tables, or policy internals.

## Expected Checks

- `examples/secured-local/bootstrap.sh --service-port 50052 --issuer local-admin --principal admin-1`
- Secured service start with generated env.
- `examples/secured-local/scripts/verify-secured-service.sh` against the chosen
  service port.
- New gateway starter against the chosen service port.
- `cargo test -p grm-flight-deck-gateway` if gateway code changes.
- `npm run test` and `npm run build` in `grm-flight-deck` if frontend code
  changes.
- Focused script tests or shellcheck-equivalent validation where practical.

## Memory Updates After Merge

After merge, update GRM project memory:

- add or update a WorkSlice for secured-local connector simplification;
- link it to the flight-deck roadmap and security-core roadmap;
- keep future hosted/admin-control-plane claims out of completed status; and
- preserve the browser private-key non-goal as an active constraint.
