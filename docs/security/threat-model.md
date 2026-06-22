# GRM Threat Model

Status: Proposed

Date: 2026-06-15

## Purpose

This document establishes the initial threat model for GRM security design.
It describes what GRM needs to protect, which boundaries are trusted, what an
attacker may control, and which security properties future implementations must
preserve.

It is a design baseline, not a claim that GRM currently provides production
authentication, authorization, tenant isolation, durable audit, or hosted
security.

## Security Objective

GRM should provide typed, secure, explainable operational memory for
applications, agents, and people.

Security must protect both:

- the confidentiality, integrity, and availability of operational memory; and
- the meaning and provenance of requests, decisions, and evidence used to
  access or change that memory.

Typed operations make requests easier to validate and authorize, but typed
messages are not trustworthy merely because they are well formed.

## Current Deployment Classes

### Embedded Local

The caller and GRM runtime execute in one process. The host process and operating
system account are inside the primary trust boundary.

GRM does not currently protect an embedded workspace from malicious code running
inside the same process or with equivalent filesystem access.

### Local Service

Clients communicate with a GRM workspace service over gRPC. The service owns
workspace handles and maps opaque workspace references to server-local storage.

The current service supports insecure local transport, server-authenticated TLS,
and optional mutual TLS. TLS protects the connection and mTLS authenticates a
transport peer. Neither currently establishes application actor identity or
authorization.

When certificate validation, hostname verification, trust roots, and private
keys are correctly managed, TLS protects confidentiality and integrity in
transit and resists ordinary network man-in-the-middle attacks. Mutual TLS adds
authentication of the client transport peer.

TLS and mTLS do not protect a client from:

- a compromised or malicious GRM service;
- a compromised trusted certificate authority;
- stolen server or client private keys;
- a compromised client or server process; or
- inaccurate data produced by trusted server software or its backend.

### Future Hosted Service

A future deployment may serve multiple users, agents, applications, or tenants
across an untrusted network. It will require stronger identity, isolation,
policy, audit, lifecycle, and availability guarantees than current local modes.

No hosted-security guarantee should be inferred from the local service.

## Protected Assets

GRM security work must consider:

- user graph data;
- runtime schema and declared/inferred schema provenance;
- schema-memory orientation and workspace catalog metadata;
- durable operation logs, checkpoints, and recovery metadata;
- workspace references and active workspace handles;
- authorization policy and policy versions;
- authenticated principal and delegated actor context;
- audit and future attestation evidence;
- TLS private keys, client credentials, tokens, and other secrets;
- encryption-at-rest keys and key-wrapping metadata;
- signed state commitments, receipts, and trusted client checkpoints;
- query, traversal, explain, and profile results;
- service availability, memory, CPU, storage, and concurrency capacity;
- the integrity of generated protobuf contracts and client libraries.

## Security Actors

The security model must distinguish these concepts:

- **Transport peer**: the process or endpoint authenticated by the transport,
  such as an mTLS client certificate.
- **Authenticated principal**: the identity established by a trusted
  authentication mechanism.
- **Actor**: the human, agent, application, or service performing an operation.
- **Delegated actor**: an actor operating through another authenticated
  principal under an explicit delegation.
- **Anonymous local caller**: a compatibility identity for explicitly local,
  permissive development modes only.
- **GRM service**: the trusted enforcement point that resolves workspace scope,
  validates requests, applies policy, executes operations, and emits evidence.
- **Administrator**: a principal allowed to configure service, workspace,
  identity, policy, audit, or durability resources.
- **Backend operator**: a principal or process controlling an external storage
  backend such as Neo4j.

An actor identifier supplied by a client is an assertion, not authenticated
identity. It must not independently authorize access.

## Trust Boundaries

### Adapter Boundary

CLI, Python, MCP, Rust clients, generated SDKs, and future HTTP/UI adapters are
outside the trusted service boundary.

Adapters may parse convenient syntax and supply context, but the service must
not trust them to declare:

- effective permissions;
- resolved workspace scope;
- operation classification;
- authenticated identity;
- policy decisions; or
- audit outcomes.

### Transport Boundary

The network and intermediaries are untrusted.

TLS may provide server authentication and confidentiality. Mutual TLS may also
authenticate a transport peer. Certificate identity must remain distinct from
application actor identity until an explicit, tested mapping policy exists.

Clients must validate the expected server identity and trust root. Disabling
certificate or hostname verification removes the man-in-the-middle protection
that TLS is intended to provide.

### Service Boundary

The GRM service is the primary enforcement point for service-backed workspaces.
It must validate typed requests, resolve managed resources, authenticate
principals, authorize operations, apply limits, execute runtime behavior, and
emit evidence in a defined order.

Direct RPC aliases, future gateways, and streaming APIs must not bypass this
enforcement path.

### Runtime Boundary

