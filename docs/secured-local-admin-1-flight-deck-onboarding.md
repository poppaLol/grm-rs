# First-Time Admin-1 Secured-Local Flight-Deck Onboarding

This runbook walks through the first secured-local GRM adoption path for
Admin-1 using the flight-deck.

It uses the passwordless secured-local model from ADR 0011:

```text
local certificate material
  -> Admin-1 client certificate fingerprint
  -> local-admin/admin-1 canonical principal
  -> owner-bootstrap-admin access template
  -> explicit permission table in security.json
  -> secured mTLS service
  -> local gateway using Admin-1 client credentials
  -> flight-deck showing verified secured identity
```

The browser never handles Admin-1 private keys or raw certificates. The local
gateway consumes `.grm/secured-local/gateway.env`, the CLI can consume
`.grm/secured-local/client.env`, and the flight-deck stores only safe
connection metadata.

This is a local secured demo and operator bootstrap path. It is not production
PKI, hosted identity, certificate revocation, browser credential handling,
multi-user admin lifecycle, or a live policy mutation API.

## Connection Shape

Cloud-hosted GRM can hide most of the deployment mechanics behind one HTTPS
origin and a managed control plane. Self-hosted secured-local currently exposes
two different connection layers:

```text
browser flight-deck
  -> local HTTP gateway
  -> secured gRPC service
  -> workspace files
```

That means the operator must keep several values aligned:

- flight-deck gateway URL;
- gateway listen port;
- upstream gRPC endpoint;
- TLS CA certificate;
- TLS domain name;
- Admin-1 client certificate;
- Admin-1 private key;
- workspace reference; and
- generated safe flight-deck profile metadata.

The security boundary is correct for the current implementation: the browser
must not handle private keys or raw certificates. The product gap is that the
safe local connector should make those relationships obvious and harder to
misconfigure.

Bootstrap now writes the service/client/gateway files from one set of local
inputs so the browser-facing gateway URL and upstream secured gRPC endpoint
stay aligned.

## Ports

The examples below run the secured-local service on `127.0.0.1:50052` so an
existing useful service on `127.0.0.1:50051` can keep running.

The flight-deck gateway still listens on `127.0.0.1:3001`, and the Vite UI
still listens on `127.0.0.1:8081`.

## 1. Bootstrap Admin-1

From the repository root:

```bash
cd /home/laurie/source/grm-rs

examples/secured-local/bootstrap.sh \
  --issuer local-admin \
  --principal admin-1 \
  --service-port 50052 \
  --gateway-port 3001 \
  --workspace flight-deck-demo
```

The helper writes local material under `.grm/secured-local/`, including:

```text
ca.crt
ca.key
server.crt
server.key
admin-1.crt
admin-1.key
admin-1.sha256
security.json
bootstrap-inputs.json
service.env
client.env
gateway.env
flight-deck-profile.json
```

Inspect the non-secret profile metadata:

```bash
cat .grm/secured-local/flight-deck-profile.json
```

Existing generated material is reused by default. Use `--force` only when you
intentionally want to regenerate or replace the local secured setup.

## 2. Start The Secured Service

Terminal 1:

```bash
cd /home/laurie/source/grm-rs

set -a
. .grm/secured-local/service.env
set +a

cargo run -p grm-service-api --bin grm-local-workspace-server -- \
  127.0.0.1:50052 .grm/secured-local/workspaces
```

This starts a secured local service using:

- server-authenticated TLS;
- mTLS client certificate requirement;
- Admin-1 certificate fingerprint mapping from `security.json`;
- default-deny exact permission-table authorization; and
- the local secured workspace directory at `.grm/secured-local/workspaces`.

## 3. Verify Admin-1

Terminal 2:

```bash
cd /home/laurie/source/grm-rs

ROOT=.grm/secured-local \
GRM_SERVICE_ENDPOINT=https://127.0.0.1:50052 \
examples/secured-local/scripts/verify-secured-service.sh
```

The verification script checks that:

- missing client credentials fail;
- a trusted but unmapped client certificate fails application authentication;
- Admin-1 succeeds through a bounded public CLI service flow; and
- failure output does not include private key or raw certificate material.

## 4. Seed A Flight-Deck Workspace

Use Admin-1's client environment to create sample data for the UI:

