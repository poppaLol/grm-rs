# Import / Export

GRM-RS keeps local persistence separate from graph interchange.

- `session.save` / `session.load` restore a local workspace snapshot, including storage bookkeeping.
- `session.export` writes a machine-friendly interchange document for external tools, fixtures, migration, and future bulk import.
- `session.import` reads the same interchange document into a fresh session.
- `.grm` scripts remain the human-authored format for setup, examples, demos, and tests.

## Export Command

```text
session.export --json <path>
```

## Import Command

```text
session.import --json <path>
```

Import currently requires an empty session. If the session already has schema or graph data, GRM-RS raises an error for now instead of merging or replacing contents.

The JSON import/export format is currently an interchange v1 draft. It is versioned so import behavior can validate the format explicitly.

## JSON Shape

```json
{
  "format": "grm.interchange",
  "version": 1,
  "kind": "graph",
  "identity": {
    "node": "int",
    "edge": "int"
  },
  "schema": {
    "nodes": [
      {
        "name": "User",
        "id_field": "userId",
        "id_type": "int",
        "fields": [
          { "name": "name", "type": "string", "required": true }
        ]
      }
    ],
    "edges": [
      {
        "name": "Authored",
        "from": "User",
        "to": "Post",
        "id_field": "authoredId",
        "id_type": "int",
        "fields": [
          { "name": "year", "type": "int", "required": true }
        ]
      }
    ]
  },
  "data": {
    "nodes": [
      {
        "id": 1,
        "model": "User",
        "props": {
          "name": "Alice"
        }
      }
    ],
    "edges": [
      {
        "id": 1,
        "model": "Authored",
        "from": 1,
        "to": 2,
        "props": {
          "year": 2024
        }
      }
    ]
  }
}
```

## Format Notes

- `format` is always `grm.interchange` for this document family.
- `version` is currently `1`.
- `identity.node` and `identity.edge` describe backend ID shape for exported references.
- `schema.nodes` and `schema.edges` contain runtime model definitions.
- `data.nodes` and `data.edges` are arrays rather than storage maps so external tools can stream or transform them more easily.
- Exported IDs are included because edges reference nodes by ID.
- Storage-only fields such as `next_node_id` and `next_rel_id` are intentionally omitted.
- Import restores the exported IDs and advances the next generated IDs beyond the imported maximums.

## Planned Follow-Ups

- Conflict handling for imports into non-empty sessions.
- Richer schema and data validation diagnostics.
- `session.export --jsonl <path>` and `session.import --jsonl <path>` for streaming-oriented bulk workflows.
