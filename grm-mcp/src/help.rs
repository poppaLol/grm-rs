use serde_json::{Value, json};

pub const AGENT_GUIDE: &str = r#"GRM is a runtime graph session exposed over MCP.

Recommended agent workflow:
1. Call grm_help when first using this server in a session.
2. Call grm_schema_list or read grm://schema before creating or querying data.
3. If Neo4j mode is active, read grm://backend/status and grm://graph/summary; if schema_template_loaded is true, call grm_schema_list and use the recovered models. If runtime schema is empty, define or reconstruct schema before typed reads or writes.
4. Before defining schema, decide the graph's richness vs sparseness.
5. Prefer structured tools for schema, node, edge, introspection, import, export, and persistence operations.
6. For more than 3 creates or updates, prefer grm_batch with ops as structured operation objects, not CLI strings or JSON-encoded strings.
7. Use grm_explain or grm_profile to inspect node.find and edge.find plans when supported by the active backend.
8. Prefer grm_node_find for structured traversal-capable node.find requests; use grm_query for exact CLI parity when supported by the active backend.
9. After any tool error you cannot immediately fix, call grm_tool_help for that tool.
10. Verify writes with grm://graph/summary, grm://graph/export, or grm_export when supported by the active backend.

Neo4j mode note:
- Runtime schema metadata is session-local. Neo4j graph data may already exist even when grm_schema_list is empty.
- GRM_SCHEMA_TEMPLATE is an optional server startup environment variable, not a tool call. When set by the operator, it points at a local GRM JSON session file used as durable schema memory while Neo4j stores graph data.
- If the file is missing, startup creates a fresh schema memory file. If it exists, startup recovers runtime schema from it. Invalid files fail startup loudly.
- On startup, call grm_schema_list and inspect grm://backend/status and grm://graph/summary. If schema_template_loaded is true, verify the recovered models before writing. If schema is empty, ask whether to define a fresh schema or reconstruct one from project docs.
- Neo4j mode supports grm_schema_checkpoint as an explicit maintenance operation to fold schema-memory append-log records into the configured GRM_SCHEMA_TEMPLATE base file. Do not call it during startup or read-only orientation unless the operator requests compaction or the append log is too large. Neo4j mode also supports grm_batch for schema_define_node, schema_define_edge, node_create, node_update, node_delete, edge_create, edge_update, edge_delete, and graph summary counts for the current session-local runtime schema. General snapshots, import/export, autocommit, explain/profile, and traversal/query parity are not supported yet.

Schema richness vs sparseness:
- Rich schemas use more specific node and edge models when concepts have distinct fields, constraints, relationships, or query meaning.
- Sparse schemas use fewer broader node and edge models when instances share a shape and differ mainly by property values such as kind, type, or category.
- Prefer rich models when future queries will care about the distinction as graph structure or traversal semantics.
- Prefer sparse models when the distinction is mostly descriptive data.

Property values must be strings, numbers, or booleans. Null, arrays, and objects are not supported as graph property values.
"#;