```bash
cd /home/laurie/source/grm-rs

set -a
. .grm/secured-local/client.env
set +a

cargo run -p grm-service-api --example local_workspace_client -- \
  https://127.0.0.1:50052 flight-deck-demo
```

The repeated endpoint is deliberate: the environment configures GRM clients,
and the example also accepts the endpoint as an argument.

## 5. Start The Flight-Deck Gateway As Admin-1

Terminal 3:

```bash
cd /home/laurie/source/grm-rs

examples/secured-local/start-flight-deck-gateway.sh
```

Expected startup line:

```text
GRM flight-deck HTTP adapter listening on http://127.0.0.1:3001
```

The gateway connects upstream to the secured GRM service using Admin-1's client
certificate and key. It exposes only bounded read-only JSON to the browser.

## 6. Start The Flight-Deck UI

Terminal 4:

```bash
cd /home/laurie/source/grm-rs/grm-flight-deck
npm run dev
```

Open:

```text
http://127.0.0.1:8081
```

In the UI:

1. Click `New profile`.
2. Name it `Admin-1 secured local`.
3. Set workspace to `flight-deck-demo`.
4. Turn `Fixture` off.
5. Leave `Gateway URL` blank.
6. Click `Save profile`.
7. Click `Connect`.

Leaving `Gateway URL` blank uses the Vite `/api` proxy to the local
gateway at `http://127.0.0.1:3001`.

Expected secured status:

```text
Secured
local-admin/admin-1 via mtls-certificate
secured-local-policy-v1
```

The default workbench view is `Query`. Connection fields stay hidden after the
profile is selected; use `Change connection` only when you need to edit the
gateway URL, workspace, profile kind, limit, or fixture toggle. Query can show
optional Explain/Profile panes, but in this slice those panes are local view
summaries derived from the bounded snapshot/filter, not service planner truth.

Open `Audit` from the workspace navigation to see the bounded local event
buffer. It records fixture/service snapshot observations and explicit Query
executions with redacted context. This is useful workbench navigation for the
Admin-1 journey, but it is not a live policy editor, permission table viewer,
external audit forwarding path, signed receipt, or state commitment.

Switch back to a fixture profile and then back to `Admin-1 secured local` to
confirm the local graph-store profile restoration path.

## Troubleshooting

If the gateway logs this:

```text
flight-deck gateway security status error: upstream request failed
```

the error is intentionally redacted. Check the upstream service path directly:

```bash
cd /home/laurie/source/grm-rs

GRM_SERVICE_ENDPOINT=https://127.0.0.1:50052 \
GRM_SERVICE_TLS_CA_CERT=.grm/secured-local/ca.crt \
GRM_SERVICE_TLS_DOMAIN_NAME=localhost \
GRM_SERVICE_TLS_CLIENT_CERT=.grm/secured-local/admin-1.crt \
GRM_SERVICE_TLS_CLIENT_KEY=.grm/secured-local/admin-1.key \
cargo run -p grm-service-api --example local_workspace_client -- \
  https://127.0.0.1:50052 flight-deck-demo
```

If that direct client fails, fix the secured service, port, TLS, or
`security.json` setup first.

If the direct client succeeds but the gateway fails, restart the gateway with
the same Admin-1 TLS environment:

```bash
examples/secured-local/start-flight-deck-gateway.sh
```

Common checks:

- the secured service is listening on `127.0.0.1:50052`;
- `GRM_SERVICE_ENDPOINT` uses `https://`, not `http://`;
- `GRM_SERVICE_TLS_DOMAIN_NAME=localhost` is present;
- the gateway has `GRM_SERVICE_TLS_CA_CERT`;
- the gateway has `GRM_SERVICE_TLS_CLIENT_CERT`;
- the gateway has `GRM_SERVICE_TLS_CLIENT_KEY`; and
- the Admin-1 certificate fingerprint in `admin-1.sha256` appears in
  `security.json`.

## Current Limits

- No default admin password.
- No browser handling of private keys, raw certificates, bearer tokens, or
  policy tables.
- No browser display of certificate fingerprints, private credential paths, or
  rendered permission tables.
- No hosted identity, OIDC/OAuth, or production certificate lifecycle.
- No live service-side user or permission mutation API.
- No service-backed query/explain/profile/audit-events endpoint in this
  workbench slice; current Explain/Profile/Audit entries are local UI
  summaries unless a later read-only gateway surface replaces them.
- No hosted durability, multi-writer coordination, or production security
  claim.
