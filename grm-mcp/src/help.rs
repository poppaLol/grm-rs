use serde_json::{Value, json};

pub const AGENT_GUIDE: &str = r#"GRM is a runtime graph session exposed over MCP.

Recommended agent workflow:
1. Call grm_help when first using this server in a session.
2. Call grm_schema_list or read grm://schema before creating or querying data.
3. Prefer structured tools for schema, node, edge, import, export, and persistence operations.
4. For more than 3 creates or updates, prefer grm_batch.
5. Use grm_query for traversal queries or exact CLI parity.
6. After any tool error you cannot immediately fix, call grm_tool_help for that tool.
7. Verify writes with grm://graph/summary, grm://graph/export, or grm_export.

Property values must be strings, numbers, or booleans. Null, arrays, and objects are not supported as graph property values.
"#;

pub fn help_index() -> Value {
    json!({
        "server": "grm-mcp",
        "purpose": "Expose a local grm-rs runtime graph session to agents.",
        "recommended_workflow": [
            "Call grm_help when first using this server in a session.",
            "Call grm_schema_list or read grm://schema before creating or querying data.",
            "Prefer structured tools over grm_query except for traversal queries or CLI parity.",
            "For more than 3 creates or updates, prefer grm_batch.",
            "After recoverable errors, call grm_tool_help with the tool name before retrying.",
            "Verify writes with grm://graph/summary, grm://graph/export, or grm_export."
        ],
        "resources": [
            "grm://docs/agent-guide",
            "grm://docs/query-language",
            "grm://docs/tool-help",
            "grm://schema",
            "grm://graph/summary",
            "grm://graph/export"
        ],
        "tool_categories": {
            "help": ["grm_help", "grm_tool_help"],
            "schema": ["grm_schema_list", "grm_schema_define_node", "grm_schema_define_edge"],
            "batch": ["grm_batch"],
            "nodes": ["grm_node_create", "grm_node_update", "grm_node_delete", "grm_node_find"],
            "edges": ["grm_edge_create", "grm_edge_update", "grm_edge_delete", "grm_edge_find"],
            "query": ["grm_query"],
            "persistence": ["grm_save", "grm_load", "grm_import", "grm_export"]
        },
        "value_rules": [
            "Graph property values may be strings, numbers, or booleans.",
            "Null, arrays, and objects are rejected as graph property values.",
            "Required fields must be supplied when creating nodes or edges.",
            "Only fields declared in the runtime schema may be supplied."
        ],
        "when_to_use_grm_query": [
            "Use grm_query for traversal syntax such as via=out:Authored:Post.",
            "Use grm_query when you want exact CLI-compatible behavior.",
            "Prefer grm_node_find and grm_edge_find for simple model/filter lookups."
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
            "example": {},
            "related": ["grm://schema", "grm_help"]
        }),
        "grm_schema_define_node" => json!({
            "tool": "grm_schema_define_node",
            "purpose": "Define a runtime node model.",
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
            "before_calling": ["Use this for more than 3 creates or updates.", "Use ref on node_create operations when later edge_create operations should refer to created nodes."],
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
            "common_errors": [
                recovery("unknown field", "Call grm_schema_list and use only declared fields."),
                recovery("was not created earlier in this batch", "Create the referenced node earlier in ops or use a numeric id."),
                recovery("missing required field", "Provide all required fields from the schema.")
            ],
            "related": ["grm_schema_list", "grm_node_create", "grm_edge_create"]
        }),
        "grm_schema_define_edge" => json!({
            "tool": "grm_schema_define_edge",
            "purpose": "Define a runtime edge/link model between existing node models.",
            "before_calling": ["Define the from_model and to_model node models first."],
            "example": {
                "name": "Contains",
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
            "purpose": "Find nodes for a model using GRM query filter terms.",
            "example": {
                "model": "File",
                "filters": { "path": "grm-mcp/src/tools.rs", "limit": 10 }
            },
            "filter_syntax": [
                "Use field names for equality, for example {\"name\":\"Alice\"}.",
                "Use operator suffixes for comparisons, for example age>=, age<, title~.",
                "Use id or the model id_field for backend id equality only.",
                "Use limit, offset, and order for paging and ordering."
            ],
            "common_errors": [
                recovery("unknown field", "Call grm_schema_list and use a declared field."),
                recovery("backend id filter", "Use id equality only; comparison operators are not supported for id filters."),
                recovery("invalid query term", "Call grm_tool_help for grm_query or grm://docs/query-language if using advanced syntax.")
            ],
            "related": ["grm_schema_list", "grm_query", "grm://docs/query-language"]
        }),
        "grm_edge_create" => json!({
            "tool": "grm_edge_create",
            "purpose": "Create an edge between two existing node ids.",
            "batching_note": "For more than 3 creates or updates, prefer grm_batch.",
            "before_calling": ["Call grm_schema_list to confirm from_model and to_model.", "Call grm_node_find if you do not know endpoint ids."],
            "example": {
                "model": "Contains",
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
            "example": { "model": "Contains", "id": 1, "props": {} },
            "common_errors": [
                recovery("edge was not found", "Call grm_edge_find to locate the current edge id."),
                recovery("unknown field", "Call grm_schema_list and update only declared edge fields.")
            ],
            "related": ["grm_edge_find", "grm_schema_list"]
        }),
        "grm_edge_delete" => json!({
            "tool": "grm_edge_delete",
            "purpose": "Delete an existing edge by model and backend id.",
            "example": { "model": "Contains", "id": 1 },
            "common_errors": [
                recovery("edge was not found", "Call grm_edge_find to locate the current id.")
            ],
            "related": ["grm_edge_find", "grm://graph/summary"]
        }),
        "grm_edge_find" => json!({
            "tool": "grm_edge_find",
            "purpose": "Find edges for a model using endpoint and property filters.",
            "example": {
                "model": "Contains",
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
                "command": "node.find User name=\"Alice Jones\" via=out:Authored:Post return=end"
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
            "related": ["grm://docs/query-language", "grm_node_find", "grm_edge_find"]
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
        .filter_map(|tool| tool_help(tool))
        .collect::<Vec<_>>();
    json!({ "tools": tools })
}

pub fn known_tools() -> Vec<&'static str> {
    vec![
        "grm_help",
        "grm_tool_help",
        "grm_schema_list",
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
}
