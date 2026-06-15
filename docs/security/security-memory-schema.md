# SOML Security Memory Schema

Status: Proposed

Date: 2026-06-15

## Purpose

This document defines the security-specific graph vocabulary used to reason
about and develop the SOML Security layer in GRM.

The schema complements the generic project-memory models such as `Risk`,
`Constraint`, `Decision`, and `OpenQuestion`. Security concepts use dedicated
models because they carry distinct trust, evidence, mitigation, verification,
and traceability meaning.

The schema records design intent and implementation evidence. The existence of
a node does not imply that a control or security guarantee is implemented.

## Node Models

### TrustBoundary

A point where data, identity, authority, or execution crosses between trust
domains.

Fields:

- `name`: stable boundary name.
- `summary`: what crosses the boundary and why it matters.
- `trustAssumption`: the assumption currently made about the destination side.
- `deploymentScope`: embedded, local service, secured service, hosted, or
  another explicit scope.
- `status`: lifecycle state of the boundary definition.

Examples include adapter, transport, service, runtime, backend, storage, key
provider, and client-verification boundaries.

### IdentityKind

A security identity concept with explicit provenance.

Fields:

- `name`: identity concept name.
- `summary`: meaning and permitted use.
- `proofSource`: evidence from which the identity is established.
- `status`: lifecycle state.

Examples include transport peer, authenticated principal, asserted actor,
delegated actor, anonymous local caller, service identity, and administrator.

An `IdentityKind` describes a class of identity, not an individual person,
certificate, token, or credential.

### SecurityControl

A preventive, detective, corrective, or recovery mechanism.

Fields:

- `title`: control name.
- `summary`: behavior and intended guarantee.
- `controlType`: preventive, detective, corrective, recovery, or compensating.
- `layer`: transport, service, runtime, backend, storage, client, supply chain,
  or operations.
- `status`: proposed, planned, implemented, tested, or another explicit
  lifecycle state.

Controls should make implementation truth visible. A proposed control must not
be presented as an implemented guarantee.

### Threat

An attacker action or failure mode against protected SOML assets.

Fields:

- `title`: concise threat name.
- `summary`: attack or failure scenario.
- `category`: identity, authorization, isolation, disclosure, integrity,
  availability, audit, replay, malicious service, supply chain, or another
  explicit category.
- `severity`: current qualitative severity.
- `status`: active, mitigated, accepted, deferred, or another explicit state.

`Threat` is distinct from generic `Risk`: a threat describes the hostile or
adverse scenario, while a risk may additionally capture likelihood, delivery
impact, sequencing, or project exposure.

### SecurityRequirement

A verifiable security property that the design or implementation must satisfy.

Fields:

- `title`: requirement name.
- `summary`: normative security property.
- `priority`: required, recommended, future, or another explicit priority.
- `verification`: evidence needed to demonstrate satisfaction.
- `status`: proposed, accepted, implemented, verified, or another explicit
  lifecycle state.

Requirements should be testable or otherwise evidence-bearing. Broad intentions
without a verification route belong in design prose rather than this model.

### SecurityOpenQuestion

An unresolved security choice whose answer changes guarantees, architecture, or
implementation.

Fields:

- `question`: the unresolved question.
- `impact`: why the answer matters.
- `decisionNeededBy`: phase, work slice, or milestone requiring resolution.
- `status`: open, resolved, deferred, or another explicit state.

### SecurityDecision

An accepted, rejected, or superseded security-specific design choice.

Fields:

- `title`: decision name.
- `summary`: selected direction.
- `status`: proposed, accepted, rejected, or superseded.
- `date`: decision date.
- `rationale`: security and architecture reasoning.

Security decisions remain separate from controls. A decision selects or rejects
a direction; a control is the mechanism eventually implemented and verified.

## Relationship Models

| Relationship | From | To | Meaning |
| --- | --- | --- | --- |
| `THREAT_TARGETS_BOUNDARY` | `Threat` | `TrustBoundary` | The threat exploits or crosses this boundary. |
| `IDENTITY_ESTABLISHED_AT` | `IdentityKind` | `TrustBoundary` | Trusted evidence for this identity is established at this boundary. |
| `CONTROL_ESTABLISHES_IDENTITY` | `SecurityControl` | `IdentityKind` | The control authenticates, maps, or validates this identity kind. |
| `CONTROL_APPLIES_TO_BOUNDARY` | `SecurityControl` | `TrustBoundary` | The control is enforced at this boundary. |
| `CONTROL_MITIGATES_THREAT` | `SecurityControl` | `Threat` | The control reduces or detects this threat. |
| `REQUIREMENT_ADDRESSES_THREAT` | `SecurityRequirement` | `Threat` | The requirement exists to address this threat. |
| `REQUIREMENT_REQUIRES_CONTROL` | `SecurityRequirement` | `SecurityControl` | The named control contributes to satisfying the requirement. |
| `REQUIREMENT_APPLIES_TO_BOUNDARY` | `SecurityRequirement` | `TrustBoundary` | The requirement is enforced or verified at this boundary. |
| `QUESTION_CONCERNS_REQUIREMENT` | `SecurityOpenQuestion` | `SecurityRequirement` | The unresolved choice affects this requirement. |
| `DECISION_RESOLVES_SECURITY_QUESTION` | `SecurityDecision` | `SecurityOpenQuestion` | The decision resolves or closes the question. |
| `DECISION_SATISFIES_SECURITY_REQUIREMENT` | `SecurityDecision` | `SecurityRequirement` | The selected direction defines how the requirement will be met. |
| `DECISION_SELECTS_SECURITY_CONTROL` | `SecurityDecision` | `SecurityControl` | The decision selects, constrains, or rejects a control. |

Each relationship may carry an optional `reason` field explaining the specific
connection.

## Intended Traversals

The schema should support questions such as:

- Which threats cross the transport or storage boundary?
- Which controls mitigate a malicious-service threat?
- Which requirements remain unsupported by an implemented and tested control?
- Where is authenticated principal identity established?
- Which open questions block a security requirement?
- Which decision selected a control, and what requirement justified it?
- Which controls are proposed but not yet verified?

## Modeling Rules

1. Do not represent a client-supplied actor label as an authenticated
   principal.
2. Do not mark a requirement verified solely because a design document exists.
3. Keep transport controls distinct from application authentication and
   authorization controls.
4. Keep preventive controls distinct from audit, attestation, and
   client-verification evidence.
5. Record deployment scope in summaries and boundaries so local guarantees do
   not silently become hosted claims.
6. Prefer one clear relationship over duplicating the same meaning through
   generic `Risk`, `Decision`, or `OpenQuestion` edges.
7. Link implementation status to tested evidence in the summary or verification
   field until a dedicated security-evidence model is justified.

## Initial Scope

The initial schema deliberately does not add models for credentials, keys,
policies, permissions, audit events, attestations, receipts, or evidence
artifacts. Those are likely runtime or product-domain concepts rather than
development-memory concepts.

Add them only when concrete implementation and query needs demonstrate that a
dedicated model is more useful than a `SecurityControl` or
`SecurityRequirement`.