The runtime may trust that a service request has passed service-level
authentication and authorization only when that contract is explicit.

Runtime invariants, schema validation, transaction safety, delete controls, and
durability rules remain mandatory even for authorized requests.

### Backend And Storage Boundary

External backends, workspace files, mounted volumes, backups, and operator
tools may be read, modified, replaced, or deleted outside GRM.

GRM must not assume backend data is confidential, intact, or schema-consistent
unless the deployment and storage controls establish those properties.

Workspace storage, WAL records, checkpoints, backups, catalog metadata, and
external-backend credentials should support encryption at rest. Encryption at
rest must include an explicit key-management model: where keys originate, which
principal may unwrap them, how they are rotated and revoked, and whether the
service host can access plaintext.

Disk or volume encryption protects lost media and offline copies, but normally
does not protect data from a compromised service process that can use the
decryption key. Application- or field-level encryption can reduce that trust,
but limits server-side validation, traversal, indexing, explain/profile, and
query capability unless specialised searchable-encryption designs are adopted.

## Attacker Capabilities

The threat model assumes an attacker may:

- send arbitrary protobuf messages and malformed field combinations;
- omit, forge, replay, or duplicate request and actor identifiers;
- claim another actor identity;
- obtain or guess a workspace reference or handle;
- issue high-cost traversals, batches, profile requests, or large responses;
- open many connections or retain workspace handles;
- observe error messages, timing, logs, and response sizes;
- exploit differences between CLI, Python, MCP, Rust, and direct gRPC clients;
- compromise an adapter without compromising the GRM service;
- possess a valid transport credential without being authorized for a
  workspace;
- operate a compromised or deliberately malicious GRM service that returns
  stale, fabricated, incomplete, or equivocated results;
- alter local workspace files or external backend data when they have host or
  backend access;
- cause process termination, partial writes, retries, and network interruption;
- submit graph values intended to leak through logs, audit records, errors, or
  telemetry;
- exploit dependency, build, release, or generated-client supply chains.

For local embedded mode, malicious code inside the same process or operating
system account is generally outside the protection boundary.

## Primary Threats

### Identity Spoofing

A client claims a privileged actor identity without proving control of that
identity.

Required direction:

- authorization must use an authenticated principal;
- asserted actor identity must be labelled and treated as untrusted;
- delegation must be explicit, bounded, and auditable.

### Workspace Confusion And Cross-Workspace Access

A request is executed against a workspace the principal should not access, or a
stale/guessed handle is reused across clients.

Required direction:

- workspace scope is resolved server-side;
- handle ownership and lifecycle semantics are defined;
- authorization binds principal, action, and workspace;
- cross-workspace negative tests are mandatory.

### Privilege Escalation Through Operation Classification

A request is authorized as a low-risk family but performs a stronger action,
especially through batch, traversal, profile, admin, import, or future direct
RPC aliases.

Required direction:

- operation and resource classification is server-derived;
- batch authorization accounts for contained operations and delete controls;
- aliases route through the same canonical enforcement path;
- deny and policy-error precedence are defined.

### Policy Bypass Or Fail-Open Behaviour

Missing policy, policy evaluation failure, cache inconsistency, or unsupported
operation silently results in access.

Required direction:

- production/service policy defaults must be explicit;
- policy evaluation errors fail closed;
- local permissive modes are deliberate, visible, and unsuitable for hosted
  claims;
- policy version and decision reason are available to evidence.

### Sensitive Data Disclosure

Graph values, schema names, credentials, identifiers, policy details, or
private paths leak through responses, errors, logs, audit events, explain/profile
output, or telemetry.

Required direction:

- logs and telemetry avoid graph values by default;
- error messages are useful without revealing protected data;
- redaction rules apply consistently to audit and observability;
- server-local filesystem paths never enter public service contracts.

### Audit Evasion Or Misrepresentation

Denied, failed, or successful operations are absent, reordered, incorrectly
classified, or recorded with attacker-controlled identity. Runtime success may
also differ from client-visible delivery success.

Required direction:

- audit stages and outcome semantics are defined;
- authenticated principal, asserted actor, policy decision, runtime outcome,
  and response-delivery outcome remain distinguishable;
- audit retention is bounded and uses an explicit sink contract;
- future durable audit needs integrity, ordering, availability, and redaction
  guarantees.

### Resource Exhaustion

An attacker consumes memory, CPU, disk, handles, connections, audit capacity, or
backend resources.

Required direction:

- bound request and response sizes;
- limit batch size, traversal depth, result count, and profile cost;
- define deadlines, cancellation, concurrency, and rate limits;
- expire or reclaim stale workspace handles;
- never retain unbounded process-local audit history.

### Replay And Duplicate Mutation

A captured or retried request is executed more than once.

Required direction:

