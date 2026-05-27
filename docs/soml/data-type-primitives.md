# SOML Primitive Types and Generics

Status: exploratory architecture note

This note documents the emerging direction for primitive data types, container types, and generic type wrappers in SOML.

SOML types should not be limited to basic storage values such as `string` and `number`. The type system should provide enough structure to support validation, runtime behaviour, semantic interpretation, policy enforcement, and safe use by agents.

The goal is not to build a full programming language immediately. The goal is to define a practical type vocabulary that can grow from simple schema validation into richer graph-resident operational semantics.

This is not an implementation claim. Current GRM runtime schema supports a much smaller field type set, and richer SOML primitive, generic, sensitive, attested, and graph-native types should become product claims only after runtime/service surfaces and tests make them true.

---

## 1. Primitive Scalar Types

The initial SOML type system should include a small set of core scalar types.

Candidate scalar types:

```soml
string
bool
int
float
decimal
number
null
bytes
```

### Notes

`number` may remain as a convenient broad type, but internally it is useful to distinguish:

```soml
int
float
decimal
```

This distinction matters because different number types imply different behaviour.

Examples:

```soml
field retry_count: int
field confidence_score: float
field invoice_total: decimal
```

Suggested interpretation:

* `int` is for whole-number counts and identifiers
* `float` is for approximate measurement, scoring, ranking, and statistical values
* `decimal` is for financial or exact base-10 values
* `number` is a broad or unresolved numeric type

---

## 2. Temporal Types

Temporal types should be treated as first-class primitives.

Candidate temporal types:

```soml
date
time
datetime
duration
timezone
interval<T>
```

Examples:

```soml
field created_at: datetime
field valid_from: datetime
field valid_to: datetime
field retention_period: duration
field local_timezone: timezone
field active_window: interval<datetime>
```

Temporal types are important because SOML is expected to support operational memory, provenance, retention, validity, expiry, evidence timelines, and policy enforcement.

---

## 3. Identity and Reference Types

SOML should include explicit identity-oriented types.

Candidate identity types:

```soml
uuid
ulid
id<T>
ref<T>
```

Examples:

```soml
field event_id: uuid
field memory_id: ulid
field owner: ref<Principal>
field related_risk: ref<Risk>
```

`id<T>` represents a typed identity value.

`ref<T>` represents a reference to another typed object.

This distinction allows SOML to separate “this is an identifier” from “this points to another entity”.

---

## 4. Collection Types

SOML should distinguish between ordered, unordered, keyed, and fixed-shape collections.

Candidate collection types:

```soml
list<T>
set<T>
map<K, V>
tuple<T...>
```

Examples:

```soml
field tags: set<string>
field evidence_items: list<ref<Evidence>>
field scores_by_axis: map<string, float>
field coordinates: tuple<float, float>
```

Suggested interpretation:

* `list<T>` is ordered and may contain duplicates
* `set<T>` is unordered and should not contain duplicates
* `map<K, V>` is keyed lookup data
* `tuple<T...>` is fixed-shape positional data

This distinction matters for agentic systems because order, uniqueness, and structure affect interpretation.

---

## 5. Optional and Nullable Types

SOML should support optional values explicitly.

Possible syntax:

```soml
string?
optional<string>
nullable<string>
```

Example:

```soml
field middle_name: string?
field end_date: optional<datetime>
```

A design decision is needed on whether `optional<T>` and `nullable<T>` mean the same thing.

One possible distinction:

```soml
optional<T>  // the field may be absent
nullable<T>  // the field may be present with null value
```

This distinction is useful when mapping between JSON-like documents, database records, APIs, and graph properties.

---

## 6. Union Types

SOML should eventually support union types.

Example syntax:

```soml
string | int
EmailAddress | PhoneNumber
SuccessResult | FailureResult
```

Examples:

```soml
field external_id: string | int
field contact_method: EmailAddress | PhoneNumber
field result: Success | Failure
```

Union types are useful where external data sources are inconsistent, or where an operation can legitimately produce one of several result shapes.

However, union types should probably be introduced carefully because they complicate validation, indexing, and query planning.

---

## 7. Enum Types

Enums should be part of the practical early type system.

Example:

```soml
enum RiskLevel {
  Low
  Medium
  High
  Critical
}
```

Usage:

```soml
field risk_level: RiskLevel
field status: enum {
  Draft
  Active
  Deprecated
  Revoked
}
```

Enums are useful for controlled vocabularies, status fields, classification, policy states, and workflow states.

They are also agent-friendly because they reduce free-text ambiguity.

---

## 8. Generic Type Wrappers

SOML should support generic wrappers that add meaning or behaviour to another type.

Examples:

```soml
sensitive<T>
secret<T>
classified<T>
encrypted<T>
attested<T>
validated<T>
versioned<T>
deprecated<T>
```

These wrappers should not merely describe storage. They should influence runtime behaviour.

Example:

```soml
field email: sensitive<email>
field api_key: secret<string>
field assessment: attested<markdown>
field payload: encrypted<json>
field old_label: deprecated<string>
```

Suggested interpretation:

* `sensitive<T>` means the value requires controlled handling
* `secret<T>` means the value should not be exposed
* `classified<T>` means the value has an organisational/security classification
* `encrypted<T>` means the value should be protected at rest or in transit
* `attested<T>` means the value has provenance or integrity evidence
* `validated<T>` means the value has passed a validation process
* `versioned<T>` means the value participates in version history
* `deprecated<T>` means the value exists but should not be preferred

---

## 9. Constrained Types

SOML should support constraints on primitive and collection types.

Examples:

```soml
string[max_length=200]
string[min_length=1, max_length=80]
int[min=0, max=10]
float[min=0.0, max=1.0]
list<string>[min_items=1, max_items=10]
```

Examples in fields:

```soml
field title: string[min_length=1, max_length=120]
field confidence: float[min=0.0, max=1.0]
field tags: set<string>[max_items=20]
```

Constrained types are useful for:

* validation
* UI generation
* safe agent output
* indexing
* policy enforcement
* API contracts

---

## 10. Semantic Refinement Types

Some values are technically strings, numbers, or structured objects, but semantically more specific.

Candidate semantic refinements:

```soml
email
url
uri
domain
ip_address
cidr
country_code
language_code
currency_code
markdown
html
json
yaml
regex
```

Examples:

```soml
field contact_email: email
field website: url
field source_domain: domain
field client_ip: ip_address
field network_range: cidr
field notes: markdown
field raw_payload: json
```

These could be implemented either as first-class primitive types or as refinements over existing primitives.

For example:

```soml
type email = string[format=email]
type markdown = string[format=markdown]
type ip_address = string[format=ip_address]
```

The important point is that SOML should know the semantic intent of the value.

---

## 11. Type Composition

The type system should allow composition.

Examples:

```soml
list<sensitive<email>>
attested<sensitive<markdown>>
encrypted<json>
map<string, list<ref<Evidence>>>
optional<validated<traversal<Risk, Evidence>>>
```

This is important because operational graph systems often need to express several concerns at once.

For example:

```soml
field evidence_summary: attested<sensitive<markdown>>
```

This means:

* the value is markdown
* it contains sensitive information
* it should carry provenance or integrity evidence

The runtime should be able to inspect those wrappers and apply behaviour accordingly.

---

## 12. Suggested Early Implementation Set

A practical initial implementation could include:

```soml
string
bool
int
float
decimal
number
null
bytes

date
time
datetime
duration

uuid
ulid
ref<T>

list<T>
set<T>
map<K, V>

optional<T>
enum
```

Then later introduce:

```soml
sensitive<T>
secret<T>
encrypted<T>
attested<T>
validated<T>

string[max_length=...]
int[min=..., max=...]
float[min=..., max=...]

email
url
domain
ip_address
markdown
json
```

This keeps the first step manageable while leaving a clear path toward richer semantic behaviour.

---

## 13. Why This Matters

A richer SOML type system supports:

* better validation
* safer agent behaviour
* fewer hallucinated assumptions
* improved graph schema clarity
* better UI generation
* stronger API contracts
* policy-aware traversal
* evidence-aware reasoning
* retention and expiry logic
* clearer distinction between data, meaning, and handling requirements

The type system is not just about storage.

It is part of the operating semantics of the graph.

---

## 14. Design Principle

Use simple types for simple values.

Use semantic types where meaning matters.

Use wrappers where handling behaviour matters.

Use graph-native types where the value represents executable or inspectable graph structure.

The long-term goal is for SOML types to help agents understand not only what a value is, but also how it may be used.
