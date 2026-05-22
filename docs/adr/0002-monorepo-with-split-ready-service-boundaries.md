# ADR 0002: Keep Monorepo While Designing Split-Ready Service Boundaries

Status: Accepted

Date: 2026-05-22

## Context

GRM is moving toward a future service backend with a typed gRPC/protobuf
contract. That raises a repository question: should the service backend become
a separate codebase, similar to treating Neo4j as an external service reached
over a port, or should it remain in the current `grm-rs` repository while the
architecture is still forming?

The service contract is not fully frozen yet. Recent work is still clarifying
the runtime dispatcher, durable mutation outcomes, adapter boundaries, schema
memory, and future service API shape. Splitting repositories too early would
make those cross-cutting changes slower and more expensive.

At the same time, staying in one repository must not imply that CLI, Python,
MCP, runtime, and service code can freely depend on each other's internals. The
future service should still behave like an external dependency from the
perspective of clients: connect to an endpoint, speak a typed API, and do not
reach through private process-local implementation details.

## Decision

GRM should stay in a monorepo for now, but design the service layer as if it can
split into a separate codebase later.

The monorepo is a development convenience, not the API boundary. The API
boundary is the typed service contract and client-facing behavior.

Future service work should be organized as separate crates or packages inside
the monorepo. A likely shape is:

- core runtime/library code
- service API/proto crate
- service daemon crate
- optional shared client crate
- CLI, Python, MCP, and future UI adapters

The service daemon should be treated like an external dependency even while it
lives in the same repository. Clients should communicate through the public
service API or explicit client abstractions, not by depending on daemon
internals.

## Consequences

Positive consequences:

- Runtime, proto/API, CLI, Python, MCP, docs, and tests can still evolve
  together while the contract is settling.
- Refactors across adapter and runtime boundaries remain cheap enough to keep
  architecture honest.
- The service can grow in a reviewable crate boundary before requiring
  separate release, CI, packaging, and governance.
- Future split remains possible once the service has a stable contract and
  independent lifecycle.

Tradeoffs:

- Review discipline is required so monorepo convenience does not become hidden
  coupling.
- Tests must distinguish embedded runtime behavior from remote service/client
  behavior.
- Crate boundaries and public APIs need to be treated as real architecture, not
  merely folder organization.
- Marketplace or hosted-service packaging concerns may eventually require
  stronger separation than the monorepo provides.

## Guidance

Prefer separate crates and public contracts over private cross-crate shortcuts.

Future service-facing work should preserve these rules:

- The service API is typed structured operations, not CLI command text.
- CLI, Python, MCP, and UI layers are adapters over runtime or service
  contracts.
- A daemon crate should expose behavior through its service/client boundary.
- Tests should prove client/service behavior through the same boundary a real
  client would use.
- Configuration should make remote service mode feel like connecting to an
  endpoint, similar in spirit to connecting to Neo4j.

Do not split into a separate repository until at least one of these conditions
is true:

- the service has an independent release cadence
- hosted/cloud packaging needs separate CI, security, or deployment controls
- external users depend on the service API independently of the embedded crate
- daemon or infrastructure code overwhelms the library repository
- commercial, marketplace, or visibility concerns require different governance

## Open Questions

- What should the service API crate be named: `grm-proto`,
  `grm-service-api`, or something else?
- Should a shared client crate exist from the first daemon PR, or wait until
  multiple adapters need it?
- Which integration tests should become the first proof that CLI/Python/MCP can
  talk to a GRM service as a remote backend?
- What release/versioning policy should govern the service API once external
  users can depend on it?