pub fn help_index() -> Value {
    json!({
        "server": "grm-mcp",
        "purpose": "Expose a local grm-rs runtime graph session to agents.",
        "recommended_workflow": [
            "Call grm_help when first using this server in a session.",
            "Call grm_schema_list or read grm://schema before creating or querying data.",
            "If Neo4j mode is active, read grm://backend/status and grm://graph/summary; if schema_template_loaded is true, call grm_schema_list and use the recovered models. If runtime schema is empty, define or reconstruct schema before typed reads or writes.",
            "Before defining schema, decide the graph's richness vs sparseness.",
            "Prefer structured tools over grm_query except when exact CLI-compatible command text is required.",
            "Use grm_explain or grm_profile to inspect node.find and edge.find plans.",
            "For more than 3 creates or updates, prefer grm_batch with ops as structured operation objects, not CLI strings or JSON-encoded strings.",
            "After recoverable errors, call grm_tool_help with the tool name before retrying.",
            "Verify writes with grm://graph/summary, grm://graph/export, or grm_export."
        ],
        "modeling_guidance": {
            "richness_vs_sparseness": [
                "Rich schemas use more specific node and edge models when concepts have distinct fields, constraints, relationships, or query meaning.",
                "Sparse schemas use fewer broader node and edge models when instances share a shape and differ mainly by property values such as kind, type, or category.",
                "Prefer rich node models when categories carry different data or relationship patterns, for example Knife, Plate, and Fork.",
                "Prefer sparse node models when categories are mostly values on one shape, for example Kitchenware with kind=knife|plate|fork.",
                "Prefer rich edge models when relationships mean different things or drive different traversals, for example AUTHORED, PURCHASED, LOCATEDIN, and DEPENDSON.",
                "Prefer sparse edge models when relationships share meaning and differ mainly by properties, for example RELATEDTO with kind, confidence, and source."
            ],
            "batching": "After choosing schema granularity, batch related schema and data mutations. For more than 3 related creates or updates, prefer grm_batch so refs, validation, and rollback happen together. In grm_batch, ops must be an array of operation objects, not CLI strings or JSON-encoded strings."
        },
        "neo4j_schema_memory": {
            "configuration": "GRM_SCHEMA_TEMPLATE=<path> is set before starting grm-mcp; it is not passed to a GRM tool.",
            "purpose": "Persist and recover session-local runtime schema metadata for Neo4j mode using a local GRM JSON session file.",
            "missing_file_behavior": "If the file is missing, the server starts fresh and creates it. Later schema definitions are appended to that local file.",
            "existing_file_behavior": "If the file exists, the server recovers schema memory from it. Invalid or inconsistent files fail startup.",
            "checkpoint_behavior": "grm_schema_checkpoint folds the current runtime schema into the configured base file and clears the schema-memory append log without modifying Neo4j graph data. It is an explicit maintenance operation, not part of startup or read-only orientation.",
            "checkpoint_when_to_call": "Call only when the operator requests schema-memory compaction or the append log is too large.",
            "does_not": [
                "create Neo4j nodes",
                "create Neo4j relationships",
                "persist schema metadata into Neo4j",
                "infer schema from Neo4j labels or properties"
            ],
            "agent_startup_flow": [
                "Call grm_schema_list.",
                "Read grm://backend/status.",
                "If schema_template_loaded is true, compare the recovered node and edge models with the intended write.",
                "If schema_template_persistence_enabled is true and schema_template_loaded is false, this server started with fresh local schema memory.",
                "If runtime_schema_empty is true, ask whether to define schema with grm_schema_define_node/grm_schema_define_edge or grm_batch.",
                "Only write after the runtime schema contains the target models and fields."
            ]
        },
        "resources": [
            "grm://docs/agent-guide",
            "grm://docs/query-language",
            "grm://docs/tool-help",
            "grm://backend/status",
            "grm://schema",
            "grm://graph/summary",
            "grm://graph/export"
        ],
        "tool_categories": {
            "help": ["grm_help", "grm_tool_help"],
            "schema": ["grm_schema_list", "grm_schema_checkpoint", "grm_schema_define_node", "grm_schema_define_edge", "grm_index_list"],
            "batch": ["grm_batch"],
            "nodes": ["grm_node_create", "grm_node_update", "grm_node_delete", "grm_node_find"],
            "edges": ["grm_edge_create", "grm_edge_update", "grm_edge_delete", "grm_edge_find"],
            "query": ["grm_explain", "grm_profile", "grm_query"],
            "persistence": ["grm_save", "grm_load", "grm_import", "grm_export"]
        },
        "value_rules": [
            "Graph property values may be strings, numbers, or booleans.",
            "Null, arrays, and objects are rejected as graph property values.",
            "Required fields must be supplied when creating nodes or edges.",
            "Only fields declared in the runtime schema may be supplied."
        ],
        "when_to_use_grm_query": [
            "Use grm_node_find for structured node traversals with via, end_filters, edge_filters, return, order, limit, and offset.",
            "Use grm_query when you want exact CLI-compatible behavior.",
            "Prefer grm_node_find and grm_edge_find for model/filter lookups and supported structured traversal-shaped node finds."
        ],
        "when_to_use_introspection": [
            "Use grm_explain to inspect the current logical plan without running the query.",
            "Use grm_profile to run the same query path and return plan, result_rows, and elapsed time.",
            "Pass node.find or edge.find command text, for example node.find User name=\"Alice\"."
        ],
        "known_tools": known_tools()
    })
}

