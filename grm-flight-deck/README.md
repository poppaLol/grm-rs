# GRM Flight-Deck UI

This is the first React/Vite/TypeScript flight-deck workbench promoted from the
`spike/react-vite-flight-deck` experiment.

It is a browser adapter over bounded service/runtime data. The UI can configure
and attempt a local snapshot connection, show connection status and errors,
show read-only security posture, render a 2D graph, narrow the visible graph
with simple filters, and inspect selected nodes or edges.

## Run

```bash
npm install
npm run dev
```

Open `http://127.0.0.1:8081`.

The app starts with fixture data enabled so the UI remains reviewable without a
running service. To use real local service data, run a GRM workspace service and
the read-only flight-deck HTTP adapter:

```bash
GRM_SERVICE_ENDPOINT=http://127.0.0.1:50051 \
cargo run -p grm-flight-deck-gateway
```

Seed a small demo workspace through the public gRPC client example:

```bash
cargo run -p grm-service-api --example local_workspace_client -- \
  http://127.0.0.1:50051 flight-deck-demo
```

Then disable `Fixture` in the UI and use workspace `flight-deck-demo`. Leaving
`Service base URL` blank uses the Vite `/api` proxy to
`http://127.0.0.1:3001`; setting it to `http://127.0.0.1:3001` also works for
local development.

The gateway also exposes:

```bash
curl http://127.0.0.1:3001/api/security/status
```

That endpoint is read-only observability. It labels `anonymous_local`,
`docker_local_insecure`, or `secured`; in secured mode it reports the mapped
principal issuer/subject, authentication method, and policy version when the
service accepts the gateway credentials.

## Current Proof Boundary

- The checked-in React app is real and buildable as a Vite frontend.
- The connection form persists non-secret local settings in `localStorage`.
- Fixture data is clearly labelled and used only for local UI review.
- The security status panel is fed by a service-side read-only status RPC
  through the local gateway.
- Real data is loaded through the read-only local HTTP adapter, which opens a
  GRM service workspace and asks for typed schema, node.find, and edge.find
  requests through the existing gRPC workspace client.
- The graph filter is a client-side delimiter over a bounded snapshot, not a
  public GRM query language.
- The event band is a UI-side model hook for future execution animation.

## Security Boundary

The fixture and anonymous local modes are not authenticated or hosted security
profiles. `docker_local_insecure` is a distinct local Docker demo profile, and
`secured` means the service accepted configured credentials for the status
request.

Browser JavaScript cannot directly choose arbitrary client TLS certificate and
private-key files for `fetch`. Future secured profiles should use
browser-managed certificates, a local connector, a reverse proxy/dev helper, or
another explicit backend path that consumes credentials safely. The current
gateway may read TLS paths from process environment variables; it does not send
those paths or certificate material to the browser.

The UI must not accept or display raw credentials, private key paths, raw
certificates, policy table contents, or unbounded request/response bodies.

## Deferred

- Production authentication, authorization, policy authoring, and admin flows.
- Hosted or multi-user workspace claims.
- Real service event streams for execution animation.
- 3D or spatial navigation layers.
