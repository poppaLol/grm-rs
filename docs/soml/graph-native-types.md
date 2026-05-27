# SOML Graph-Native Types And Affordances

Status: exploratory architecture note

This note records a future SOML direction: SOML data types should describe not
only primitive values, but also graph-native operational structures that agents
and applications can inspect, validate, and execute safely.

This is not an implementation claim. Current GRM runtime schema is still much
smaller. These concepts should become product claims only when runtime/service
surfaces and tests make them true.

## Graph-Native Type Candidates

Candidate graph-native SOML types include:

```soml
node_type
rel_type
node_ref<T>
rel_ref<T>
path<TStart, TEnd>
pattern
traversal<TIn, TOut>
query<TOut>
projection<T>
subgraph
schema_fragment
affordance
```

These types let graph "things" be stored as typed data inside operational
memory. The goal is to reduce prompt-only reconstruction of labels,
relationship names, traversal shapes, query assumptions, and schema fragments.

## Affordances

An affordance is a stored, validated, permissionable graph action available to
an agent or application in a specific context.

An affordance should describe:

- what the caller may do
- required input type
- expected output type
- allowed traversal, query, or pattern
- constraints and safety rules
- evidence requirements
- policy restrictions
- version and provenance metadata

Example:

```soml
affordance InvestigateSupplierRisk {
  input: Supplier
  allowed_traversals: [
    FindContracts,
    FindKnownRisks,
    FindControls,
    FindOpenQuestions
  ]
  forbidden_traversals: [
    TraversePersonalData
  ]
  output: RiskAssessmentDraft
}
```

Affordances align with the SOML framing because they are typed operational
memory, not textual prompts or arbitrary query strings.

## Skills As Graph-Resident Operating Models

Skills should evolve from markdown prompt files into graph-resident operating
models.

**Skills are not prompts. Skills are graph-resident operating models.**

A skill can contain:

- concepts
- affordances
- reusable traversals
- constraints
- output shapes
- evidence requirements
- safety rules
- permissions
- examples
- version metadata

Markdown skill files may remain useful as thin shims, bootloaders,
human-readable exports, or fallback prompts. They should not be the canonical
runtime representation once SOML skill semantics exist.

Example:

```soml
skill CyberRiskAnalyst {
  concepts: [Asset, Threat, Control, Vulnerability, Risk]
  affordances: [
    IdentifyRisks,
    TraceControls,
    FindEvidence,
    ProduceAssessment
  ]
  traversals: [
    AssetToRisks,
    RiskToControls,
    ControlToEvidence
  ]
  constraints: [
    RequireEvidenceForClaims,
    DoNotExposeSensitiveData,
    PreferAttestedSources
  ]
}
```

## Why This Matters

Graph-native SOML types and affordances can:

- reduce hallucinated graph labels and relationship types
- make agent behaviour inspectable
- make skills versionable and testable
- enable permissioned actions
- support audit and future attestation
- allow different models and runtimes to use the same skill
- turn compliance and safety rules into executable graph semantics

## Implementation Notes

This does not need to be fully implemented immediately. A practical sequence is:

1. Document the conceptual model.
2. Introduce SOML type definitions in graph memory/docs.
3. Add validated traversal objects.
4. Add affordance and skill nodes.
5. Connect skills, affordances, traversals, policies, and output schemas.
6. Add runtime lookup of available affordances by context.

Near-term work should stay honest: graph-native types, affordances, projections,
and attestations are architecture direction until implemented and tested through
public runtime or service surfaces.