pub fn tool_help(name: &str) -> Option<Value> {
    let help = match name {
        "grm_help" => json!({
            "tool": "grm_help",
            "purpose": "Return the server guide, resources, value rules, and common workflow in one JSON object.",
            "example": {},
            "related": ["grm_tool_help", "grm://docs/agent-guide"]
        }),
        "grm_tool_help" => json!({
            "tool": "grm_tool_help",
            "purpose": "Return usage examples, preconditions, and recovery hints for one GRM MCP tool.",
            "example": { "tool": "grm_node_create" },
            "common_errors": [
                recovery("unknown tool", "Call grm_help and choose one of known_tools.")
            ],
            "related": ["grm_help", "grm://docs/tool-help"]
        }),
        "grm_schema_list" => json!({
            "tool": "grm_schema_list",
            "purpose": "Return node models, edge models, and backend identity types.",
            "before_calling": ["Call this before creating or querying graph data if model fields are unknown."],
            "neo4j_note": "In Neo4j mode this reports session-local runtime schema, not full Neo4j store introspection. GRM_SCHEMA_TEMPLATE may recover this schema from a local GRM session file at server startup. If it is empty, define or reconstruct schema before typed reads or writes.",
            "neo4j_startup_interpretation": [
                "If grm://backend/status reports schema_template_loaded=true, this tool should show the recovered schema-memory models.",
                "If schema_template_persistence_enabled=true and schema_template_loaded=false, the configured local file was missing and this server started with fresh schema memory.",
                "Recovered schema memory is metadata only; it does not prove any matching Neo4j nodes or relationships exist."
            ],
            "example": {},
            "related": ["grm://schema", "grm://backend/status", "grm_help"]
        }),
        "grm_schema_checkpoint" => json!({
            "tool": "grm_schema_checkpoint",
            "purpose": "Fold Neo4j session-local runtime schema memory into the configured GRM_SCHEMA_TEMPLATE base checkpoint and clear its append log.",
            "before_calling": [
                "Use only in Neo4j MCP mode.",
                "Confirm grm://backend/status reports schema_template_persistence_enabled=true."
            ],
            "notes": [
                "This checkpoints runtime schema metadata only.",
                "It does not create, update, delete, compact, or otherwise modify Neo4j graph data.",
                "Schema definitions are already durable in the append log before this tool is called."
            ],
            "example": {},
            "common_errors": [
                recovery("only supported in Neo4j MCP mode", "Start grm-mcp with GRM_BACKEND=neo4j."),
                recovery("requires GRM_SCHEMA_TEMPLATE", "Configure GRM_SCHEMA_TEMPLATE before starting grm-mcp.")
            ],
            "related": ["grm_schema_list", "grm://backend/status"]
        }),
        "grm_index_list" => json!({
            "tool": "grm_index_list",
            "purpose": "Return the current automatic system index catalog.",
            "notes": [
                "Indexes are backend-maintained derived acceleration structures.",
                "User-defined indexes and durable index contents are future work."
            ],
            "example": {},
            "related": ["grm_explain", "grm_profile", "grm_schema_list"]
        }),
        "grm_schema_define_node" => json!({
            "tool": "grm_schema_define_node",
            "purpose": "Define a runtime node model.",
            "modeling_guidance": [
                "Decide richness vs sparseness before defining many similar node models.",
                "Use richer, specific node models when categories have distinct fields, constraints, relationship patterns, or query meaning.",
                "Use a sparser, broader node model with a kind/type/category field when instances share one shape and differ mostly by property values."
            ],
            "example": {
                "name": "File",
                "id_field": "fileId",
                "fields": [
                    { "name": "path", "type": "string", "required": true },
                    { "name": "summary", "type": "string", "required": false }
                ]
            },
            "common_errors": [
                recovery("model name must be PascalCase", "Use a model name such as File or RustItem."),
                recovery("field name 'id' is reserved", "Choose a domain id field such as fileId or itemId."),
                recovery("field is defined more than once", "Remove duplicate field names, including duplicates of the id_field.")
            ],
            "related": ["grm_schema_list", "grm_node_create"]
        }),
        "grm_batch" => json!({
            "tool": "grm_batch",
            "purpose": "Apply an ordered list of structured schema, node, and edge mutations in one MCP call.",
            "input_shape": [
                "ops is an array of operation objects, not CLI command strings and not serialized JSON strings.",
                "Correct item shape: { \"op\": \"node_create\", \"args\": { \"model\": \"File\", \"props\": { \"path\": \"src/lib.rs\" } } }.",
                "Incorrect item shape: \"{\\\"op\\\":\\\"node_create\\\",\\\"args\\\":{...}}\"."
            ],
            "modeling_guidance": [
                "Before batching schema creation, choose the graph's richness vs sparseness.",
                "Use richer node/edge models when distinctions matter to fields, constraints, relationships, or traversal semantics.",
                "Use sparser node/edge models when distinctions are mostly descriptive property values.",
                "After choosing granularity, batch related schema definitions and data creation together."
            ],
            "defaults": {
                "atomic": true,
                "allow_deletes": false,
                "response": "summary"
            },
            "supported_ops": [
                "schema_define_node",
                "schema_define_edge",
                "node_create",
                "node_update",
                "node_delete",
                "edge_create",
                "edge_update",
                "edge_delete"
            ],
            "neo4j_supported_ops": [
                "schema_define_node",
                "schema_define_edge",
                "node_create",
                "node_update",
                "node_delete",
                "edge_create",
                "edge_update",
                "edge_delete"
            ],
            "neo4j_note": "Neo4j mode currently requires atomic=true. It applies supported batch operations in order, writes graph mutations in one Neo4j transaction, and stages session-local schema until commit. It does not auto-create schema from data writes. If GRM_SCHEMA_TEMPLATE recovered schema memory at startup, omit schema_define_* ops only when grm_schema_list already shows the needed models and fields. New schema definitions are persisted to the local schema memory file when configured.",
            "before_calling": [
                "Use this for more than 3 creates or updates.",
                "In Neo4j mode, read grm://backend/status and call grm_schema_list first; recovered schema memory may already contain the needed schema metadata.",
                "Define referenced models before creating nodes or edges.",
                "Use ref on node_create operations when later edge_create operations should refer to those new nodes.",
                "Set allow_deletes=true only when the batch intentionally includes node_delete or edge_delete operations."
            ],
            "endpoint_rules": [
                "edge_create from and to may be numeric node ids already known to the caller.",
                "edge_create from and to may be refs from earlier node_create operations in the same batch.",
                "Refs are batch-local, are only produced by node_create operations, and must be unique within the batch."
            ],
            "example": {
                "atomic": true,
                "response": "detailed",
                "ops": [
                    {
                        "op": "node_create",
                        "args": {
                            "ref": "file:src/lib.rs",
                            "model": "File",
                            "props": { "path": "src/lib.rs" }
                        }
                    }
                ]
            },
            "result_shape": {
                "summary": ["applied", "atomic", "operation_count", "counts", "errors"],
                "detailed_adds": ["ids"],
                "counts": "Grouped by operation and model.",
                "errors": "Each error includes the failing operation index, message, and recovery hint."
            },
            "common_errors": [
                recovery("unknown field", "Call grm_schema_list and use only declared fields."),
                recovery("was not created earlier in this batch", "Create the referenced node earlier in ops or use a numeric id."),
                recovery("duplicate batch ref", "Use a unique ref for each node_create operation in the batch."),
                recovery("requires allow_deletes=true", "Set allow_deletes=true when the batch intentionally includes delete operations."),
                recovery("missing required field", "Provide all required fields from the schema."),
                recovery("invalid type: string, expected adjacently tagged enum SessionBatchOp", "Pass each ops entry as a JSON object with op and args fields, not as a JSON-encoded string.")
            ],
            "related": ["grm_schema_list", "grm_node_create", "grm_edge_create"]
        }),
        "grm_schema_define_edge" => json!({
            "tool": "grm_schema_define_edge",
            "purpose": "Define a runtime edge/link model between existing node models.",
            "before_calling": ["Define the from_model and to_model node models first."],
            "modeling_guidance": [
                "Decide richness vs sparseness before defining many similar edge models.",
                "Use richer, specific edge models when relationships have different meanings, fields, constraints, or traversal semantics.",
                "Use a sparser, broader edge model with a kind/type/category field when relationships share meaning and differ mostly by property values."
            ],
            "example": {
                "name": "CONTAINS",
                "from_model": "File",
                "to_model": "RustItem",
                "id_field": "containsId",
                "fields": []
            },
            "common_errors": [
                recovery("from model", "Call grm_schema_list and define or correct from_model."),
                recovery("to model", "Call grm_schema_list and define or correct to_model."),
                recovery("model already exists", "Choose a new edge model name or reuse the existing one.")
            ],
            "related": ["grm_schema_define_node", "grm_edge_create"]
        }),
        "grm_node_create" => json!({
            "tool": "grm_node_create",
            "purpose": "Create a node for an existing runtime model.",
            "batching_note": "For more than 3 creates or updates, prefer grm_batch.",
            "before_calling": ["Call grm_schema_list if you do not know the model fields."],
            "example": {
                "model": "File",
                "props": { "path": "grm-mcp/src/tools.rs", "summary": "MCP tool handlers" }
            },
            "common_errors": [
                recovery("unknown field", "Call grm_schema_list and remove or rename fields not declared on the model."),
                recovery("missing required field", "Call grm_schema_list and provide all required fields."),
                recovery("expected int value", "Send numeric fields as numbers or numeric strings."),
                recovery("null is not a supported graph value", "Omit optional fields instead of passing null.")
            ],
            "related": ["grm_schema_list", "grm_node_find", "grm_edge_create"]
        }),
        "grm_node_update" => json!({
            "tool": "grm_node_update",
            "purpose": "Update properties on an existing node.",
            "batching_note": "For more than 3 creates or updates, prefer grm_batch.",
            "example": {
                "model": "File",
                "id": 1,
                "props": { "summary": "Updated summary" }
            },
            "common_errors": [
                recovery("node id must be an int id", "Use the numeric id returned by grm_node_create or grm_node_find."),
                recovery("node was not found", "Call grm_node_find to locate the node before updating."),
                recovery("unknown field", "Call grm_schema_list and update only declared fields.")
            ],
            "related": ["grm_node_find", "grm_schema_list"]
        }),
        "grm_node_delete" => json!({
            "tool": "grm_node_delete",
            "purpose": "Delete an existing node by model and backend id.",
            "example": { "model": "File", "id": 1 },
            "common_errors": [
                recovery("node was not found", "Call grm_node_find to locate the current id."),
                recovery("does not match model", "Call grm_node_find with the expected model or correct the model name.")
            ],
            "related": ["grm_node_find", "grm://graph/summary"]
        }),
        "grm_node_find" => json!({
            "tool": "grm_node_find",
            "purpose": "Find nodes for a model using structured filters, paging, ordering, and traversal-shaped node.find controls.",
            "example": {
                "model": "File",
                "filters": { "path": "grm-mcp/src/tools.rs", "limit": 10 }
            },
            "traversal_example": {
                "model": "User",
                "filters": { "name": "Alice" },
                "via": ["out:AUTHORED:Post"],
                "end_filters": { "title~": "Graph" },
                "edge_filters": { "year>=": 2024 },
                "return": "end",
                "order": "title:asc",
                "limit": 5,
                "offset": 0
            },
            "filter_syntax": [
                "Use field names for equality, for example {\"name\":\"Alice\"}.",
                "Use operator suffixes for comparisons, for example age>=, age<, title~.",
                "Use id or the model id_field for backend id equality only.",
                "Use top-level limit, offset, and order for paging and ordering.",
                "Use via entries formatted as <out|in|both>:<LinkName|*>:<EndModel> for traversal steps.",
                "Use end_filters and edge_filters for predicates on the returned end node and traversed edge.",
                "Use return=root, return=end, or return=edge for traversal result shape in local and gRPC service modes."
            ],
            "common_errors": [
                recovery("unknown field", "Call grm_schema_list and use a declared field."),
                recovery("backend id filter", "Use id equality only; comparison operators are not supported for id filters."),
                recovery("return=edge", "Use a traversal-shaped grm_node_find request; Neo4j MCP mode still supports simple node finds only."),
                recovery("invalid query term", "Use structured grm_node_find fields for supported traversals; call grm_tool_help for grm_query only when exact CLI-compatible text is required.")
            ],
            "related": ["grm_schema_list", "grm_query", "grm://docs/query-language"]
        }),
        "grm_edge_create" => json!({
            "tool": "grm_edge_create",
            "purpose": "Create an edge between two existing node ids.",
            "batching_note": "For more than 3 creates or updates, prefer grm_batch.",
            "before_calling": ["Call grm_schema_list to confirm from_model and to_model.", "Call grm_node_find if you do not know endpoint ids."],
            "example": {
                "model": "CONTAINS",
                "from": 1,
                "to": 2,
                "props": {}
            },
            "common_errors": [
                recovery("from node", "Call grm_node_find to locate a valid from id."),
                recovery("to node", "Call grm_node_find to locate a valid to id."),
                recovery("does not match model", "Check the edge model's from_model/to_model in grm_schema_list."),
                recovery("missing required field", "Provide required edge properties from the schema.")
            ],
            "related": ["grm_schema_list", "grm_node_find", "grm_edge_find"]
        }),
        "grm_edge_update" => json!({
            "tool": "grm_edge_update",
            "purpose": "Update properties on an existing edge.",
            "batching_note": "For more than 3 creates or updates, prefer grm_batch.",
            "example": { "model": "CONTAINS", "id": 1, "props": {} },
            "common_errors": [
                recovery("edge was not found", "Call grm_edge_find to locate the current edge id."),
                recovery("unknown field", "Call grm_schema_list and update only declared edge fields.")
            ],
            "related": ["grm_edge_find", "grm_schema_list"]
        }),
        "grm_edge_delete" => json!({
            "tool": "grm_edge_delete",
            "purpose": "Delete an existing edge by model and backend id.",
            "example": { "model": "CONTAINS", "id": 1 },
            "common_errors": [
                recovery("edge was not found", "Call grm_edge_find to locate the current id.")
            ],
            "related": ["grm_edge_find", "grm://graph/summary"]
        }),
        "grm_edge_find" => json!({
            "tool": "grm_edge_find",
            "purpose": "Find edges for a model using endpoint and property filters.",
            "example": {
                "model": "CONTAINS",
                "filters": { "from": 1, "limit": 10 }
            },
            "filter_syntax": [
                "Use from and to for endpoint id equality.",
                "Use id or the edge id_field for backend edge id equality.",
                "Use field names and operator suffixes for declared edge properties.",
                "Use limit, offset, and order for paging and ordering."
            ],
            "common_errors": [
                recovery("special filter", "Use equality only for id, from, and to."),
                recovery("unknown field", "Call grm_schema_list and use declared edge fields.")
            ],
            "related": ["grm_schema_list", "grm_edge_create"]
        }),
        "grm_query" => json!({
            "tool": "grm_query",
            "purpose": "Run one CLI-compatible GRM session command. Best for traversal queries.",
            "example": {
                "command": "node.find User name=\"Alice Jones\" via=out:AUTHORED:Post return=end"
            },
            "query_notes": [
                "Traversal uses via=<out|in|both>:<LinkName|*>:<EndModel>.",
                "Use end.<field> filters for the final node in a traversal.",
                "Use edge.<field> or rel.<field> filters for the final traversed edge.",
                "Use return=root, return=end, or return=edge with traversal queries."
            ],
            "common_errors": [
                recovery("Unknown command", "Use grm_node_find, grm_edge_find, or a documented session command."),
                recovery("traversal filters require at least one via", "Add a via= traversal or remove end./edge. filters."),
                recovery("graph format is only supported", "Use format=graph only for traversal-shaped queries.")
            ],
            "related": ["grm_explain", "grm_profile", "grm://docs/query-language", "grm_node_find", "grm_edge_find"]
        }),
        "grm_explain" => json!({
            "tool": "grm_explain",
            "purpose": "Return the current logical plan for a CLI-compatible node.find or edge.find command without running it.",
            "example": {
                "command": "node.find User name=\"Alice Jones\" via=out:AUTHORED:Post"
            },
            "result_shape": {
                "command": "node.find or edge.find",
                "target": "Model or link name",
                "plan": ["steps", "text"]
            },
            "common_errors": [
                recovery("expected command", "Pass node.find <ModelName> [terms...] or edge.find <LinkName> [terms...]."),
                recovery("format= is not supported", "Remove format=; introspection results are structured JSON."),
                recovery("unknown field", "Call grm_schema_list and use declared fields.")
            ],
            "related": ["grm_profile", "grm_query", "grm://docs/query-language"]
        }),
        "grm_profile" => json!({
            "tool": "grm_profile",
            "purpose": "Run a CLI-compatible node.find or edge.find query and return the plan, row count, and elapsed time.",
            "example": {
                "command": "edge.find AUTHORED from=1"
            },
            "result_shape": {
                "command": "node.find or edge.find",
                "target": "Model or link name",
                "plan": ["steps", "text"],
                "result_rows": "Number of rows returned by the query path.",
                "elapsed": ["micros", "display"],
                "per_step_metrics": [{
                    "step_index": 0,
                    "kind": "RelationshipEndpointSeek",
                    "access_path": "outgoing_adjacency",
                    "input_rows": 0,
                    "output_rows": 1,
                    "elapsed_micros": 42
                }]
            },
            "common_errors": [
                recovery("expected command", "Pass node.find <ModelName> [terms...] or edge.find <LinkName> [terms...]."),
                recovery("format= is not supported", "Remove format=; profile results are structured JSON."),
                recovery("unknown field", "Call grm_schema_list and use declared fields.")
            ],
            "related": ["grm_explain", "grm_query", "grm://docs/query-language"]
        }),
        "grm_save" => json!({
            "tool": "grm_save",
            "purpose": "Save the current runtime session snapshot to JSON or binary.",
            "example": { "format": "json", "path": "session.json" },
            "common_errors": [
                recovery("failed to write", "Check that the target directory exists and is writable.")
            ],
            "related": ["grm_load", "grm_export"]
        }),
        "grm_load" => json!({
            "tool": "grm_load",
            "purpose": "Load a GRM runtime session snapshot from JSON or binary.",
            "example": { "format": "json", "path": "session.json" },
            "common_errors": [
                recovery("failed to read", "Check the path and format."),
                recovery("failed to deserialize", "Use grm_import for interchange exports; grm_load expects session snapshots.")
            ],
            "related": ["grm_save", "grm_import"]
        }),
        "grm_import" => json!({
            "tool": "grm_import",
            "purpose": "Import a GRM interchange JSON document into an empty session.",
            "example": { "path": "graph.export.json" },
            "common_errors": [
                recovery("requires an empty session", "Start a fresh server process or avoid importing into an existing session."),
                recovery("unsupported import", "Confirm the document has format grm.interchange, version 1, and kind graph.")
            ],
            "related": ["grm_export", "grm://graph/export"]
        }),
        "grm_export" => json!({
            "tool": "grm_export",
            "purpose": "Return the current graph as interchange JSON, optionally writing it to a path.",
            "example": { "path": null },
            "common_errors": [
                recovery("failed to write", "Check that the target directory exists and is writable.")
            ],
            "related": ["grm://graph/export", "grm://graph/summary"]
        }),
        _ => return None,
    };

    Some(help)
}

