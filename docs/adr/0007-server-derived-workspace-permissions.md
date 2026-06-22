# ADR 0007: Authorize Exact Server-Derived Workspace Permissions

Status: Accepted

Date: 2026-06-22

## Context

GRM's Phase 1 security proof constructs trusted internal operations from typed
workspace requests. It distinguishes workspace lifecycle, schema, node, edge,
batch, query, traversal, explain, profile, index, and workspace-inspection
actions and binds them to server-derived resources. Public gRPC tests prove
default denial, fail-closed policy errors, authorization before effects,
contained-operation batch classification, and underlying resource
classification for query and traversal wrappers.

The implementation does not yet include a real permission table, durable
policy configuration, or policy versioning. Its authorization policy is an
interface with anonymous-local, default-deny, and test-only implementations.
Some operation classifications are also too weak for a real policy: save,
load, export, and import currently share `WorkspaceInspect` rather than
distinct durability or data-movement actions.

The first policy must be deterministic and small enough to prove through the
public service boundary. It must work for any canonical authenticated
principal, rather than assuming that authorization applies only to the current
service-principal use case. At the same time, it must not accidentally become a
general policy language or freeze deployment-local role names into GRM's
security contract.

## Decision

GRM will standardize an initial workspace action, resource, and permission
taxonomy. The first policy implementation will be an exact deterministic table
over canonical authenticated principals and server-derived operation scope.

The authorization pipeline is:

```text
authentication evidence
  -> canonical authenticated principal
  -> server-derived workspace, action, and resource
  -> exact permission evaluation
  -> authorization decision
```

Canonical authenticated principals may represent workloads, services, or
users. Authentication method does not implicitly grant a role or permission.
The policy receives the canonical principal established by the authentication
system and does not derive authority from a transport peer, client actor
assertion, credential type, or adapter-supplied label.

Authorization decisions are evaluated against the authenticated principal
presented to the authorization system. Future delegated or end-user
pass-through models require explicit contracts describing how user and service
identities are represented, audited, and authorized. A service principal must
not silently substitute for an authenticated end user, and a client-supplied
actor assertion must not substitute for either.

## Permission Contract

An initial permission is the conjunction of:

```text
Permission = Action + ResourceSelector
```

Policy assignment separately binds that permission to a canonical principal,
service or stable workspace scope, and explicit policy version. Workspace
creation is the one initial lifecycle action evaluated only against service
scope because no server-allocated workspace identity exists yet. All parts of
an operation must match. Missing permissions, missing policy, unsupported
classifications, ambiguous scope, policy load errors, and policy evaluation
errors deny access.

The service, not the client or adapter, derives every effective action,
resource, workspace, and contained operation. The authorization provider may
return a decision and bounded reason code; it must not rewrite the operation or
manufacture trusted scope from client metadata.

Deployment-local named roles may bundle explicit permissions for
configuration convenience. Role names and bundles are not canonical GRM
semantics and do not independently grant authority. The evaluated authority is
the expanded, versioned permission set.

## Initial Actions

The initial taxonomy distinguishes:

- workspace create, open, close, and inspect;
- schema inspect and define;
- node create, read, update, and delete;
- edge create, read, update, and delete;
- batch apply;
- query and traversal;
- explain and profile;
- index-catalog inspect;
- workspace save, load, export, and import; and
- reserved identity, policy, audit, key, and durability administration.

Workspace save, load, export, and import are distinct actions. Read-like
`WorkspaceInspect` authority is insufficient for data export, durable state
replacement, or import. Reserved administrative actions remain denied and
unsupported until their public operations and stronger authorization contracts
are separately accepted and implemented.

`WorkspaceCreate` requires an explicit service-scoped create permission before
the service allocates workspace state or a stable workspace identity. A
client-requested name, ID, mode, or the synthetic placeholder
`new-workspace` is not an authorization resource and cannot grant or select
authority. After authorization, the service creates the workspace and returns
its stable identity and managed handle.

`WorkspaceOpen` is workspace-scoped. It requires a syntactically valid stable
opaque workspace or snapshot reference, uses that canonical requested reference
as policy scope without accepting a server-local path, and checks existence
only after authorization so denied callers cannot distinguish inaccessible
from missing workspaces. Execute and close resolve their stable workspace scope
from the managed handle and reauthorize on every RPC.

Batch authorization requires permission for `BatchApply` and for every
contained operation. Query, explain, profile, and traversal authorization
requires permission for the wrapper action and all server-derived underlying
node, edge, root, destination, and traversal resources. A wrapper permission
never substitutes for underlying data access.

## Initial Resources And Selectors

Initial resource kinds are:

- service;
- stable workspace;
- node model;
- edge or link model;
- operation family;
- index catalog;
- workspace artifact; and
- reserved administrative resource.

Selectors may identify an exact service category, stable workspace, model, or
operation category, or use an explicit bounded wildcard within an already
authorized workspace. Workspace handles and client-supplied filesystem paths
are not stable policy resources.

The initial table does not evaluate graph instance IDs, property values,
ownership attributes, tenant attributes, arbitrary expressions, or other
data-level predicates. This is a boundary on the first implementation, not a
permanent exclusion from GRM's authorization architecture.

Future ownership, tenancy, relationship-based, attribute-based, or
finer-grained authorization models may extend the resource and selector
contract. Such extensions require separate accepted decisions covering
server-derived trusted inputs, evaluation bounds, consistency and policy
versioning, existence and timing leakage, administrative authority, audit
semantics, and public negative tests.

## Decision And Failure Semantics

The first permission table must provide:

- default deny;
- an explicit policy version included in the trusted decision context;
- deterministic allow and deny results with bounded reason codes;
- fail-closed policy loading and evaluation;
- exact authorization of every contained or underlying operation;
- stricter handling of durability, data-movement, and future administrative
  actions; and
- stable public errors that do not reveal inaccessible workspaces, models,
  principals, or policy contents.

Authorization remains separate from runtime validation, transaction safety,
delete controls, backend capability checks, and durability outcomes. Permission
to attempt an operation does not guarantee that the runtime will accept or
commit it.

## Required Public-Boundary Proof

Public gRPC service integration tests must prove:

- allow and deny behavior for every implemented action family;
- the same permission semantics for canonical workload, service, and user test
  principals;
- wrong-principal and wrong-workspace denial;
- workspace create requires service-scoped authority, ignores client-requested
  IDs as permission scope, and performs no allocation when denied;
- workspace open uses a stable opaque workspace/snapshot scope and preserves
  missing-versus-inaccessible masking;
- exact node-model and edge-model selectors;
- batch wrapper and every contained operation are authorized;
- query, traversal, explain, and profile wrappers retain underlying resource
  authorization;
- save, load, export, and import use distinct stronger actions;
- missing permission, missing policy, unsupported classification, load failure,
  and evaluation failure deny before effects;
- asserted actor metadata and authentication method cannot grant permission;
- denied execute and close operations do not disclose handle existence; and
- authorized operations still pass through runtime validation and durability
  checks.

Private tests may supplement the public proof for deterministic table
expansion, wildcard matching, configuration validation, and policy-version
selection.

## Non-Goals

- No general policy language or arbitrary policy expressions.
- No universal or hard-coded role semantics.
- No assumption that permissions primarily apply to service accounts.
- No permission derived from authentication method, mTLS trust, actor
  assertion, or adapter metadata.
- No hosted tenancy, workspace ownership, membership, delegation, or
  end-user pass-through contract.
- No graph-instance, row, property, ownership, relationship, or
  attribute-based policy in the initial table.
- No permanent prohibition on those finer-grained models when separately
  designed and accepted.
- No implementation of administrative RPCs, broad admission limits,
  encryption at rest, audit receipts, or attestation.
- No production-security or hosted-isolation claim.

## Consequences

Positive consequences:

- Service, workload, and future user principals share one canonical
  authorization boundary.
- Policy behavior is deterministic, inspectable, versionable, and suitable for
  exhaustive public negative tests.
- Server-derived classification prevents adapters and clients from selecting a
  weaker permission than the operation they perform.
- Deployment-local roles remain convenient without becoming permanent product
  semantics.
- The bounded first table leaves an explicit path to future ownership,
  tenancy, relationship, or attribute-aware authorization.

Tradeoffs:

- Coarse model and operation selectors cannot express user-level data ownership
  or fine-grained sharing requirements.
- Exact wrapper and contained-operation checks require careful classification
  whenever the typed runtime operation set changes.
- Policy configuration and version replacement need their own safe loading and
  administration design.
- Service-scoped create authority is deliberately coarser than future
  namespace, quota, ownership, or tenant-aware workspace provisioning policy.
- Stable opaque workspace identity must be available for open decisions
  without trusting handles or server-local paths.

## Relationship To Existing Decisions

This decision builds on ADR 0006's provider-independent canonical principal
boundary. It preserves the accepted decisions to default secured policy to
deny, treat actor identifiers as assertions, use server-owned security context,
authorize through the canonical workspace path, require explicit secured
traversal edge models, and retain runtime safety after authorization.

It resolves the first workspace permission taxonomy. It does not resolve
future delegation, user pass-through, tenancy, ownership, or finer-grained
authorization.
