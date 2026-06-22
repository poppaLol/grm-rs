# Architecture Decision Records

This directory records product and architecture decisions that should survive
individual PRs, chats, and implementation phases.

Records use a lightweight ADR format:

- status
- context
- decision
- consequences
- open questions

Current records:

- [ADR 0001: Separate Graph Data From Schema Memory](0001-graph-data-and-schema-memory.md)
- [ADR 0002: Keep Monorepo While Designing Split-Ready Service Boundaries](0002-monorepo-with-split-ready-service-boundaries.md)
- [ADR 0003: Transparent Backend Acceleration From Profiled Workloads](0003-transparent-backend-acceleration.md)
- [ADR 0004: Frame GRM As A Structured Operational Memory Layer](0004-structured-operational-memory-layer.md)
- [ADR 0005: Use Graph Workspaces And Durable Envelopes](0005-graph-workspace-and-durable-envelope.md)
- [ADR 0006: Use Explicit mTLS Certificate Mapping As The First Application Authentication Provider](0006-mtls-certificate-mapping-authentication-provider.md)
- [ADR 0007: Authorize Exact Server-Derived Workspace Permissions](0007-server-derived-workspace-permissions.md)
- [ADR 0008: Require Bounded Redacted Audit At Security And Effect Boundaries](0008-bounded-authoritative-security-audit.md)
