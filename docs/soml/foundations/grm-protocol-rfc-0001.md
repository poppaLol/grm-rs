# RFC-0001  GRM Protocol Standard (Draft)

## Introduction

The Graph Relational Model (GRM) Protocol is an open, implementation-independent standard for secure, structured graph operations.

The protocol defines a typed operational interface for creating, querying, traversing and managing graph data without requiring clients to construct or execute backend-specific query languages.

Rather than exposing executable query strings, GRM clients express operational intent through a strongly-typed protocol. GRM implementations are responsible for validating, planning and executing these operations against one or more storage backends.

The protocol is designed to support traditional application development, distributed systems, agentic runtimes, operational memory systems and future graph-native infrastructure while remaining secure by default and portable across implementations.

---

## Design Principles

### Secure by Default

Clients communicate using structured protocol messages rather than executable query text.

The protocol aims to eliminate entire classes of common application vulnerabilities arising from query construction, string interpolation and backend-specific execution semantics.

Security is considered a core property of the protocol rather than an optional implementation detail.

### Typed Operations

All graph interactions are represented as explicit operations.

Nodes, relationships, traversals, schema definitions and mutations are described through structured contracts with well-defined behaviour.

### Backend Independence

The protocol does not define a storage engine.

Implementations may execute operations against:

* In-memory graph stores
* File-backed graph stores
* Neo4j
* Relational databases
* Distributed graph systems
* Future storage technologies

A compliant implementation exposes the same protocol behaviour regardless of underlying persistence technology.

### Explainability

Every operation should be capable of producing an execution explanation describing:

* Operation interpretation
* Planning decisions
* Traversal strategy
* Backend execution approach

This supports observability, debugging, auditing and AI-assisted reasoning.

### Workspace Isolation

Operations occur within an explicit workspace context.

Workspaces provide isolation boundaries for:

* Data
* Schema
* Persistence configuration
* Runtime state

This enables predictable execution and simplifies multi-tenant deployments.

Conceptually, a workspace serves a role similar to a SQL schema, a MongoDB collection or database, or a namespace within other storage systems. Implementations may translate workspaces into backend-specific isolation constructs, but the workspace abstraction remains consistent at the protocol level regardless of how it is represented internally.

Workspaces are expected to become the primary unit of isolation, persistence, security and operational coordination within the protocol.

---

## Scope of Version 1

The initial standard focuses on operational graph management and traversal.

### Workspace Lifecycle

* CreateWorkspace
* OpenWorkspace
* ExecuteWorkspace
* CloseWorkspace

The workspace is the primary execution boundary for all protocol operations.

Depending on the implementation, a workspace may map to a dedicated graph, database, schema, collection, namespace or other backend-specific container. The protocol intentionally abstracts these details to provide a consistent operational model across storage technologies.

### Schema Operations

Supported operations include:

* Schema definition
* Schema inspection
* Schema enumeration

Schema definitions provide the structural contract for graph operations.

### Node Operations

Supported operations include:

* Create
* Update
* Delete
* Find

### Relationship Operations

Supported operations include:

* Create
* Update
* Delete
* Find

### Traversal Operations

Traversal-capable node queries form the primary graph navigation mechanism.

Supported concepts include:

* Multi-hop traversal
* Typed relationship traversal
* Directional traversal
* Root projections
* End-node projections
* Edge projections

### Execution Analysis

Implementations should support:

* Explain
* Profile

These operations expose execution behaviour without modifying graph state.

### Batch Operations

The protocol supports grouped operations executed as a single logical request.

Implementations may provide transactional guarantees where supported by the underlying backend.

### Persistence

The protocol currently defines:

* Binary workspace persistence as the default format
* JSON persistence as an explicit interoperability option

Additional persistence formats may be standardised in future revisions.

---

## Operational Intent over Query Syntax

GRM deliberately standardises operational intent rather than textual query syntax.

Traditional database systems expose executable query languages which require clients to construct and submit textual instructions for execution. While flexible, this approach introduces portability, validation and security challenges which become increasingly significant in distributed systems, agent runtimes and machine-generated workloads.