- define request-ID ownership and uniqueness;
- decide which mutations support idempotency keys;
- distinguish transport retries from application retries;
- include replay decisions in audit evidence.

### Backend Or Workspace Tampering

Workspace files, logs, checkpoints, schema memory, or external graph data are
modified outside GRM.

Required direction:

- load-time consistency and drift checks remain explicit;
- recovery failures must not silently become trusted state;
- future integrity evidence should distinguish corruption detection from
  attestation or non-repudiation;
- backend capability and trust differences remain visible.

### Malicious Or Compromised Service

An authenticated service returns fabricated, stale, selectively omitted, or
internally inconsistent data; acknowledges a mutation that was not durably
committed; presents different histories to different clients; or supplies false
explain, profile, policy, audit, or attestation evidence.

TLS cannot detect this because TLS authenticates and protects the connection to
the server; it does not prove that the server executed GRM semantics honestly.

Required direction:

- mutation responses can carry signed receipts binding request digest,
  workspace identity, previous state commitment, resulting state commitment,
  operation outcome, policy decision, and service identity;
- durable workspace history can use hash-linked deltas or Merkle commitments so
  clients can detect modification, omission, reordering, and rollback relative
  to a previously trusted checkpoint;
- reads and traversals can eventually provide inclusion, absence, or execution
  proofs where the selected data structure and performance envelope allow it;
- clients retain trusted state commitments and reject unexpected rollback or
  history forks;
- independent transparency logs, witnesses, replicas, or quorum comparison may
  detect a service equivocating between clients;
- signed software provenance and remote-attestation evidence may establish
  which measured service code is running in a confidential-computing
  environment;
- clients still validate schema, response shape, capability declarations,
  monotonic versions, request/response binding, and durable-operation receipts.

These mechanisms provide different guarantees and should not be conflated:

- a signature proves which key signed a statement, not that the statement is
  true;
- a hash chain proves consistency with a prior trusted commitment, not that the
  original input was correct;
- remote attestation provides evidence about measured software and environment,
  not freedom from software bugs or malicious upstream data;
- independent witnesses or replicas reduce single-service trust but introduce
  consensus, availability, and governance costs;
- fully end-to-end encrypted graph data protects confidentiality from the
  service but prevents ordinary server-side graph processing over plaintext.

### Supply-Chain Compromise

Published crates, Python wheels, Docker images, generated protobuf clients,
dependencies, or CI workflows are replaced or modified.

Required direction:

- retain provenance and artifact verification;
- minimize release credentials through trusted publishing;
- review security-sensitive dependency and workflow changes;
- define future signing and vulnerability-response expectations.

## Security Principles

Future security design should follow these principles:

1. Authenticate before authorizing.
2. Treat client-supplied identity as an assertion until bound to trusted
   authentication.
3. Resolve workspace and operation scope server-side.
4. Deny before execution and fail closed on policy errors.
5. Keep authorization separate from schema validation, transaction safety, and
   durability invariants.
6. Use one canonical enforcement path for all service adapters and aliases.
7. Make local permissive modes explicit and visibly weaker.
8. Bound all attacker-controlled resource consumption.
9. Avoid sensitive graph data in logs, errors, audit, and telemetry by default.
10. Distinguish audit evidence from provenance, attestation, and
    non-repudiation.
11. Preserve security-relevant outcomes across retries, failures, and recovery.
12. Test denial and isolation through public service surfaces.
13. Encrypt durable workspace state and backups at rest under an explicit key
    management and rotation model.
14. Design client-verifiable state and operation evidence so trust in TLS does
    not become unconditional trust in service correctness.

## Current Controls

Implemented and tested controls currently include:

- typed protobuf workspace operations;
- server-managed workspace handles and opaque workspace references;
- no client-supplied server-local filesystem paths in public admin contracts;
- trusted internal security-context types that distinguish transport peer,
  authenticated principal, asserted actor, delegated actor, server-resolved
  workspace/action/resource, and authorization decision;
- canonical authentication and authorization for workspace create, open,
  execute, and close, with pre-runtime enforcement around `ExecuteWorkspace`;
- an explicit anonymous-local compatibility profile;
- service constructors that require explicit security-profile selection rather
  than defaulting to anonymous-local;
- secured-profile unauthenticated, default-deny, policy-error, and batch-limit
  outcomes;
- server-derived classification of every contained batch operation;
- query, explain, and profile classification that preserves both wrapper action
  and underlying node, edge, or traversal access;
- traversal classification that includes the root node model, each selected
  edge model, and each destination node model for query and direct find
  requests;
- secured-profile rejection of implicit-edge traversal when the concrete edge
  resource cannot be derived before authorization;
- public service tests proving actor assertions and mTLS peer identity do not
  independently authorize access, and denied requests do not mutate state;
- public service tests proving denied execute and close requests do not reveal
  whether a workspace handle exists;
