# ADR 0006: Use Explicit mTLS Certificate Mapping As The First Application Authentication Provider

Status: Accepted

Date: 2026-06-22

## Context

GRM's secured service profile distinguishes transport-peer evidence from an
authenticated application principal and from authorization. The Phase 1
security proof implements that separation, but it does not yet include a
production-capable authentication provider.

The service already supports mutual TLS. Its public tests prove that the
configured client certificate authority accepts trusted client certificates,
rejects missing and untrusted certificates, and does not treat a trusted client
certificate as application authorization. The current internal
`TransportPeer`, however, records only whether a certificate was present. It
does not carry enough trusted certificate evidence to establish a principal.

The first deployment need is controlled service-principal access: a relatively
small number of trusted APIs authenticate and operate with workspace-level
permissions. Explicit certificate allow-listing is appropriate for that scope,
but certificate fingerprints must not become GRM's universal or permanent
principal model.

## Decision

GRM will implement explicit mTLS certificate mapping as its first application
authentication provider for controlled secured-service deployments.

After the TLS stack has validated the client certificate chain, the service
will compute the SHA-256 fingerprint of the canonical DER encoding of the
validated leaf certificate. An explicitly configured mapping table will map
that fingerprint to a canonical application principal:

```text
validated mTLS leaf-certificate evidence
  -> exact configured fingerprint mapping
  -> canonical application principal
  -> authorization
```

The fingerprint identifies credential evidence, not the principal itself. The
mapping supplies the canonical principal identifier and the authentication
method is `mtls-certificate`. Authorization receives the canonical principal
and server-derived operation scope, not the certificate, fingerprint, asserted
actor, or client-supplied permissions.

The canonical principal identifier is the pair `(issuer, subject)`. In this
structure, `issuer` means the GRM identity namespace or authority within which
`subject` is unique. It is not implicitly the X.509 certificate issuer name,
an OIDC issuer URL, or a JWT `iss` claim. A provider contract may deliberately
adopt a normalized credential issuer as the canonical GRM issuer, but that
mapping must be explicit. Credential provenance, including certificate issuer
or future token issuer, remains separately labelled authentication evidence.

Authentication fails closed when certificate evidence is missing, malformed,
unmapped, or ambiguous. Duplicate fingerprint entries are invalid
configuration and must fail startup or atomic configuration reload. Multiple
certificate fingerprints may map to the same canonical principal so certificate
rotation can use a deliberate overlap period.

The provider returns identity only. It does not return roles, permissions,
workspace scope, actor identity, or policy decisions. A client-supplied actor
identifier remains a separately labelled assertion and cannot change the
authenticated principal.

Raw certificates, fingerprints, private keys, and mapping contents must not be
exposed in ordinary logs, public errors, tracing attributes, or audit details.
Operational diagnostics may use bounded, non-secret mapping identifiers where
needed.

## Provider-Independent Identity Boundary

The architectural contract is:

```text
authentication evidence -> canonical principal -> authorization
```

The canonical principal and authorization contracts remain independent of
mTLS and certificate fingerprints. Future providers may establish workload,
service, or user principals from mechanisms such as SPIFFE-compatible
identities, OIDC/OAuth authentication, or other accepted credentials.

Those providers may complement or replace fingerprint mapping when operational
scale, user-level auditing, delegation, or least-privilege requirements justify
them. Principals established by different providers must not be merged merely
because their subject strings match. Cross-provider account linking,
delegation, impersonation, or user-principal pass-through requires a separate
accepted contract.

## Validation And Configuration Contract

Implementation must preserve these rules:

- Only certificate evidence already validated by the configured mTLS trust
  roots may reach the mapping provider.
- The provider fingerprints the validated leaf certificate's DER bytes; it
  does not interpret a certificate common name or SAN as application identity.
- Fingerprint comparison and configuration parsing are exact and
  deterministic.
- Mapping configuration is validated as a complete unit before becoming
  active. Invalid replacement configuration leaves the previous valid
  configuration active.
- The provider re-evaluates the peer certificate fingerprint against the
  currently active immutable mapping snapshot on every RPC. Authentication is
  request-scoped; a successful result is not cached as connection, principal,
  workspace-handle, or application-session authority.
- Removing a mapping prevents that credential from establishing a principal
  for every RPC that begins after the replacement configuration becomes
  active, including RPCs on an already established TLS connection. An RPC that
  already captured the prior mapping version may complete under that version;
  mappings are not revoked midway through a request.
- Workspace handles do not retain authentication. Execute and close requests
  reauthenticate, so a handle obtained before mapping removal cannot be used by
  that credential afterward.
- Certificate and trust-root replacement remain configuration operations, not
  code changes.
- Stable public authentication failures do not disclose whether a fingerprint
  or principal exists in the mapping table.

Per-request mapping evaluation defines application-principal revocation, not
mid-connection X.509 path or certificate-validity revalidation. The
implementation slice must specify the configuration format, ownership, atomic
reload behavior, certificate-chain extraction contract, operational rotation
procedure, and a bounded TLS connection lifetime or equivalent revalidation
mechanism for trust-root, certificate-expiry, and certificate-revocation
changes before this provider is described as production-capable.

## Required Public-Boundary Proof

Shared gRPC service tests must prove:

- a certificate signed by the configured client CA and present in the mapping
  establishes the configured principal through a real mTLS connection;
- trusted but unmapped, missing, malformed, and untrusted certificate evidence
  fails authentication;
- a mapped principal still requires an explicit authorization permission;
- client actor metadata cannot replace or modify the mapped principal;
- overlapping fingerprints can map to one principal during rotation;
- removing a mapping causes the next RPC on the same established TLS channel,
  including execute or close using an existing workspace handle, to fail
  authentication while an already-started RPC retains its captured mapping
  version;
- duplicate or invalid mapping configuration fails closed; and
- public errors and observable logs do not expose certificate bodies, private
  keys, fingerprints, or mapping contents.

Authentication and mapping behavior must be tested through the public service
boundary. Private unit tests may supplement that proof for fingerprint
normalization and configuration validation helpers.

## Non-Goals

- This decision does not make fingerprints canonical principal identifiers.
- It does not require mTLS for every future access pattern.
- It does not define OIDC, OAuth, PKCE, SPIFFE federation, bearer tokens,
  workload identity federation, account linking, delegation, or user-principal
  pass-through.
- It does not define certificate issuance, a general PKI lifecycle, or hosted
  identity management.
- It does not grant workspace permission based on certificate trust or mapping
  alone.
- It does not define the workspace action/resource/permission taxonomy.
- It does not constitute a hosted-service or production-security claim.

## Consequences

Positive consequences:

- The first provider matches the controlled service-principal use case with a
  narrow and inspectable trust configuration.
- Certificate rotation does not require changing canonical principal identity.
- Transport trust, application authentication, actor assertions, and
  authorization remain distinct.
- Future authentication mechanisms can reuse the canonical principal and
  authorization boundaries.

Tradeoffs:

- Leaf-certificate rotation requires explicit mapping overlap and removal.
- A large population of certificates would make fingerprint allow-listing
  operationally cumbersome.
- Exact fingerprints deliberately avoid the scaling advantages of stable
  workload or user identity claims.
- Revocation and configuration replacement procedures become part of secured
  service operations.

## Relationship To Existing Decisions

This decision specializes the accepted security decision to keep mTLS
transport identity distinct from application identity. It also preserves the
minimal server-owned security context, default-deny authorization, the
workspace-scoped canonical enforcement path, and the rule that security claims
require implementation plus public tests.

It resolves the first-provider choice only. It does not close the broader
question of which authentication providers GRM should support over time.
