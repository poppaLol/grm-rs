# ADR 0013: Establish SOML Primitive Value Types

Status: Accepted

Date: 2026-09-21

## Context

GRM currently exposes four schema field and graph property value types across
its runtime and service boundary:

- `string`
- `int`
- `float`
- `bool`

That set is enough for basic graph CRUD, but it is too small for operational
memory. Evidence, policy, audit, finance, identity, retention, and integration
work all need values whose meaning cannot safely be reconstructed from an
unconstrained string or approximate number.

The exploratory SOML notes
[Primitive Types and Generics](../soml/data-type-primitives.md) and
[Graph-Native Types and Affordances](../soml/graph-native-types.md) describe a
larger future vocabulary. This ADR preserves that direction while choosing a
smaller primitive foundation that can be implemented consistently across the
runtime, durable workspace, protobuf service, CLI, Python, MCP, and supported
backends.

GRM also needs to avoid two traps:

- allowing each adapter or backend to invent different value semantics; and
- expanding the first implementation into a programming-language type system.

## Decision

GRM will define a canonical SOML primitive value layer beneath schema fields,
property values, predicates, ordering, persistence, and typed service requests.
The canonical semantics belong to GRM. Backend representations and adapter
syntax are mappings onto those semantics.

### Foundation Types

The foundation consists of the existing types plus the following operational
types:

| Type | Canonical meaning |
| --- | --- |
| `string` | Unicode text. |
| `bool` | `true` or `false`. |
| `int` | Signed 64-bit integer. |
| `float` | IEEE 754 binary64 finite value. NaN and infinities are rejected. |
| `bytes` | An uninterpreted sequence of bytes. |
| `decimal` | Exact finite base-10 decimal value. |
| `date` | Calendar date without a time or timezone. |
| `datetime` | An absolute instant with an explicit offset, normalized to UTC. |
| `duration` | Signed elapsed time independent of a calendar or timezone. |
| `uuid` | RFC 4122-compatible 128-bit identifier in canonical textual form. |

`int` and `float` retain their current public names for compatibility. Their
width and finite-value semantics are now explicit.

The first implementation slice does not need to deliver every new type at
once. It must, however, use this table as the compatibility target and must not
introduce a conflicting public representation.

### Logical And Physical Representation

Primitive values are typed logical values even where a backend stores them
using a simpler physical representation.

- `string`, `bool`, `int`, and finite `float` map directly where supported.
- `bytes` uses a binary wire representation and a lossless backend mapping.
- `decimal` uses a canonical decimal textual wire/persistence representation
  until every relevant boundary can preserve exact decimal semantics directly.
- `date`, `datetime`, and `duration` use validated canonical representations;
  adapters must not accept ambiguous locale-dependent forms.
- `uuid` uses a validated canonical hyphenated textual representation unless a
  backend has a lossless native representation.

Physical encoding does not erase logical type. Schema metadata must retain the
declared primitive type, and reads must reconstruct and validate that logical
type before returning it through a canonical typed GRM surface.

Backend-native values that cannot be represented losslessly by the declared
GRM type must be rejected or exposed only through an explicitly backend-native,
non-portable capability.

### Absence And Null

The foundation distinguishes field absence from a stored null value.

- `required: true` means the property must be present.
- `required: false` means the property may be absent.
- Explicit stored `null` is not a foundation graph property value.

This preserves current behavior and avoids incompatible semantics across JSON,
protobuf, the in-memory backend, and Neo4j, where assigning null may mean
property removal. A future `nullable<T>` design may add explicit null only with
defined query, indexing, persistence, and backend behavior.

### Validation And Coercion

Canonical runtime and service requests carry typed values. They do not perform
implicit cross-type coercion.

- Adapters may parse lexical input into a requested declared type.
- `int` does not silently become `float`, and `float` does not silently become
  `decimal`.
- Strings are not interpreted as dates, UUIDs, or decimals unless the schema or
  typed request declares that logical type.
- Validation occurs before mutation and before a durable operation is accepted.
- Invalid values produce stable constraint errors without backend-specific
  leakage.

Equality, ordering, predicates, serialization, and round trips must follow the
declared logical type. A mode may report a capability as unsupported until it
can preserve those semantics; it must not claim parity through lossy coercion.