pub fn tool_help_index() -> Value {
    let tools = known_tools()
        .into_iter()
        .filter_map(tool_help)
        .collect::<Vec<_>>();
    json!({ "tools": tools })
}

pub fn known_tools() -> Vec<&'static str> {
    vec![
        "grm_help",
        "grm_tool_help",
        "grm_schema_list",
        "grm_schema_checkpoint",
        "grm_index_list",
        "grm_batch",
        "grm_schema_define_node",
        "grm_schema_define_edge",
        "grm_node_create",
        "grm_node_update",
        "grm_node_delete",
        "grm_node_find",
        "grm_edge_create",
        "grm_edge_update",
        "grm_edge_delete",
        "grm_edge_find",
        "grm_explain",
        "grm_profile",
        "grm_query",
        "grm_save",
        "grm_load",
        "grm_import",
        "grm_export",
    ]
}

fn recovery(message_contains: &str, recovery: &str) -> Value {
    json!({
        "message_contains": message_contains,
        "recovery": recovery,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_index_names_agent_guide_resource() {
        let help = help_index();
        assert!(
            help["resources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|resource| resource == "grm://docs/agent-guide")
        );
    }

    #[test]
    fn node_create_help_mentions_schema_recovery() {
        let help = tool_help("grm_node_create").unwrap();
        assert!(
            help.to_string().contains("grm_schema_list"),
            "node create help should guide agents back to schema"
        );
    }

    #[test]
    fn node_find_help_describes_structured_traversal_controls() {
        let help = tool_help("grm_node_find").unwrap();
        let rendered = help.to_string();
        assert!(rendered.contains("via"));
        assert!(rendered.contains("end_filters"));
        assert!(rendered.contains("edge_filters"));
        assert!(rendered.contains("return=edge"));
    }

    #[test]
    fn help_surfaces_richness_vs_sparseness_modeling_guidance() {
        let help = help_index();
        assert!(
            help.to_string().contains("richness vs sparseness"),
            "top-level help should guide agents on schema granularity"
        );

        let batch_help = tool_help("grm_batch").unwrap();
        assert!(
            batch_help.to_string().contains("richness vs sparseness"),
            "batch help should remind agents to choose schema granularity before batching"
        );
    }

    #[test]
    fn help_explains_neo4j_schema_memory_startup_flow() {
        let help = help_index();
        assert!(
            help.to_string().contains("GRM_SCHEMA_TEMPLATE"),
            "top-level help should explain Neo4j schema memory startup config"
        );

        let schema_help = tool_help("grm_schema_list").unwrap();
        assert!(
            schema_help
                .to_string()
                .contains("schema_template_persistence_enabled"),
            "schema help should tell agents how to interpret schema memory"
        );
    }

    #[test]
    fn batch_help_warns_ops_are_objects_not_strings() {
        let help = tool_help("grm_batch").unwrap();
        let rendered = help.to_string();
        assert!(
            rendered.contains("operation objects"),
            "batch help should tell agents that ops entries are structured objects"
        );
        assert!(
            rendered.contains("JSON-encoded string"),
            "batch help should prevent agents from passing serialized JSON strings"
        );
    }
}
