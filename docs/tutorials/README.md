# GRM Tutorials

This directory is the planned home for workflow-first tutorials.

The README is the front door for the project, and the roadmap explains where the
project is going. Detailed command references and end-to-end walkthroughs should
live here as the tutorial set grows.

Available tutorials:

- [CLI sessions](cli-session.md): define schema, create data, traverse,
  explain/profile, save, and export
- [Python sessions](python-session.md): the same runtime schema and graph
  workflow through `grm_rs.Session`
- [MCP workflows](mcp-workflow.md): agent-oriented graph memory with structured
  tools, batching, traversal, explain/profile, and export

Planned tutorials:

- Rust typed models: derives, repositories, typed IDs, transactions, and
  traversal
- Neo4j backend: running shared behavior against a live graph backend

Existing starting points:

- [Python quickstart](../python-quickstart.md)
- [Query language design](../query-language-design.md)
- [Import/export](../import-export.md)
- [MCP batch and graph patch requirements](../mcp-batch-graph-patch-requirements.md)