GRM adopts a different model.

Clients express what operation they wish to perform through a structured protocol contract. Implementations determine how those operations are planned and executed against the underlying storage engine.

This distinction allows implementations to provide consistent behaviour across storage technologies while reducing dependency on backend-specific query languages.

The protocol therefore defines graph operations rather than graph scripts.

---

## Explicit Non-Goals

The following areas are intentionally outside the scope of Version 1. Some of them form non-goals which will remain in the zone of avoidance for functionality considered regressive to the long-term direction of the standard.

### Free-Form Query Languages

GRM is not intended to become another graph query language, nor a replacement for existing scripting languages.

The standard intentionally avoids introducing new textual query syntaxes.

The directly stated intent is to make executable graph query languages unnecessary for most application development.

Implementations may choose to provide translation layers from external languages such as Cypher, GQL, SQL or GraphQL. Such translators are implementation concerns and are not considered part of the protocol standard itself.

The long-term direction of the protocol is to encourage structured operational contracts rather than executable query text.

### Backend-Specific Features

The protocol avoids exposing implementation-specific capabilities that reduce portability between compliant systems.

### Storage Engine Standardisation

The protocol defines behaviour rather than storage architecture.

Implementations remain free to innovate internally.

Storage engine concerns should primarily remain implementation details, although future standards may provide mechanisms for communicating storage characteristics and behavioural guarantees.

---

## Capability-Based Implementations

GRM recognises that not all implementations will provide identical functionality.

Rather than exposing backend-specific features directly, implementations communicate supported functionality through capability declarations.

Capabilities form part of the contract between caller and implementation and allow clients to reason about supported behaviour in a portable manner.

Examples may include:

* Traversal support
* Explain support
* Profile support
* Persistence support
* Snapshot support
* Distributed execution support
* Multi-writer support
* Durability guarantees
* Consistency guarantees

Future RFCs are expected to formalise a common capability taxonomy and negotiation mechanism.

Capability is expected to become a first-class concept within the protocol, allowing callers to discover, negotiate and depend upon specific behavioural guarantees without knowledge of implementation details.

---

## Future Areas of Standardisation

The following areas are expected to evolve through future RFCs.

### Authentication and Transport Security

* TLS requirements
* Identity models
* Authentication mechanisms
* Authorisation semantics

In the modern era this is an absolute requirement and remains a priority for future revisions of the standard.

### Import and Export

* Interoperability formats
* Bulk loading
* Data migration
* Cross-system exchange

This is necessary for practical data exchange between systems without lock-in.

### Snapshot Management

* Backup semantics
* Restore semantics
* Point-in-time recovery

This is expected to become the primary semantic for communicating how data is protected, recovered and reverted.

### Hosted Durability

* Replication
* Persistence guarantees
* Failure recovery models
* Isolation semantics
* Atomicity guarantees

As a database protocol standard, GRM should eventually provide common language for discussing durability, consistency, isolation and recovery behaviour.

### Multi-Writer Coordination

Future versions may define common semantics for:

* Concurrent modification
* Conflict resolution
* Consistency guarantees
* Point-in-time accuracy
* Distributed execution behaviour

As a modern operational protocol, GRM should provide mechanisms through which clients and implementations can reason about multi-agent and multi-writer access patterns.

---

## Vision

GRM aims to provide a common operational language for graph systems in the same way that HTTP provides a common language for web systems.

Applications should be able to interact with graph data through a portable, secure and explainable protocol without knowledge of backend implementation details.

GRM seeks to become for graph operations what HTTP became for resource access: a common, implementation-neutral protocol capable of spanning local runtimes, databases, services, agent systems and future graph-native infrastructure.

The long-term goal is to establish a foundation for graph-native applications, structured operational memory systems, agent runtimes and future distributed graph infrastructure while preserving strong security and interoperability guarantees.

In doing so, GRM intentionally shifts focus away from query languages and toward operational contracts, capability negotiation, explainable execution and secure-by-default graph computing.
