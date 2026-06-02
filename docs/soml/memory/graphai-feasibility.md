# GraphAI Feasibility Direction

Status: strategic feasibility direction, contingent on Innovate UK funding

Date: 2026-06-02

## Summary

GraphAI evaluates GRM/SOML as a graph-based intermediate representation for
traceable AI reasoning.

The core idea is that an AI system should not only produce an answer. It should
also leave behind structured, queryable reasoning memory: observations,
interpretations, decisions, constraints, conflicts, provenance, and temporal
validity represented as graph state that humans and agents can inspect.

## Problem

AI systems are increasingly used to support complex decisions across distributed
and heterogeneous data sources. Current interaction patterns, especially chat
and vector retrieval, can provide access to information but do not reliably
capture how information is interpreted, transformed, challenged, and carried
forward into decisions over time.

Even when a response presents intermediate reasoning, that reasoning is still
derived from model behavior that is not fully inspectable. In high-assurance
contexts this creates a gap: users need to understand, verify, and challenge
decision-relevant outputs using a representation that can be independently
queried.

## Proposed Approach

GraphAI should represent decision-relevant knowledge as a structured reasoning
graph. A feasibility prototype should focus on a small, explicit set of node and
edge concepts:

- observations and source inputs
- interpretations derived from observations
- decisions and recommendations
- constraints and policy conditions
- conflicts and inconsistencies
- provenance and attribution
- temporal validity and evolving state

The same graph representation should support both agent execution and human
interrogation. A human user should be able to ask about the graph using the same
structured memory that the AI system used to produce or support its decision.

## Challenge Queries

Useful feasibility scenarios should include queries such as:

- why was this decision made?
- what evidence supports it?
- what conflicting information exists?
- what changed over time?
- which constraints were active?
- which source influenced this interpretation?
- which decision paths depend on stale, missing, or disputed observations?

## Relationship To GRM

GRM is the implementation seed for this direction:

- typed graph workspaces provide the structured memory substrate
- runtime schema gives reasoning graphs explicit shape
- traversal and filtering support challenge queries
- explain/profile make access paths and query behavior inspectable
- local durability and WAL/replay work support reasoning state over time
- service-backed workspaces allow CLI, MCP, Python, Rust, and future clients to
  share one memory representation

GraphAI should not be framed as a separate product bolted onto GRM. It is an
applied feasibility track for the same SOML direction: structured operational
memory for applications and agents.

## Near-Term Implications

The current engineering sequence still holds, but the GraphAI lens changes why
the work matters:

- WAL/replay matters because reasoning memory must survive restarts and support
  audit of state transitions.
- Engine indexing and GraphBLAS-style acceleration matter because reasoning
  graphs need fast conflict detection, temporal traversal, path explanation, and
  multi-source evidence search.
- Explain/profile matters because users need inspectable query behavior, not
  another opaque reasoning layer.
- Provenance and temporal validity should become explicit scenario requirements
  before they become broad product claims.

Protocol standardization, protobuf versioning governance, and conformance
suites remain important longer-term work. They should follow more implementation
truth from durability, engine acceleration, and GraphAI feasibility scenarios.

## Non-Claims

This note is not an implementation claim.

GRM does not yet provide a complete GraphAI product, high-assurance validation
framework, hosted durability model, formal conformance suite, or complete
provenance/attestation runtime. Those claims should be made only after the
runtime, service surfaces, tests, and evaluation scenarios make them true.

## Evaluation Direction

A feasibility study should compare graph-based reasoning memory against
chat-only and vector-retrieval-only approaches across defined scenarios:

- multi-source decision support
- conflicting inputs
- evolving state
- missing or stale evidence
- failure and recovery conditions

The evaluation should assess whether structured reasoning graphs improve
interpretability, challengeability, resilience, and practical performance enough
to justify deeper GraphAI development.