- server-authenticated TLS;
- optional mutual TLS requiring a certificate signed by the configured client
  CA;
- service-boundary tests for trusted, missing, and untrusted client
  certificates;
- explicit unsupported direct RPC families;
- runtime schema validation, transaction behavior, and delete controls;
- logging tests intended to avoid graph values and obvious secrets;
- artifact provenance and verification for selected release paths.

These controls do not yet constitute application authentication,
authorization, tenant isolation, production audit, or hosted security.
They also do not provide encryption at rest or client-verifiable proof that an
authenticated service returned complete and accurate state.

## Known Gaps

GRM has accepted but not yet implemented:

- [ADR 0006](../adr/0006-mtls-certificate-mapping-authentication-provider.md),
  which selects explicit validated mTLS leaf-certificate fingerprint mapping
  as the first controlled service-principal authentication provider while
  preserving a provider-independent canonical principal boundary;
- [ADR 0007](../adr/0007-server-derived-workspace-permissions.md), which
  defines an exact server-derived workspace action/resource permission table
  for canonical workload, service, or user principals without foreclosing
  separately designed finer-grained authorization; and
- [ADR 0008](../adr/0008-bounded-authoritative-security-audit.md), which
  defines versioned principal-centric audit events, a bounded local
  authoritative sink, and explicit anonymous-local, secured-profile, and
  future high-assurance/regulated audit postures.

Design acceptance is not implementation or verification. GRM still does not
implement:

- a production credential mechanism for authenticated application principal
  resolution;
- authenticated delegation semantics;
- the accepted certificate-to-principal mapping provider or any
  token-to-principal provider;
- workspace ownership, membership, or tenant isolation;
- the accepted exact permission table, policy storage, or safe policy-version
  replacement;
- policy administration or recovery semantics;
- bounded request, traversal, result, profile, concurrency, or broad admission
  policy beyond the current secured-profile batch count;
- the accepted bounded authoritative audit sink, external authoritative audit,
  or tamper-evident audit storage;
- request replay and idempotency semantics;
- production secret and certificate lifecycle;
- encryption-at-rest guarantees;
- encryption key generation, wrapping, rotation, revocation, recovery, and
  separation-of-duties semantics;
- signed mutation receipts or state commitments;
- hash-linked or Merkle-verifiable workspace history;
- rollback, omission, fork, and equivocation detection;
- independent witnesses, transparency logs, or replica comparison;
- remote-attestation semantics for measured service deployments;
- client-held trusted checkpoints and verification APIs;
- cross-workspace isolation tests;
- a security incident and vulnerability-response process;
- a dependency and supply-chain security policy;
- security semantics for external backends;
- a boundary between authorization evidence and future attestation.

## Required Next Decisions

The first authentication provider, permission taxonomy, and bounded audit
contract are resolved by ADR 0006, ADR 0007, and ADR 0008. Remaining decisions
include:

1. Which additional identity mechanisms are supported beyond the first
   controlled mTLS provider for local service and future hosted deployments?
2. Which bounded canonical-principal attributes and authentication-provider
   provenance must cross internal boundaries beyond the accepted
   `(issuer, subject)` principal identifier?
3. How are asserted actors and delegated actors represented without enabling
   impersonation?
4. Which ownership, tenancy, relationship, attribute, or other finer-grained
   authorization dimensions are justified after the exact initial table?
5. How are policy configuration, version replacement, and administration
   authorized and recovered safely?
6. Which limits are mandatory before a service can bind beyond loopback?
7. What negative security tests are release-blocking beyond the explicit tests
   attached to the three Phase 2 implementation slices?
8. When does a deployment require an external authoritative audit sink or a
   stronger high-assurance/regulated posture?
9. Which workspace artifacts must be encrypted at rest, and who is allowed to
   possess or unwrap each key?
10. Is host-transparent disk encryption sufficient for the first secured
   deployment, or must selected fields/workspaces be encrypted above the
   service host?
11. What is the first client-verifiable commitment: signed mutation receipts,
    hash-linked WAL/checkpoints, Merkle workspace roots, or another design?
12. Where does the client retain its last trusted workspace commitment, and how
    are rollback and fork warnings handled?
13. Does GRM require independent witnesses or remote attestation for any
    claimed zero-trust deployment class?

## Review Triggers

This threat model must be reviewed when GRM adds or changes:

- identity or credential handling;
- authorization or policy semantics;
- public service operations or gateways;
- workspace ownership, sharing, or tenancy;
- audit, telemetry, or attestation;
- import, export, backup, restore, or migration;
- backend trust assumptions;
- secret or sensitive-data types;
- encryption-at-rest or key-management design;
- signed receipts, state commitments, transparency logs, witnesses, or remote
  attestation;
- hosted deployment claims;
- release signing, provenance, or distribution channels.