### Extended Types

The following remain part of the preserved SOML design horizon but are not
primitive foundation types selected by this ADR:

- enums and constrained/refinement types;
- `list<T>`, `set<T>`, `map<K,V>`, tuples, records, and unions;
- `optional<T>` and `nullable<T>` wrappers;
- semantic refinements such as email, URL, CIDR, Markdown, and JSON;
- handling wrappers such as `sensitive<T>`, `secret<T>`, `encrypted<T>`, and
  `attested<T>`;
- graph-native references, paths, traversals, projections, schema fragments,
  and affordances.

These require separate decisions because they affect schema descriptors,
query semantics, authorization, indexing, persistence, and backend capability
negotiation. The exploratory SOML notes remain the source material for that
future work.

## Compatibility And Evolution

- Protobuf evolution must be additive: existing enum numbers and oneof fields
  remain stable, and new primitive variants receive new identifiers.
- Durable JSON and binary workspace formats must preserve logical type and
  round-trip values without loss.
- Schema loading must reject unknown required type semantics rather than
  silently degrading them to strings.
- Capability reporting must identify unsupported primitive operations where a
  mode cannot preserve validation, comparison, or round-trip behavior.
- Schema migration for changing an existing field's declared type is separate
  future work; adding types does not imply automatic conversion of stored data.

## Consequences

Positive consequences:

- operational values gain portable meaning across GRM surfaces and backends;
- exact decimals, temporal values, identifiers, and binary evidence no longer
  need to masquerade as unconstrained strings or approximate floats;
- parity can be tested against one canonical type contract;
- future SOML refinements and graph-native types have a stable primitive base;
- Cypher and backend-native adapters can map to GRM semantics without becoming
  the semantic authority.

Tradeoffs:

- the runtime, service DTOs, persistence formats, adapters, and backends require
  coordinated additive changes;
- logical types may initially have less backend pushdown or indexing support;
- exact comparison and canonicalization introduce implementation dependencies
  and test obligations;
- rejecting lossy coercion may expose previously hidden adapter assumptions.

## Non-Goals

- No complete SOML generic type system in the first implementation.
- No arbitrary nested JSON, list, map, record, tuple, or union property values.
- No implicit schema migration or data rewriting.
- No guarantee that every backend immediately supports every new type.
- No backend-native type is added to the portable contract without canonical
  GRM semantics and cross-boundary tests.
- No change to the rule that typed structured operations are canonical.

## Required Proof

Implementation work following this ADR must provide:

- runtime validation and round-trip tests for each implemented primitive;
- protobuf code-generation and conversion tests;
- durable JSON and binary reopen/recovery tests;
- public CLI, Python, and MCP tests where those surfaces expose the type;
- shared backend contract tests for portable behavior;
- explicit capability/error tests for unsupported backend operations; and
- compatibility tests proving existing four-type workspaces still load and
  behave unchanged.

## Implementation Status

Implemented on 2026-09-21 for `bytes`, `decimal`, `date`, `datetime`,
`duration`, and `uuid`, while preserving the existing `string`, `bool`, signed
64-bit `int`, and finite binary64 `float` behavior.

New logical values use the canonical tagged JSON form
`{"$grm_type":"<type>","value":"<canonical-text>"}` at structured JSON
boundaries. Protobuf uses additive typed oneof fields, including binary bytes.
Runtime operations, JSON and binary durability, reopen/recovery, gRPC, Python,
and MCP preserve the logical type. Tagged values currently use predicate scans
rather than scalar property-index lookup.

Neo4j runtime operations explicitly report these six new types as unsupported
until portable native mappings preserve the same validation, comparison, and
round-trip semantics. No graph-native types, collections, generic wrappers,
schema migration, or Cypher expansion are included.

## Open Questions

- What canonical duration unit and textual form best supports exact comparison
  without conflating elapsed and calendar-relative time?
- Should future temporal support include a separate local `time` type and
  timezone identifier, or only absolute instants plus dates and durations?
- When should the schema type descriptor evolve beyond a flat primitive enum to
  support constraints, refinements, and generic composition?
