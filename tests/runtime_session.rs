use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Write};

use grm_rs::{
    BackendIdType, BatchRequest, CliSession, DefineEdgeRequest, DefineNodeRequest,
    DurabilityFormat, DurableOperation, EdgeFindRequest, EdgeRequest, EdgeResponse, ExplainRequest,
    FieldSpec, FieldValueType, GraphTx, NodeFindRequest, NodeRequest, NodeResponse, PredicateOp,
    ProfileRequest, PropertyPredicate, QueryRequest, QueryTerm, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeRequest, RuntimeResponse, RuntimeValueType, SchemaRequest,
    SchemaResponse, SessionBatchResponse, SessionFindResult, SessionModelCatalog, SessionState,
    TraversalDirection, TraversalReturn, TraversalStepRequest,
};
use serde_json::{Value, json};

#[test]
fn session_catalog_starts_empty() {
    let state = SessionState::new();
    assert!(state.catalog().is_empty());
}

#[test]
fn registering_valid_model_works() {
    let mut catalog = SessionModelCatalog::new();
    let model = RuntimeNodeModel::new(
        "User",
        "userId",
        BackendIdType::Int64,
        vec![
            RuntimeField {
                name: "name".into(),
                value_type: RuntimeValueType::String,
                required: true,
            },
            RuntimeField {
                name: "age".into(),
                value_type: RuntimeValueType::Int,
                required: false,
            },
        ],
    )
    .unwrap();

    catalog.register(model.clone()).unwrap();
    let stored = catalog.get("User").unwrap();
    assert_eq!(stored.label, "User");
    assert_eq!(stored.id_field_name, "userId");
    assert_eq!(stored.id_type, BackendIdType::Int64);
    assert_eq!(stored.fields, model.fields);
}

#[test]
fn registering_valid_relationship_model_works() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap(),
        )
        .unwrap();
    state
        .register_model(
            RuntimeNodeModel::new("Post", "postId", BackendIdType::Int64, vec![]).unwrap(),
        )
        .unwrap();

    let model = RuntimeRelModel::new(
        "Authored",
        "User",
        "Post",
        "authoredId",
        BackendIdType::Int64,
        vec![RuntimeField {
            name: "year".into(),
            value_type: RuntimeValueType::Int,
            required: true,
        }],
    )
    .unwrap();

    state.register_rel_model(model.clone()).unwrap();
    let stored = state.rel_model("Authored").unwrap();
    assert_eq!(stored.rel_type, "Authored");
    assert_eq!(stored.from_model, "User");
    assert_eq!(stored.to_model, "Post");
    assert_eq!(stored.id_field_name, "authoredId");
    assert_eq!(stored.fields, model.fields);
}

#[test]
fn model_name_collisions_are_rejected() {
    let mut catalog = SessionModelCatalog::new();
    let model = RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap();
    catalog.register(model.clone()).unwrap();

    let err = catalog.register(model).unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn relationship_models_require_existing_endpoint_models() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap(),
        )
        .unwrap();

    let model = RuntimeRelModel::new(
        "Authored",
        "User",
        "Post",
        "authoredId",
        BackendIdType::Int64,
        vec![],
    )
    .unwrap();

    let err = state.register_rel_model(model).unwrap_err();
    assert!(err.to_string().contains("to model 'Post'"));
}

#[test]
fn invalid_model_names_are_rejected() {
    let err = RuntimeNodeModel::new("user", "userId", BackendIdType::Int64, vec![]).unwrap_err();
    assert!(err.to_string().contains("PascalCase"));
}

#[test]
fn duplicate_and_reserved_fields_are_rejected() {
    let reserved = RuntimeNodeModel::new(
        "User",
        "userId",
        BackendIdType::Int64,
        vec![RuntimeField {
            name: "id".into(),
            value_type: RuntimeValueType::String,
            required: true,
        }],
    )
    .unwrap_err();
    assert!(reserved.to_string().contains("reserved"));

    let duplicate = RuntimeNodeModel::new(
        "User",
        "userId",
        BackendIdType::Int64,
        vec![
            RuntimeField {
                name: "name".into(),
                value_type: RuntimeValueType::String,
                required: true,
            },
            RuntimeField {
                name: "name".into(),
                value_type: RuntimeValueType::String,
                required: false,
            },
        ],
    )
    .unwrap_err();
    assert!(duplicate.to_string().contains("more than once"));
}

#[test]
fn id_field_name_is_validated_and_reserved_against_properties() {
    let reserved = RuntimeNodeModel::new("User", "id", BackendIdType::Int64, vec![]).unwrap_err();
    assert!(reserved.to_string().contains("reserved"));

    let duplicate = RuntimeNodeModel::new(
        "User",
        "userId",
        BackendIdType::Int64,
        vec![RuntimeField {
            name: "userId".into(),
            value_type: RuntimeValueType::String,
            required: true,
        }],
    )
    .unwrap_err();
    assert!(duplicate.to_string().contains("more than once"));
}

#[test]
fn scalar_field_types_are_parsed() {
    assert_eq!(
        RuntimeValueType::parse_keyword("string"),
        Some(RuntimeValueType::String)
    );
    assert_eq!(
        RuntimeValueType::parse_keyword("int"),
        Some(RuntimeValueType::Int)
    );
    assert_eq!(
        RuntimeValueType::parse_keyword("float"),
        Some(RuntimeValueType::Float)
    );
    assert_eq!(
        RuntimeValueType::parse_keyword("bool"),
        Some(RuntimeValueType::Bool)
    );
}

#[tokio::test]
async fn instance_validation_rejects_missing_type_mismatch_and_unknown_fields() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new(
                "User",
                "userId",
                BackendIdType::Int64,
                vec![
                    RuntimeField {
                        name: "name".into(),
                        value_type: RuntimeValueType::String,
                        required: true,
                    },
                    RuntimeField {
                        name: "age".into(),
                        value_type: RuntimeValueType::Int,
                        required: false,
                    },
                ],
            )
            .unwrap(),
        )
        .unwrap();

    let missing = BTreeMap::new();
    let err = state.create_instance("User", &missing).await.unwrap_err();
    assert!(err.to_string().contains("missing required field"));

    let mut wrong_type = BTreeMap::new();
    wrong_type.insert("name".into(), "Alice".into());
    wrong_type.insert("age".into(), "not-a-number".into());
    let err = state
        .create_instance("User", &wrong_type)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("expected int"));

    let mut unknown = BTreeMap::new();
    unknown.insert("name".into(), "Alice".into());
    unknown.insert("nickname".into(), "Al".into());
    let err = state.create_instance("User", &unknown).await.unwrap_err();
    assert!(err.to_string().contains("unknown field"));
}

#[tokio::test]
async fn successful_instance_creation_writes_expected_node() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new(
                "User",
                "userId",
                BackendIdType::Int64,
                vec![
                    RuntimeField {
                        name: "name".into(),
                        value_type: RuntimeValueType::String,
                        required: true,
                    },
                    RuntimeField {
                        name: "active".into(),
                        value_type: RuntimeValueType::Bool,
                        required: false,
                    },
                ],
            )
            .unwrap(),
        )
        .unwrap();

    let mut input = BTreeMap::new();
    input.insert("name".into(), "Alice".into());
    let created = state.create_instance("User", &input).await.unwrap();

    assert_eq!(created.labels, vec!["User".to_string()]);
    assert_eq!(created.props.len(), 1);
    assert_eq!(created.props.get("name").unwrap(), "Alice");
    assert_eq!(state.node_id_type(), BackendIdType::Int64);
}

#[tokio::test]
async fn successful_relationship_creation_writes_expected_rel() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new(
                "User",
                "userId",
                BackendIdType::Int64,
                vec![RuntimeField {
                    name: "name".into(),
                    value_type: RuntimeValueType::String,
                    required: true,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_model(
            RuntimeNodeModel::new(
                "Post",
                "postId",
                BackendIdType::Int64,
                vec![RuntimeField {
                    name: "title".into(),
                    value_type: RuntimeValueType::String,
                    required: true,
                }],
            )
            .unwrap(),
        )
        .unwrap();
    state
        .register_rel_model(
            RuntimeRelModel::new(
                "Authored",
                "User",
                "Post",
                "authoredId",
                BackendIdType::Int64,
                vec![RuntimeField {
                    name: "year".into(),
                    value_type: RuntimeValueType::Int,
                    required: true,
                }],
            )
            .unwrap(),
        )
        .unwrap();

    let mut user_input = BTreeMap::new();
    user_input.insert("name".into(), "Alice".into());
    let user = state.create_instance("User", &user_input).await.unwrap();

    let mut post_input = BTreeMap::new();
    post_input.insert("title".into(), "Hello".into());
    let post = state.create_instance("Post", &post_input).await.unwrap();

    let mut rel_input = BTreeMap::new();
    rel_input.insert("year".into(), "2024".into());
    let rel = state
        .create_relationship_instance(
            "Authored",
            &user.id.to_string(),
            &post.id.to_string(),
            &rel_input,
        )
        .await
        .unwrap();

    assert_eq!(rel.rel_type, "Authored");
    assert_eq!(rel.from, user.id);
    assert_eq!(rel.to, post.id);
    assert_eq!(rel.props.get("year").unwrap(), 2024);
}

#[tokio::test]
async fn relationship_creation_rejects_wrong_endpoint_models() {
    let mut state = SessionState::new();
    state
        .register_model(
            RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap(),
        )
        .unwrap();
    state
        .register_model(
            RuntimeNodeModel::new("Post", "postId", BackendIdType::Int64, vec![]).unwrap(),
        )
        .unwrap();
    state
        .register_rel_model(
            RuntimeRelModel::new(
                "Authored",
                "User",
                "Post",
                "authoredId",
                BackendIdType::Int64,
                vec![],
            )
            .unwrap(),
        )
        .unwrap();

    let user = state
        .create_instance("User", &BTreeMap::new())
        .await
        .unwrap();
    let wrong_to = state
        .create_instance("User", &BTreeMap::new())
        .await
        .unwrap();

    let err = state
        .create_relationship_instance(
            "Authored",
            &user.id.to_string(),
            &wrong_to.id.to_string(),
            &BTreeMap::new(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("does not match model 'Post'"));
}

#[tokio::test]
async fn guided_model_creation_and_listing_work() {
    let input = Cursor::new(
        "model.define\nUser\nuserId\nname\nstring\ny\nage\nint\nn\ndone\ny\nn\nmodel.list\nmodel.show User\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.model("User").is_some());
    assert!(output.contains("Model 'User' created."));
    assert!(output.contains("Session models:"));
    assert!(output.contains("Id: userId (int)"));
    assert!(output.contains("name: string (required)"));
    assert!(output.contains("age: int (optional)"));
}

#[tokio::test]
async fn canceling_confirmation_does_not_register_model() {
    let input = Cursor::new("model.define\nUser\nuserId\nname\nstring\ny\ndone\nn\nsession.exit\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.model("User").is_none());
    assert!(output.contains("Model creation canceled."));
}

#[tokio::test]
async fn choosing_first_instance_launches_creation_flow() {
    let input = Cursor::new(
        "model.define\nUser\nuserId\nname\nstring\ny\ndone\ny\ny\nAlice\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Creating instance of 'User'."));
    assert!(output.contains("Created node"));
    assert!(output.contains("userId="));
}

#[tokio::test]
async fn script_mode_can_define_models() {
    let input = Cursor::new(
        "# setup models\n\nmodel.define User userId name:string:required age:int:optional\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nmodel.list\nmodel.show User\nlink.list\nlink.show Authored\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    let model = state.model("User").unwrap();
    assert_eq!(model.id_field_name, "userId");
    assert_eq!(model.fields.len(), 2);
    let rel_model = state.rel_model("Authored").unwrap();
    assert_eq!(rel_model.from_model, "User");
    assert_eq!(rel_model.to_model, "Post");
    assert!(output.contains("Welcome to GRM-RS CLI."));
    assert!(output.contains("Script Summary"));
    assert!(output.contains("Types created:"));
    assert!(output.contains("nodes: User, Post"));
    assert!(output.contains("links: Authored"));
    assert!(output.contains("Inserted rows:"));
    assert!(output.contains("  none"));
}

#[tokio::test]
async fn script_mode_supports_inline_comments() {
    let input = Cursor::new(
        "# setup models\nmodel.define User userId name:string:required # primary entity\nnode.create User name=Alice # seed row\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.model("User").is_some());
    assert!(output.contains("Script Summary"));
    assert!(output.contains("| node |"));
}

#[tokio::test]
async fn script_mode_keeps_hash_inside_quoted_values() {
    let input = Cursor::new(
        "model.define User userId name:string:required bio:string:optional\nnode.create User name=Alice bio=\"likes #graphs\"\n",
    );
    let output = Vec::new();
    let mut script_session = CliSession::new(input, output);

    script_session.run_script().await.unwrap();

    let (state, _, output) = script_session.into_parts();
    let interactive_input = Cursor::new("node.find User bio~\"#graphs\"\nsession.exit\n");
    let mut interactive_session = CliSession::with_state(state, interactive_input, output);

    interactive_session.continue_interactive().await.unwrap();

    let (_, _, output) = interactive_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("#graphs"));
}

#[tokio::test]
async fn script_mode_outputs_colored_summary() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId authoredOn:string:required\nnode.create User name=\"Alice Jones\"\nnode.create Post title=\"Graph Notes\"\nedge.create Authored from=1 to=2 authoredOn=2026-04-12\nnode.find User name=\"Alice Jones\"\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new_with_color(input, output, true);

    session.run_script().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Welcome to GRM-RS CLI."));
    assert!(output.contains("nodes: \u{1b}[32mUser\u{1b}[0m, \u{1b}[32mPost\u{1b}[0m"));
    assert!(output.contains("links: \u{1b}[32mAuthored\u{1b}[0m"));
    assert!(output.contains("| node |"));
    assert!(output.contains("| edge |"));
    assert!(output.contains("\u{1b}[32mUser\u{1b}[0m"));
    assert!(output.contains("\u{1b}[32mPost\u{1b}[0m"));
    assert!(output.contains("\u{1b}[32mAuthored\u{1b}[0m"));
    assert!(output.contains("\u{1b}[34minserted\u{1b}[0m"));
    assert!(!output.contains("Node \u{1b}[32mUser\u{1b}[0m"));
}

#[tokio::test]
async fn script_mode_supports_let_bound_node_refs_for_edges() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nlet alice = node.create User name=Alice\nlet hello = node.create Post title=Hello\nedge.create Authored from=alice to=hello year=2026\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let edges = state
        .find_relationships(
            "Authored",
            &BTreeMap::from([("year".to_string(), "2026".to_string())]),
        )
        .unwrap();

    assert_eq!(edges.len(), 1);
    assert!(output.contains("| edge |"));
}

#[tokio::test]
async fn script_mode_reports_unknown_edge_binding() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nlet alice = node.create User name=Alice\nedge.create Authored from=alice to=missing year=2026\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();

    assert!(err.to_string().contains("unknown binding 'missing'"));
}

#[tokio::test]
async fn script_mode_rejects_duplicate_binding_before_create() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nlet alice = node.create User name=Alice\nlet alice = node.create User name=Bob\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();

    assert!(
        err.to_string()
            .contains("binding 'alice' is already defined")
    );
    assert_eq!(
        session
            .state()
            .find_nodes(
                "User",
                &BTreeMap::from([("name".to_string(), "Bob".to_string())]),
            )
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn script_transaction_commit_keeps_changes() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\ntx.begin\nlet alice = node.create User name=Alice\nlet hello = node.create Post title=Hello\nedge.create Authored from=alice to=hello year=2026\ntx.commit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await.unwrap();

    assert_eq!(
        session
            .state()
            .find_relationships(
                "Authored",
                &BTreeMap::from([("year".to_string(), "2026".to_string())]),
            )
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn script_transaction_error_rolls_back_changes() {
    let input = Cursor::new(
        "model.define User userId name:string:required\ntx.begin\nlet alice = node.create User name=Alice\nnode.create User\ntx.commit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();

    assert!(err.to_string().contains("missing required field"));
    assert_eq!(
        session
            .state()
            .find_nodes(
                "User",
                &BTreeMap::from([("name".to_string(), "Alice".to_string())]),
            )
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn script_transaction_rejects_nested_begin() {
    let input = Cursor::new("tx.begin\ntx.begin\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();

    assert!(err.to_string().contains("transaction is already open"));
}

#[tokio::test]
async fn script_transaction_rejects_unclosed_transaction() {
    let input = Cursor::new("tx.begin\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();

    assert!(
        err.to_string()
            .contains("script ended with an open transaction")
    );
}

#[tokio::test]
async fn script_mode_rejects_bad_field_specs() {
    let input = Cursor::new("model.define User userId name:string:maybe\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();
    assert!(err.to_string().contains("invalid field requirement"));
}

#[tokio::test]
async fn script_bootstrap_can_continue_interactively() {
    let script_input = Cursor::new("model.define User userId name:string:required\n");
    let output = Vec::new();
    let mut script_session = CliSession::new(script_input, output);

    script_session.run_script().await.unwrap();

    let (state, _, output) = script_session.into_parts();
    let interactive_input = Cursor::new("model.show User\nsession.exit\n");
    let mut interactive_session = CliSession::with_state(state, interactive_input, output);

    interactive_session.continue_interactive().await.unwrap();

    let (state, _, output) = interactive_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.model("User").is_some());
    assert!(output.contains("Welcome to GRM-RS CLI."));
    assert!(output.contains("Script loaded. Entering interactive session."));
    assert!(output.contains("Model: User"));
    assert!(output.contains("Id: userId (int)"));
}

#[tokio::test]
async fn guided_relationship_model_creation_and_listing_work() {
    let input = Cursor::new(
        "model.define User userId\nmodel.define Post postId\nlink.define\nAuthored\nUser\nPost\nauthoredId\nyear\nint\ny\ndone\ny\nn\nlink.list\nlink.show Authored\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.rel_model("Authored").is_some());
    assert!(output.contains("Link 'Authored' created."));
    assert!(output.contains("Session links:"));
    assert!(output.contains("Type: Authored"));
    assert!(output.contains("From: User"));
    assert!(output.contains("To: Post"));
}

#[tokio::test]
async fn node_find_uses_dotted_query_syntax() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:optional\nnode.create User name=Alice age=42\nnode.create User name=Bob\nnode.find User name=Alice\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Node User userId=1 {age=42 name=Alice}"));
    assert!(!output.contains("userId=2 {name=Bob}"));
}

#[tokio::test]
async fn edge_find_uses_dotted_query_syntax() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nedge.find Authored from=1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Edge Authored authoredId=1 from=1 to=2 {year=2024}"));
}

#[tokio::test]
async fn node_find_supports_quoted_values_with_spaces() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=\"Alice Jones\"\nnode.create User name=Bob\nnode.find User name=\"Alice Jones\"\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 nodes matched model 'User'."));
    assert!(output.contains("Node User userId=1 {name=\"Alice Jones\"}"));
    assert!(!output.contains("userId=2 {name=Bob}"));
}

#[tokio::test]
async fn node_find_supports_line_continuation() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=\"Alice Jones\" age=42\nnode.find User \\\nname=\"Alice Jones\" \\\norder=age:desc\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 nodes matched model 'User'."));
    assert!(output.contains("Node User userId=1 {age=42 name=\"Alice Jones\"}"));
}

#[tokio::test]
async fn node_find_traverses_to_related_end_nodes() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required text:string:optional\nlink.define Authored User Post authoredId authoredOn:string:required\nnode.create User name=\"Alice Jones\"\nnode.create User name=\"Bob Smith\"\nnode.create Post title=\"Hello World\" text=\"A short welcome post.\"\nnode.create Post title=\"Draft Notes\" text=\"A quick draft.\"\nedge.create Authored from=1 to=3 authoredOn=2026-04-10\nedge.create Authored from=2 to=4 authoredOn=2026-04-12\nnode.find User name=\"Alice Jones\" via=out:Authored:Post\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 nodes matched model 'Post'."));
    assert!(output.contains("Node Post postId=3"));
    assert!(output.contains("title=\"Hello World\""));
    assert!(!output.contains("Node Post postId=4"));
}

#[tokio::test]
async fn node_find_traversal_supports_edge_filters_and_return_edge() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Accessed User Post accessedId accessedOn:string:required\nnode.create User name=\"Alice Jones\"\nnode.create Post title=\"Draft Notes\"\nnode.create Post title=\"Traversal Tips\"\nedge.create Accessed from=1 to=2 accessedOn=2026-04-20\nedge.create Accessed from=1 to=3 accessedOn=2026-04-22\nnode.find User name=\"Alice Jones\" via=out:Accessed:Post edge.accessedOn=2026-04-20 end.title=\"Draft Notes\" return=edge\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 edges matched link 'Accessed'."));
    assert!(output.contains("Edge Accessed accessedId=1 from=1 to=2 {accessedOn=2026-04-20}"));
    assert!(!output.contains("accessedId=2 from=1 to=3"));
}

#[tokio::test]
async fn session_explain_node_find_renders_flat_logical_plan_without_mutating() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.explain node.find User name=Alice\nnode.find User\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert_eq!(
        state.find_nodes("User", &Default::default()).unwrap().len(),
        1
    );
    assert!(output.contains("Current logical plan for node.find User"));
    assert!(output.contains("NodePropertySeek v0 User.name"));
    assert!(output.contains("Return Node v0"));
    assert!(output.contains("1 nodes matched model 'User'."));
}

#[tokio::test]
async fn session_explain_node_find_renders_traversal_logical_plan() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.explain node.find User name=Alice via=out:Authored:Post\nsession.profile --verbose node.find User name=Alice via=out:Authored:Post\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Current logical plan for node.find User"));
    assert!(output.contains("NodePropertySeek v0 User.name"));
    assert!(output.contains("ExpandOut v0 -[v1:Authored]-> v2"));
    assert!(output.contains("Return Node v2"));
    assert!(output.contains("rows_in=unknown rows_out=unknown elapsed=unknown"));
    assert!(output.contains("rows_in=unknown rows_out=1 elapsed=unknown"));
}

#[tokio::test]
async fn session_profile_node_find_reports_count_and_elapsed_time() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=Alice\nnode.create User name=Bob\nsession.profile node.find User name=Alice\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Profile for node.find User"));
    assert!(output.contains("NodePropertySeek v0 User.name"));
    assert!(output.contains("Result rows: 1"));
    assert!(output.contains("Elapsed: "));
    assert!(!output.contains("rows_in="));
}

#[tokio::test]
async fn session_state_explain_and_profile_return_structured_values() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=Alice\nnode.create User name=Bob\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, _) = session.into_parts();
    let terms = vec![QueryTerm {
        key: "name".to_string(),
        value: "Alice".to_string(),
    }];

    let explain = state.explain_node_find_terms("User", &terms).unwrap();
    assert_eq!(explain["command"], "node.find");
    assert_eq!(explain["target"], "User");
    assert!(
        explain["plan"]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap().contains("NodePropertySeek"))
    );

    let profile = state.profile_node_find_terms("User", &terms).await.unwrap();
    assert_eq!(profile["result_rows"], 1);
    assert!(profile["elapsed"]["micros"].as_u64().is_some());
    assert!(profile["elapsed"]["display"].as_str().is_some());
    assert!(profile["per_step_metrics"].as_array().unwrap().len() >= 2);
}

#[tokio::test]
async fn session_index_catalog_exposes_system_indexes() {
    let state = SessionState::new();
    let catalog = state.index_catalog_value();
    let indexes = catalog["indexes"].as_array().unwrap();

    assert!(indexes.iter().any(|index| {
        index["name"] == json!("system.node.label")
            && index["kind"] == json!("system")
            && index["entity"] == json!("node")
            && index["durable"] == json!(false)
            && index["derived"] == json!(true)
    }));
    assert!(indexes.iter().any(|index| {
        index["name"] == json!("system.edge.outgoing_adjacency")
            && index["fields"] == json!(["from", "type"])
    }));
    assert_eq!(
        catalog["notes"]["user_defined_indexes"],
        json!("future_work")
    );
}

#[tokio::test]
async fn session_indexes_command_renders_system_catalog() {
    let input = Cursor::new("session.indexes\nsession.exit\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Index Catalog"));
    assert!(output.contains("system.node.label"));
    assert!(output.contains("system.edge.incoming_adjacency"));
    assert!(!output.contains("durable=false"));
    assert!(!output.contains("user-defined indexes are future work"));
}

#[tokio::test]
async fn verbose_session_commands_render_diagnostic_metadata() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.describe --verbose\nsession.indexes --verbose\nsession.explain --verbose edge.find Authored from=1 to=2\nsession.profile --verbose node.find User name=Alice\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Verbose details:"));
    assert!(output.contains("backend: in_memory"));
    assert!(output.contains("system indexes: 7"));
    assert!(output.contains("durable indexes: 0"));
    assert!(output.contains("| durable"));
    assert!(output.contains("durable=false means index contents are not source-of-truth data"));
    assert!(output.contains("access_path=relationship_endpoint_adjacency"));
    assert!(
        output.contains("indexes=system.edge.outgoing_adjacency,system.edge.incoming_adjacency")
    );
    assert!(output.contains("access_path=node_property_index"));
    assert!(output.contains("index=system.node.property"));
    assert!(output.contains("scan=false"));
    assert!(output.contains("chosen_anchor=User.name"));
    assert!(output.contains("selected_access_path=node_property_index"));
    assert!(output.contains("rows_in=0 rows_out=1 elapsed="));
}

#[tokio::test]
async fn explain_structured_access_paths_identify_indexes_and_scans() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:optional\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice age=42\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, _) = session.into_parts();

    let id_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                id: Some(1),
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &id_explain,
        "NodeById",
        "node_id_lookup",
        Some("system.node.id"),
        false,
    );
    assert_eq!(
        id_explain["plan"]["details"][0]["planner"]["selected_access_path"],
        json!("node_id_lookup")
    );

    let property_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                predicates: vec![PropertyPredicate {
                    field: "name".to_string(),
                    op: PredicateOp::Eq,
                    value: json!("Alice"),
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &property_explain,
        "NodePropertySeek",
        "node_property_index",
        Some("system.node.property"),
        false,
    );
    assert_eq!(
        property_explain["plan"]["details"][0]["planner"]["chosen_anchor"],
        json!("User.name")
    );

    let multi_equality_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                predicates: vec![
                    PropertyPredicate {
                        field: "name".to_string(),
                        op: PredicateOp::Eq,
                        value: json!("Alice"),
                    },
                    PropertyPredicate {
                        field: "age".to_string(),
                        op: PredicateOp::Eq,
                        value: json!(42),
                    },
                ],
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &multi_equality_explain,
        "NodePropertySeek",
        "node_property_index",
        Some("system.node.property"),
        false,
    );
    assert_eq!(
        multi_equality_explain["plan"]["details"][0]["planner"]["residual_filters"],
        json!(["age"])
    );
    let details = multi_equality_explain["plan"]["details"]
        .as_array()
        .unwrap();
    assert!(
        details
            .iter()
            .any(|step| step["kind"] == json!("NodeFilter")
                && step["display"] == json!("NodeFilter v0 User age"))
    );

    let label_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &label_explain,
        "NodeLabelScan",
        "node_label_index",
        Some("system.node.label"),
        false,
    );

    let traversal_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                traversals: vec![TraversalStepRequest {
                    direction: TraversalDirection::Out,
                    edge_model: Some("Authored".to_string()),
                    end_model: "Post".to_string(),
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &traversal_explain,
        "ExpandOut",
        "outgoing_adjacency",
        Some("system.edge.outgoing_adjacency"),
        false,
    );

    let bidirectional_traversal_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                traversals: vec![TraversalStepRequest {
                    direction: TraversalDirection::Both,
                    edge_model: Some("Authored".to_string()),
                    end_model: "Post".to_string(),
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &bidirectional_traversal_explain,
        "ExpandBoth",
        "bidirectional_adjacency",
        None,
        false,
    );
    assert_plan_has_candidate_indexes(
        &bidirectional_traversal_explain,
        "ExpandBoth",
        &[
            "system.edge.outgoing_adjacency",
            "system.edge.incoming_adjacency",
        ],
    );

    let both_endpoint_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::EdgeFind(EdgeFindRequest {
                model: "Authored".to_string(),
                from: Some(1),
                to: Some(2),
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(
        &both_endpoint_explain,
        "RelationshipEndpointSeek",
        "relationship_endpoint_adjacency",
        None,
        false,
    );
    assert_plan_has_candidate_indexes(
        &both_endpoint_explain,
        "RelationshipEndpointSeek",
        &[
            "system.edge.outgoing_adjacency",
            "system.edge.incoming_adjacency",
        ],
    );

    let range_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(NodeFindRequest {
                model: "User".to_string(),
                predicates: vec![PropertyPredicate {
                    field: "age".to_string(),
                    op: PredicateOp::Gt,
                    value: json!(40),
                }],
                ..Default::default()
            }),
        })
        .unwrap();
    assert_plan_has_access(&range_explain, "NodeFilter", "scan", None, true);
    assert_eq!(
        range_explain["plan"]["details"][0]["planner"]["residual_filters"],
        json!(["age"])
    );
}

fn assert_plan_has_access(
    explain: &Value,
    kind: &str,
    access_path: &str,
    index: Option<&str>,
    scan: bool,
) {
    let details = explain["plan"]["details"].as_array().unwrap();
    let step = details
        .iter()
        .find(|step| step["kind"] == json!(kind))
        .unwrap_or_else(|| panic!("missing plan step kind {kind}: {details:#?}"));

    assert_eq!(step["access_path"], json!(access_path));
    assert_eq!(step["index"], index.map(Value::from).unwrap_or(Value::Null));
    assert_eq!(step["scan"], json!(scan));
}

fn assert_plan_has_candidate_indexes(explain: &Value, kind: &str, indexes: &[&str]) {
    let details = explain["plan"]["details"].as_array().unwrap();
    let step = details
        .iter()
        .find(|step| step["kind"] == json!(kind))
        .unwrap_or_else(|| panic!("missing plan step kind {kind}: {details:#?}"));
    assert_eq!(step["indexes"], json!(indexes));
}

#[tokio::test]
async fn typed_node_find_traversal_matches_cli_terms_and_supports_introspection() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create User name=Bob\nnode.create Post title=Hello\nedge.create Authored from=1 to=3 year=2024\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (state, _, _) = session.into_parts();
    let typed = NodeFindRequest {
        model: "User".to_string(),
        predicates: vec![PropertyPredicate {
            field: "name".to_string(),
            op: PredicateOp::Eq,
            value: json!("Alice"),
        }],
        traversals: vec![TraversalStepRequest {
            direction: TraversalDirection::Out,
            edge_model: Some("Authored".to_string()),
            end_model: "Post".to_string(),
        }],
        return_mode: Some(TraversalReturn::End),
        ..Default::default()
    };
    let terms = vec![
        QueryTerm {
            key: "name".to_string(),
            value: "Alice".to_string(),
        },
        QueryTerm {
            key: "via".to_string(),
            value: "out:Authored:Post".to_string(),
        },
    ];

    let typed_rows = match state.node_find(typed.clone()).await.unwrap() {
        SessionFindResult::Nodes(nodes) => nodes,
        SessionFindResult::Edges(_) => panic!("expected node rows"),
    };
    let cli_rows = match state.find_nodes_with_terms("User", &terms).await.unwrap() {
        SessionFindResult::Nodes(nodes) => nodes,
        SessionFindResult::Edges(_) => panic!("expected node rows"),
    };
    assert_eq!(typed_rows.len(), cli_rows.len());
    assert_eq!(typed_rows[0].id, cli_rows[0].id);
    assert_eq!(typed_rows[0].props, cli_rows[0].props);
    assert_eq!(typed_rows[0].props["title"], json!("Hello"));

    let typed_explain = state
        .explain(ExplainRequest {
            query: QueryRequest::NodeFind(typed.clone()),
        })
        .unwrap();
    let cli_explain = state.explain_node_find_terms("User", &terms).unwrap();
    assert_eq!(typed_explain["plan"], cli_explain["plan"]);

    let profile = state
        .profile(ProfileRequest {
            query: QueryRequest::NodeFind(typed),
        })
        .await
        .unwrap();
    assert_eq!(profile["command"], "node.find");
    assert_eq!(profile["result_rows"], 1);
    let phase_timings = profile["phase_timings"].as_object().unwrap();
    for phase in [
        "explain",
        "anchor_metric",
        "execute_node_query",
        "metric_push",
        "profile_value",
    ] {
        assert!(
            phase_timings[phase].as_u64().is_some(),
            "profile phase {phase} should report micros"
        );
    }
}

#[tokio::test]
async fn typed_node_find_allows_format_as_model_property() {
    let mut state = SessionState::new();
    state
        .define_node(DefineNodeRequest {
            name: "Document".to_string(),
            id_field: "documentId".to_string(),
            fields: vec![FieldSpec {
                name: "format".to_string(),
                value_type: FieldValueType::String,
                required: true,
            }],
        })
        .unwrap();
    state
        .create_instance(
            "Document",
            &BTreeMap::from([("format".to_string(), "jsonl".to_string())]),
        )
        .await
        .unwrap();

    let rows = match state
        .node_find(NodeFindRequest {
            model: "Document".to_string(),
            predicates: vec![PropertyPredicate {
                field: "format".to_string(),
                op: PredicateOp::Eq,
                value: json!("jsonl"),
            }],
            ..Default::default()
        })
        .await
        .unwrap()
    {
        SessionFindResult::Nodes(nodes) => nodes,
        SessionFindResult::Edges(_) => panic!("expected node rows"),
    };

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].props["format"], json!("jsonl"));
}

#[tokio::test]
async fn typed_edge_find_allows_from_and_to_as_relationship_properties() {
    let mut state = SessionState::new();
    state
        .define_node(DefineNodeRequest {
            name: "User".to_string(),
            id_field: "userId".to_string(),
            fields: vec![],
        })
        .unwrap();
    state
        .define_node(DefineNodeRequest {
            name: "Post".to_string(),
            id_field: "postId".to_string(),
            fields: vec![],
        })
        .unwrap();
    state
        .define_edge(DefineEdgeRequest {
            name: "Authored".to_string(),
            from_model: "User".to_string(),
            to_model: "Post".to_string(),
            id_field: "authoredId".to_string(),
            fields: vec![FieldSpec {
                name: "from".to_string(),
                value_type: FieldValueType::String,
                required: true,
            }],
        })
        .unwrap();

    let user = state
        .create_instance("User", &BTreeMap::new())
        .await
        .unwrap();
    let post = state
        .create_instance("Post", &BTreeMap::new())
        .await
        .unwrap();
    let mut tx = state.client().transaction().await.unwrap();
    tx.tx_mut()
        .unwrap()
        .create_relationship(
            user.id,
            post.id,
            "Authored",
            BTreeMap::from([("from".to_string(), json!("imported"))]),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let property_rows = state
        .edge_find(EdgeFindRequest {
            model: "Authored".to_string(),
            predicates: vec![PropertyPredicate {
                field: "from".to_string(),
                op: PredicateOp::Eq,
                value: json!("imported"),
            }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(property_rows.len(), 1);
    assert_eq!(property_rows[0].props["from"], json!("imported"));

    let endpoint_rows = state
        .edge_find(EdgeFindRequest {
            model: "Authored".to_string(),
            from: Some(user.id),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(endpoint_rows.len(), 1);
}

#[test]
fn typed_schema_requests_use_stable_field_json_shape() {
    let value = serde_json::to_value(DefineNodeRequest {
        name: "User".to_string(),
        id_field: "userId".to_string(),
        fields: vec![FieldSpec {
            name: "age".to_string(),
            value_type: FieldValueType::Int,
            required: false,
        }],
    })
    .unwrap();

    assert_eq!(value["fields"][0]["name"], json!("age"));
    assert_eq!(value["fields"][0]["type"], json!("int"));
    assert!(value["fields"][0].get("value_type").is_none());
}

#[tokio::test]
async fn runtime_apply_operations_return_durable_entries() {
    let mut state = SessionState::new();

    let user_schema = state
        .apply_define_node(DefineNodeRequest {
            name: "User".to_string(),
            id_field: "userId".to_string(),
            fields: vec![FieldSpec {
                name: "name".to_string(),
                value_type: FieldValueType::String,
                required: true,
            }],
        })
        .unwrap();
    assert!(matches!(
        user_schema.durable_op,
        DurableOperation::RegisterNodeModel { .. }
    ));

    state
        .apply_define_node(DefineNodeRequest {
            name: "Post".to_string(),
            id_field: "postId".to_string(),
            fields: vec![],
        })
        .unwrap();
    let edge_schema = state
        .apply_define_edge(DefineEdgeRequest {
            name: "Authored".to_string(),
            from_model: "User".to_string(),
            to_model: "Post".to_string(),
            id_field: "authoredId".to_string(),
            fields: vec![],
        })
        .unwrap();
    assert!(matches!(
        edge_schema.durable_op,
        DurableOperation::RegisterRelModel { .. }
    ));

    let user = state
        .apply_node_create(grm_rs::NodeCreateRequest {
            model: "User".to_string(),
            props: BTreeMap::from([("name".to_string(), json!("Alice"))]),
        })
        .await
        .unwrap();
    assert!(matches!(
        user.durable_op,
        DurableOperation::UpsertNode { .. }
    ));
    let post = state
        .apply_node_create(grm_rs::NodeCreateRequest {
            model: "Post".to_string(),
            props: BTreeMap::new(),
        })
        .await
        .unwrap();

    let edge = state
        .apply_edge_create(grm_rs::EdgeCreateRequest {
            model: "Authored".to_string(),
            from: user.value.id,
            to: post.value.id,
            props: BTreeMap::new(),
        })
        .await
        .unwrap();
    assert!(matches!(
        edge.durable_op,
        DurableOperation::UpsertRel { .. }
    ));

    let deleted = state
        .apply_edge_delete(grm_rs::EdgeDeleteRequest {
            model: "Authored".to_string(),
            id: edge.value.id,
        })
        .await
        .unwrap();
    assert_eq!(deleted.value.model, "Authored");
    assert!(matches!(
        deleted.durable_op,
        DurableOperation::DeleteRel { .. }
    ));
}

#[test]
fn typed_batch_request_has_flat_ordered_batch_shape() {
    let value = serde_json::to_value(BatchRequest {
        atomic: true,
        allow_deletes: false,
        response: SessionBatchResponse::Summary,
        ops: vec![],
    })
    .unwrap();

    assert_eq!(value["atomic"], json!(true));
    assert_eq!(value["allow_deletes"], json!(false));
    assert_eq!(value["response"], json!("summary"));
    assert!(value["ops"].as_array().is_some());
    assert!(value["ops"].get("ops").is_none());
}

#[tokio::test]
async fn runtime_node_find_response_uses_structured_request_fields() {
    let mut state = SessionState::new();
    state
        .define_node(DefineNodeRequest {
            name: "User".to_string(),
            id_field: "userId".to_string(),
            fields: vec![
                FieldSpec {
                    name: "name".to_string(),
                    value_type: FieldValueType::String,
                    required: true,
                },
                FieldSpec {
                    name: "age".to_string(),
                    value_type: FieldValueType::Int,
                    required: true,
                },
            ],
        })
        .unwrap();

    for (name, age) in [("Alice", 42), ("Bob", 37), ("Carol", 31)] {
        state
            .node_create(grm_rs::NodeCreateRequest {
                model: "User".to_string(),
                props: BTreeMap::from([
                    ("name".to_string(), json!(name)),
                    ("age".to_string(), json!(age)),
                ]),
            })
            .await
            .unwrap();
    }

    let response = state
        .node_find_response(grm_rs::NodeFindRequest {
            model: "User".to_string(),
            predicates: vec![PropertyPredicate {
                field: "age".to_string(),
                op: PredicateOp::Gt,
                value: json!(35),
            }],
            order: vec![grm_rs::OrderSpec {
                field: "age".to_string(),
                direction: grm_rs::OrderDirection::Asc,
            }],
            limit: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.model, "User");
    assert_eq!(response.nodes.len(), 1);
    assert_eq!(response.nodes[0].props["name"], json!("Bob"));
}

#[tokio::test]
async fn runtime_edge_find_response_uses_structured_request_fields() {
    let mut state = SessionState::new();
    state
        .define_node(DefineNodeRequest {
            name: "User".to_string(),
            id_field: "userId".to_string(),
            fields: vec![],
        })
        .unwrap();
    state
        .define_node(DefineNodeRequest {
            name: "Post".to_string(),
            id_field: "postId".to_string(),
            fields: vec![],
        })
        .unwrap();
    state
        .define_edge(DefineEdgeRequest {
            name: "Authored".to_string(),
            from_model: "User".to_string(),
            to_model: "Post".to_string(),
            id_field: "authoredId".to_string(),
            fields: vec![FieldSpec {
                name: "year".to_string(),
                value_type: FieldValueType::Int,
                required: true,
            }],
        })
        .unwrap();

    let user = state
        .node_create(grm_rs::NodeCreateRequest {
            model: "User".to_string(),
            props: BTreeMap::new(),
        })
        .await
        .unwrap();
    let post = state
        .node_create(grm_rs::NodeCreateRequest {
            model: "Post".to_string(),
            props: BTreeMap::new(),
        })
        .await
        .unwrap();
    state
        .edge_create(grm_rs::EdgeCreateRequest {
            model: "Authored".to_string(),
            from: user.id,
            to: post.id,
            props: BTreeMap::from([("year".to_string(), json!(2026))]),
        })
        .await
        .unwrap();

    let response = state
        .edge_find_response(grm_rs::EdgeFindRequest {
            model: "Authored".to_string(),
            from: Some(user.id),
            predicates: vec![PropertyPredicate {
                field: "year".to_string(),
                op: PredicateOp::Eq,
                value: json!(2026),
            }],
            ..Default::default()
        })
        .unwrap();

    assert_eq!(response.model, "Authored");
    assert_eq!(response.edges.len(), 1);
    assert_eq!(response.edges[0].to, post.id);
}

fn created_node_id_with_matching_durable(outcome: grm_rs::RuntimeDispatchOutcome) -> i64 {
    let grm_rs::RuntimeDispatchOutcome {
        response,
        durable_ops,
    } = outcome;
    match (response, durable_ops.as_slice()) {
        (
            RuntimeResponse::Node(NodeResponse::Create(node)),
            [DurableOperation::UpsertNode { node: durable_node }],
        ) if durable_node.id == node.id => node.id,
        other => panic!("expected node create response with matching durable op, got {other:?}"),
    }
}

fn created_edge_id_with_matching_durable(outcome: grm_rs::RuntimeDispatchOutcome) -> i64 {
    let grm_rs::RuntimeDispatchOutcome {
        response,
        durable_ops,
    } = outcome;
    match (response, durable_ops.as_slice()) {
        (
            RuntimeResponse::Edge(EdgeResponse::Create(edge)),
            [DurableOperation::UpsertRel { rel: durable_edge }],
        ) if durable_edge.id == edge.id => edge.id,
        other => panic!("expected edge create response with matching durable op, got {other:?}"),
    }
}

#[tokio::test]
async fn runtime_dispatcher_executes_schema_node_and_edge_requests() {
    let mut state = SessionState::new();

    let response = state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "User".to_string(),
                id_field: "userId".to_string(),
                fields: vec![FieldSpec {
                    name: "name".to_string(),
                    value_type: FieldValueType::String,
                    required: true,
                }],
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Schema(SchemaResponse::DefineNode(model)) if model.name == "User"
    ));
    assert!(matches!(
        response.durable_ops.as_slice(),
        [DurableOperation::RegisterNodeModel { model }] if model.name == "User"
    ));

    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "Post".to_string(),
                id_field: "postId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();

    let response = state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineEdge(
            DefineEdgeRequest {
                name: "Authored".to_string(),
                from_model: "User".to_string(),
                to_model: "Post".to_string(),
                id_field: "authoredId".to_string(),
                fields: vec![FieldSpec {
                    name: "year".to_string(),
                    value_type: FieldValueType::Int,
                    required: true,
                }],
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Schema(SchemaResponse::DefineEdge(model)) if model.name == "Authored"
    ));
    assert!(matches!(
        response.durable_ops.as_slice(),
        [DurableOperation::RegisterRelModel { model }] if model.name == "Authored"
    ));

    let response = state
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            grm_rs::NodeCreateRequest {
                model: "User".to_string(),
                props: BTreeMap::from([("name".to_string(), json!("Alice"))]),
            },
        )))
        .await
        .unwrap();
    let user_id = created_node_id_with_matching_durable(response);

    let response = state
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
            grm_rs::NodeCreateRequest {
                model: "Post".to_string(),
                props: BTreeMap::new(),
            },
        )))
        .await
        .unwrap();
    let post_id = created_node_id_with_matching_durable(response);

    let response = state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
            grm_rs::EdgeCreateRequest {
                model: "Authored".to_string(),
                from: user_id,
                to: post_id,
                props: BTreeMap::from([("year".to_string(), json!(2026))]),
            },
        )))
        .await
        .unwrap();
    let edge_id = created_edge_id_with_matching_durable(response);

    let response = state
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Find(NodeFindRequest {
            model: "User".to_string(),
            predicates: vec![PropertyPredicate {
                field: "name".to_string(),
                op: PredicateOp::Eq,
                value: json!("Alice"),
            }],
            ..Default::default()
        })))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Node(NodeResponse::Find(found))
            if found.model == "User" && found.nodes.len() == 1
    ));
    assert!(response.durable_ops.is_empty());

    let response = state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Find(
            grm_rs::EdgeFindRequest {
                model: "Authored".to_string(),
                id: Some(edge_id),
                ..Default::default()
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Edge(EdgeResponse::Find(found))
            if found.model == "Authored" && found.edges.len() == 1
    ));
    assert!(response.durable_ops.is_empty());
}

#[tokio::test]
async fn runtime_dispatcher_executes_update_and_delete_requests() {
    let mut state = SessionState::new();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "User".to_string(),
                id_field: "userId".to_string(),
                fields: vec![FieldSpec {
                    name: "name".to_string(),
                    value_type: FieldValueType::String,
                    required: true,
                }],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "Post".to_string(),
                id_field: "postId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineEdge(
            DefineEdgeRequest {
                name: "Authored".to_string(),
                from_model: "User".to_string(),
                to_model: "Post".to_string(),
                id_field: "authoredId".to_string(),
                fields: vec![FieldSpec {
                    name: "year".to_string(),
                    value_type: FieldValueType::Int,
                    required: false,
                }],
            },
        )))
        .await
        .unwrap();

    let user_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "User".to_string(),
                    props: BTreeMap::from([("name".to_string(), json!("Alice"))]),
                },
            )))
            .await
            .unwrap(),
    );
    let post_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "Post".to_string(),
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );
    let edge_id = created_edge_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
                grm_rs::EdgeCreateRequest {
                    model: "Authored".to_string(),
                    from: user_id,
                    to: post_id,
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );

    let response = state
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Update(
            grm_rs::NodeUpdateRequest {
                model: "User".to_string(),
                id: user_id,
                props: BTreeMap::from([("name".to_string(), json!("Ada"))]),
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        (&response.response, response.durable_ops.as_slice()),
        (
            RuntimeResponse::Node(NodeResponse::Update(node)),
            [DurableOperation::UpsertNode { node: durable_node }]
        ) if node.id == durable_node.id && node.props["name"] == json!("Ada")
    ));

    let response = state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Update(
            grm_rs::EdgeUpdateRequest {
                model: "Authored".to_string(),
                id: edge_id,
                props: BTreeMap::from([("year".to_string(), json!(2026))]),
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        (&response.response, response.durable_ops.as_slice()),
        (
            RuntimeResponse::Edge(EdgeResponse::Update(edge)),
            [DurableOperation::UpsertRel { rel: durable_edge }]
        ) if edge.id == durable_edge.id && edge.props["year"] == json!(2026)
    ));

    let response = state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Delete(
            grm_rs::EdgeDeleteRequest {
                model: "Authored".to_string(),
                id: edge_id,
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Edge(EdgeResponse::Delete(deleted))
            if deleted.model == "Authored" && deleted.id == edge_id
    ));
    assert!(matches!(
        response.durable_ops.as_slice(),
        [DurableOperation::DeleteRel { id }] if *id == edge_id
    ));

    let response = state
        .execute_runtime(RuntimeRequest::Node(NodeRequest::Delete(
            grm_rs::NodeDeleteRequest {
                model: "User".to_string(),
                id: user_id,
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        response.response,
        RuntimeResponse::Node(NodeResponse::Delete(deleted))
            if deleted.model == "User" && deleted.id == user_id
    ));
    assert!(matches!(
        response.durable_ops.as_slice(),
        [DurableOperation::DeleteNode { id }] if *id == user_id
    ));
}

#[tokio::test]
async fn runtime_dispatcher_routes_query_find_requests_through_find_responses() {
    let mut state = SessionState::new();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "User".to_string(),
                id_field: "userId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "Post".to_string(),
                id_field: "postId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineEdge(
            DefineEdgeRequest {
                name: "Authored".to_string(),
                from_model: "User".to_string(),
                to_model: "Post".to_string(),
                id_field: "authoredId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();

    let user_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "User".to_string(),
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );
    let post_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "Post".to_string(),
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );
    state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
            grm_rs::EdgeCreateRequest {
                model: "Authored".to_string(),
                from: user_id,
                to: post_id,
                props: BTreeMap::new(),
            },
        )))
        .await
        .unwrap();

    let node_response = state
        .execute_runtime(RuntimeRequest::Query(QueryRequest::NodeFind(
            NodeFindRequest {
                model: "User".to_string(),
                id: Some(user_id),
                ..Default::default()
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        node_response.response,
        RuntimeResponse::Node(NodeResponse::Find(found))
            if found.model == "User" && found.nodes.len() == 1
    ));
    assert!(node_response.durable_ops.is_empty());

    let edge_response = state
        .execute_runtime(RuntimeRequest::Query(QueryRequest::EdgeFind(
            grm_rs::EdgeFindRequest {
                model: "Authored".to_string(),
                from: Some(user_id),
                ..Default::default()
            },
        )))
        .await
        .unwrap();
    assert!(matches!(
        edge_response.response,
        RuntimeResponse::Edge(EdgeResponse::Find(found))
            if found.model == "Authored" && found.edges.len() == 1
    ));
    assert!(edge_response.durable_ops.is_empty());
}

#[tokio::test]
async fn runtime_dispatcher_executes_batch_request_through_existing_batch_path() {
    let mut state = SessionState::new();

    let response = state
        .execute_runtime(RuntimeRequest::Batch(BatchRequest {
            atomic: true,
            allow_deletes: false,
            response: SessionBatchResponse::Detailed,
            ops: vec![
                grm_rs::SessionBatchOp::SchemaDefineNode(grm_rs::SessionBatchDefineNodeParams {
                    name: "User".to_string(),
                    id_field: "userId".to_string(),
                    fields: vec![grm_rs::SessionBatchFieldParam {
                        name: "name".to_string(),
                        value_type: "string".to_string(),
                        required: true,
                    }],
                }),
                grm_rs::SessionBatchOp::NodeCreate(grm_rs::SessionBatchNodeCreateParams {
                    model: "User".to_string(),
                    props: BTreeMap::from([("name".to_string(), json!("Ada"))]),
                    local_ref: Some("ada".to_string()),
                }),
            ],
        }))
        .await
        .unwrap();

    assert!(matches!(
        response.response,
        RuntimeResponse::Batch(batch)
            if batch.should_persist
                && batch.value["applied"] == json!(true)
                && batch.value["ids"][0]["ref"] == json!("ada")
    ));
    assert!(matches!(
        response.durable_ops.as_slice(),
        [DurableOperation::Batch { ops }] if ops.len() == 2
    ));
}

#[tokio::test]
async fn runtime_dispatcher_executes_explain_and_profile_requests() {
    let mut state = SessionState::new();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "User".to_string(),
                id_field: "userId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineNode(
            DefineNodeRequest {
                name: "Post".to_string(),
                id_field: "postId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();
    state
        .execute_runtime(RuntimeRequest::Schema(SchemaRequest::DefineEdge(
            DefineEdgeRequest {
                name: "Authored".to_string(),
                from_model: "User".to_string(),
                to_model: "Post".to_string(),
                id_field: "authoredId".to_string(),
                fields: vec![],
            },
        )))
        .await
        .unwrap();

    let user_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "User".to_string(),
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );
    let post_id = created_node_id_with_matching_durable(
        state
            .execute_runtime(RuntimeRequest::Node(NodeRequest::Create(
                grm_rs::NodeCreateRequest {
                    model: "Post".to_string(),
                    props: BTreeMap::new(),
                },
            )))
            .await
            .unwrap(),
    );
    state
        .execute_runtime(RuntimeRequest::Edge(EdgeRequest::Create(
            grm_rs::EdgeCreateRequest {
                model: "Authored".to_string(),
                from: user_id,
                to: post_id,
                props: BTreeMap::new(),
            },
        )))
        .await
        .unwrap();

    let query = QueryRequest::NodeFind(NodeFindRequest {
        model: "User".to_string(),
        traversals: vec![TraversalStepRequest {
            direction: TraversalDirection::Out,
            edge_model: Some("Authored".to_string()),
            end_model: "Post".to_string(),
        }],
        return_mode: Some(TraversalReturn::End),
        ..Default::default()
    });

    let explain = state
        .execute_runtime(RuntimeRequest::Explain(ExplainRequest {
            query: query.clone(),
        }))
        .await
        .unwrap();
    assert!(matches!(
        explain.response,
        RuntimeResponse::Explain(value)
            if value["command"] == json!("node.find")
                && value["plan"]["details"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|step| step["kind"] == json!("ExpandOut"))
    ));
    assert!(explain.durable_ops.is_empty());

    let profile = state
        .execute_runtime(RuntimeRequest::Profile(ProfileRequest { query }))
        .await
        .unwrap();
    assert!(matches!(
        profile.response,
        RuntimeResponse::Profile(value)
            if value["command"] == json!("node.find") && value["result_rows"] == json!(1)
    ));
    assert!(profile.durable_ops.is_empty());
}

#[tokio::test]
async fn runtime_dispatcher_returns_clear_unsupported_errors_for_excluded_variants() {
    let mut state = SessionState::new();
    let unsupported = vec![
        (
            RuntimeRequest::Query(QueryRequest::Traversal(grm_rs::TraversalRequest {
                root: NodeFindRequest {
                    model: "User".to_string(),
                    ..Default::default()
                },
            })),
            "traversal query requests yet",
        ),
        (
            RuntimeRequest::Admin(grm_rs::AdminRequest::SchemaList),
            "admin requests",
        ),
    ];

    for (request, message) in unsupported {
        let err = state.execute_runtime(request).await.unwrap_err();
        assert!(
            err.to_string().contains(message),
            "expected unsupported error containing {message:?}, got {err}"
        );
    }
}

#[test]
fn adapter_filter_values_build_structured_find_requests() {
    let node = grm_rs::NodeFindRequest::from_adapter_filter_values(
        "User",
        BTreeMap::from([
            ("age>".to_string(), json!(35)),
            ("order".to_string(), json!("age:asc")),
            ("limit".to_string(), json!(1)),
        ]),
    )
    .unwrap();
    assert_eq!(node.model, "User");
    assert_eq!(node.predicates[0].field, "age");
    assert_eq!(node.predicates[0].op, PredicateOp::Gt);
    assert_eq!(node.order[0].field, "age");
    assert_eq!(node.limit, Some(1));

    let edge = grm_rs::EdgeFindRequest::from_adapter_filter_values(
        "Authored",
        BTreeMap::from([
            ("from".to_string(), json!(7)),
            ("year<=".to_string(), json!(2026)),
        ]),
    )
    .unwrap();
    assert_eq!(edge.from, Some(7));
    assert_eq!(edge.predicates[0].field, "year");
    assert_eq!(edge.predicates[0].op, PredicateOp::Le);
}

#[tokio::test]
async fn session_explain_and_profile_edge_find_render_logical_plan() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.explain edge.find Authored from=1\nsession.profile --verbose edge.find Authored from=1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Current logical plan for edge.find Authored"));
    assert!(output.contains("RelationshipEndpointSeek v0 :Authored from=1"));
    assert!(output.contains("Return Rel v0"));
    assert!(output.contains("Profile for edge.find Authored"));
    assert!(output.contains("Result rows: 1"));
    assert!(output.contains("Elapsed: "));
    assert!(output.contains("rows_in=0 rows_out=1 elapsed="));
}

#[tokio::test]
async fn session_explain_edge_find_property_filter_uses_type_scan_then_filter() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.explain edge.find Authored year=2024\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("RelationshipTypeScan v0 :Authored"));
    assert!(output.contains("RelationshipFilter v0 :Authored year"));
    assert!(!output.contains("RelationshipPropertySeek"));
}

#[tokio::test]
async fn session_explain_rejects_format_terms() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nsession.explain node.find User format=jsonl\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("format= is not supported with session.explain or session.profile"));
}

#[tokio::test]
async fn node_find_supports_jsonl_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.find User age>=21 format=jsonl\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""kind":"node""#));
    assert!(output.contains(r#""model":"User""#));
    assert!(output.contains(r#""id":1"#));
    assert!(output.contains(r#""labels":["User"]"#));
    assert!(output.contains(r#""name":"Alice""#));
    assert!(output.contains(r#""age":42"#));
    assert!(!output.contains("nodes matched model"));
}

#[tokio::test]
async fn node_find_supports_colored_default_output() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.find User age>=21\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new_with_color(input, output, true);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 nodes matched model 'User'."));
    assert!(output.contains("Node \u{1b}[32mUser\u{1b}[0m \u{1b}[34muserId\u{1b}[0m=1"));
    assert!(output.contains(
        "{\u{1b}[34mage\u{1b}[0m=42 \u{1b}[34mname\u{1b}[0m=\u{1b}[38;5;208mAlice\u{1b}[0m}"
    ));
}

#[tokio::test]
async fn edge_find_supports_jsonl_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nedge.find Authored from=1 format=jsonl\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains(r#""kind":"edge""#));
    assert!(output.contains(r#""model":"Authored""#));
    assert!(output.contains(r#""id":1"#));
    assert!(output.contains(r#""from":1"#));
    assert!(output.contains(r#""to":2"#));
    assert!(output.contains(r#""type":"Authored""#));
    assert!(output.contains(r#""year":2024"#));
    assert!(!output.contains("edges matched link"));
}

#[tokio::test]
async fn edge_find_supports_colored_table_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId authoredOn:string:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 authoredOn=2026-04-12\nedge.find Authored from=1 format=table\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new_with_color(input, output, true);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("\u{1b}[34mauthoredId\u{1b}[0m"));
    assert!(output.contains("\u{1b}[32mtype\u{1b}[0m"));
    assert!(output.contains("\u{1b}[34mauthoredOn\u{1b}[0m"));
    assert!(output.contains("\u{1b}[32mAuthored\u{1b}[0m"));
    assert!(output.contains("\u{1b}[38;5;208m2026-04-12\u{1b}[0m"));
}

#[tokio::test]
async fn node_find_supports_table_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.find User age>=21 format=table\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("| userId | name  | age |"));
    assert!(output.contains("| 1      | Alice | 42  |"));
    assert!(!output.contains("nodes matched model"));
}

#[tokio::test]
async fn edge_find_supports_table_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nedge.find Authored from=1 format=table\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("| authoredId | from | to | type     | year |"));
    assert!(output.contains("| 1          | 1    | 2  | Authored | 2024 |"));
}

#[tokio::test]
async fn find_graph_format_is_rejected_for_flat_results() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=Alice\nnode.find User format=graph\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("graph format is only supported for graph-shaped query results"));
}

#[tokio::test]
async fn node_find_traversal_supports_graph_format() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId authoredOn:string:required\nnode.create User name=\"Alice Jones\"\nnode.create Post title=\"Hello World\"\nnode.create Post title=\"Draft Notes\"\nedge.create Authored from=1 to=2 authoredOn=2026-04-10\nedge.create Authored from=1 to=3 authoredOn=2026-04-20\nnode.find User name=\"Alice Jones\" via=out:Authored:Post format=graph\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("graph: 3 nodes, 2 links"));
    assert!(output.contains("* (User#1) name=\"Alice Jones\""));
    assert!(output.contains("|\\"));
    assert!(
        output.contains("| * [Authored#1] authoredOn=2026-04-10 -> (Post#2) title=\"Hello World\"")
    );
    assert!(
        output.contains("| * [Authored#2] authoredOn=2026-04-20 -> (Post#3) title=\"Draft Notes\"")
    );
}

#[tokio::test]
async fn node_find_traversal_graph_marks_revisited_nodes_as_seen() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nlink.define Knows User User knowsId since:string:required\nnode.create User name=Alice\nnode.create User name=Bob\nedge.create Knows from=1 to=2 since=2026-04-10\nedge.create Knows from=2 to=1 since=2026-04-11\nnode.find User name=Alice via=out:Knows:User via=out:Knows:User format=graph\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("graph: 2 nodes, 2 links"));
    assert!(output.contains("* (User#1) name=Alice"));
    assert!(output.contains("* [Knows#1] since=2026-04-10 -> (User#2) name=Bob"));
    assert!(output.contains("* [Knows#2] since=2026-04-11 -> (User#1) [seen]"));
}

#[tokio::test]
async fn node_find_supports_comparison_and_contains_operators() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:optional\nnode.create User name=\"Alice Jones\" age=42\nnode.create User name=Bob age=35\nnode.find User age>40\nnode.find User name!=\"Alice Jones\"\nnode.find User name~Jones\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Node User userId=1 {age=42 name=\"Alice Jones\"}"));
    assert!(output.contains("Node User userId=2 {age=35 name=Bob}"));
    assert_eq!(output.matches("1 nodes matched model 'User'.").count(), 3);
}

#[tokio::test]
async fn node_find_supports_order_limit_and_offset() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.create User name=Bob age=35\nnode.create User name=Carol age=50\nnode.find User age>=35 order=age:desc limit=2 offset=1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("2 nodes matched model 'User'."));
    assert!(output.contains("Node User userId=1 {age=42 name=Alice}"));
    assert!(output.contains("Node User userId=2 {age=35 name=Bob}"));
    assert!(!output.contains("userId=3 {age=50 name=Carol}"));
}

#[tokio::test]
async fn node_find_supports_multi_field_ordering() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Bob age=42\nnode.create User name=Alice age=42\nnode.create User name=Carol age=35\nnode.find User age>=35 order=age:desc,name:asc\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    let alice_pos = output
        .find("Node User userId=2 {age=42 name=Alice}")
        .unwrap();
    let bob_pos = output.find("Node User userId=1 {age=42 name=Bob}").unwrap();
    let carol_pos = output
        .find("Node User userId=3 {age=35 name=Carol}")
        .unwrap();

    assert!(alice_pos < bob_pos);
    assert!(bob_pos < carol_pos);
}

#[tokio::test]
async fn edge_find_supports_endpoint_filters_and_comparison_operators() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nnode.create Post title=World\nedge.create Authored from=1 to=2 year=2024\nedge.create Authored from=1 to=3 year=2025\nedge.find Authored from=1 year>=2025\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("1 edges matched link 'Authored'."));
    assert!(output.contains("Edge Authored authoredId=2 from=1 to=3 {year=2025}"));
    assert!(!output.contains("authoredId=1 from=1 to=2 {year=2024}"));
}

#[tokio::test]
async fn edge_find_supports_multi_field_ordering() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Alpha\nnode.create Post title=Beta\nnode.create Post title=Gamma\nedge.create Authored from=1 to=2 year=2024\nedge.create Authored from=1 to=3 year=2024\nedge.create Authored from=1 to=4 year=2025\nedge.find Authored from=1 order=year:desc,to:asc\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    let rel_2025 = output
        .find("Edge Authored authoredId=3 from=1 to=4 {year=2025}")
        .unwrap();
    let rel_to_2 = output
        .find("Edge Authored authoredId=1 from=1 to=2 {year=2024}")
        .unwrap();
    let rel_to_3 = output
        .find("Edge Authored authoredId=2 from=1 to=3 {year=2024}")
        .unwrap();

    assert!(rel_2025 < rel_to_2);
    assert!(rel_to_2 < rel_to_3);
}

#[tokio::test]
async fn node_find_rejects_duplicate_order_fields() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.find User order=age:desc,age:asc\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("duplicate order field 'age'"));
}

#[tokio::test]
async fn node_find_reports_malformed_order_errors() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:required\nnode.create User name=Alice age=42\nnode.find User order=age\nnode.find User order=age:up\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("order must use order=<field>:asc|desc[,<field>:asc|desc ...]"));
    assert!(output.contains("order direction must be asc or desc"));
}

#[tokio::test]
async fn node_find_reports_invalid_query_term_shapes() {
    let input = Cursor::new(
        "model.define User userId age:int:required\nnode.create User age=42\nnode.find User age>>40\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("invalid query term 'age>>40'"));
    assert!(output.contains("line 1, column"));
    assert!(output.contains("^"));
}

#[tokio::test]
async fn node_find_reports_invalid_limit_values() {
    let input = Cursor::new(
        "model.define User userId age:int:required\nnode.create User age=42\nnode.find User limit=ten\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("limit must be a non-negative integer"));
}

#[tokio::test]
async fn multiline_query_errors_include_line_and_column() {
    let input = Cursor::new(
        "model.define User userId age:int:required\nnode.create User age=42\nnode.find User \\\nage>>40\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("invalid query term 'age>>40'"));
    assert!(output.contains("line 2, column 1"));
    assert!(output.contains("age>>40"));
    assert!(output.contains("^"));
}

#[tokio::test]
async fn node_find_reports_unknown_order_fields() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nnode.create User name=Alice\nnode.find User order=nickname:asc\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("unknown order field 'nickname' for model 'User'"));
}

#[tokio::test]
async fn edge_find_reports_reserved_endpoint_operator_misuse() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nedge.find Authored from>1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("special filter 'from' only supports '='"));
}

#[tokio::test]
async fn node_update_and_delete_work() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:optional\nnode.create User name=Alice age=42\nnode.update User 1 age=43\nnode.find User age=43\nnode.delete User 1\nnode.find User id=1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Updated node User userId=1 {age=43 name=Alice}"));
    assert!(output.contains("Node User userId=1 {age=43 name=Alice}"));
    assert!(output.contains("Deleted node User 1."));
    assert!(output.contains("No nodes matched model 'User'."));
}

#[tokio::test]
async fn node_update_supports_quoted_strings_and_multiple_fields() {
    let input = Cursor::new(
        "model.define User userId name:string:required age:int:optional\nnode.create User name=\"Alice Jones\" age=42\nnode.update User 1 name=\"Alice Johnson\" age=43\nnode.find User name=\"Alice Johnson\"\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Updated node User userId=1 {age=43 name=\"Alice Johnson\"}"));
    assert!(output.contains("Node User userId=1 {age=43 name=\"Alice Johnson\"}"));
}

#[tokio::test]
async fn edge_update_and_delete_work() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nedge.update Authored 1 year=2025\nedge.find Authored year=2025\nedge.delete Authored 1\nedge.find Authored id=1\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Updated edge Authored authoredId=1 from=1 to=2 {year=2025}"));
    assert!(output.contains("Edge Authored authoredId=1 from=1 to=2 {year=2025}"));
    assert!(output.contains("Deleted edge Authored 1."));
    assert!(output.contains("No edges matched link 'Authored'."));
}

#[tokio::test]
async fn edge_update_supports_string_date_properties() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId authoredOn:string:required\nnode.create User name=\"Alice Jones\"\nnode.create Post title=\"Hello World\"\nedge.create Authored from=1 to=2 authoredOn=2026-04-10\nedge.update Authored 1 authoredOn=2026-04-12\nedge.find Authored authoredOn=2026-04-12\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(
        output.contains("Updated edge Authored authoredId=1 from=1 to=2 {authoredOn=2026-04-12}")
    );
    assert!(output.contains("Edge Authored authoredId=1 from=1 to=2 {authoredOn=2026-04-12}"));
}

#[tokio::test]
async fn deleting_node_removes_attached_edges_via_session_commands() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nnode.delete User 1\nedge.find Authored\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Deleted node User 1."));
    assert!(output.contains("No edges matched link 'Authored'."));
}

#[tokio::test]
async fn session_save_supports_json_and_bin_flags() {
    let json_path = "/tmp/grm-session-save-test.json";
    let bin_path = "/tmp/grm-session-save-test.bin";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(bin_path);

    let input = Cursor::new(format!(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.save --json {json_path}\nsession.save --bin {bin_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(fs::metadata(json_path).is_ok());
    assert!(fs::metadata(bin_path).is_ok());
    assert!(output.contains("Saved session to JSON file"));
    assert!(output.contains("Saved session to binary file"));

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(bin_path);
}

#[tokio::test]
async fn session_load_restores_graph_and_runtime_schema() {
    let json_path = "/tmp/grm-session-load-test.json";
    let _ = fs::remove_file(json_path);

    let input = Cursor::new(format!(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.save --json {json_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    let load_input = Cursor::new(format!(
        "session.load --json {json_path}\nmodel.show User\nlink.show Authored\nnode.find User name=Alice\nedge.find Authored from=1\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut loaded_session = CliSession::new(load_input, output);
    loaded_session.run().await.unwrap();

    let (state, _, output) = loaded_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(state.model("User").is_some());
    assert!(state.rel_model("Authored").is_some());
    assert!(output.contains("Loaded session from JSON file"));
    assert!(output.contains("Model: User"));
    assert!(output.contains("Link: Authored"));
    assert!(output.contains("Node User userId=1 {name=Alice}"));
    assert!(output.contains("Edge Authored authoredId=1 from=1 to=2 {year=2024}"));

    let _ = fs::remove_file(json_path);
}

#[tokio::test]
async fn session_export_writes_interchange_json() {
    let json_path = "/tmp/grm-session-export-test.json";
    let _ = fs::remove_file(json_path);

    let input = Cursor::new(format!(
        "model.define User userId name:string:required age:int:optional\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice age=42\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.export --json {json_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let exported: Value = serde_json::from_str(&fs::read_to_string(json_path).unwrap()).unwrap();
    let expected = valid_interchange_document();

    assert!(output.contains("Exported graph to JSON file"));
    assert_eq!(exported, expected);

    let _ = fs::remove_file(json_path);
}

#[tokio::test]
async fn session_import_loads_interchange_json_into_empty_session() {
    let json_path = "/tmp/grm-session-import-test.json";
    let _ = fs::remove_file(json_path);

    let export_input = Cursor::new(format!(
        "model.define User userId name:string:required age:int:optional\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice age=42\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.export --json {json_path}\nsession.exit\n"
    ));
    let mut export_session = CliSession::new(export_input, Vec::new());
    export_session.run().await.unwrap();

    let import_input = Cursor::new(format!(
        "session.import --json {json_path}\nmodel.list\nlink.list\nnode.find User name=Alice\nedge.find Authored from=1\nsession.exit\n"
    ));
    let mut import_session = CliSession::new(import_input, Vec::new());
    import_session.run().await.unwrap();

    let (_, _, output) = import_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Imported graph from JSON file"));
    assert!(output.contains("User [2 fields, label=User]"));
    assert!(output.contains("Authored [1 fields, User -> Post, type=Authored]"));
    assert!(output.contains("1 nodes matched model 'User'."));
    assert!(output.contains("Node User userId=1 {age=42 name=Alice}"));
    assert!(output.contains("1 edges matched link 'Authored'."));
    assert!(output.contains("Edge Authored authoredId=1 from=1 to=2 {year=2024}"));

    let _ = fs::remove_file(json_path);
}

#[tokio::test]
async fn session_import_requires_empty_session() {
    let json_path = "/tmp/grm-session-import-non-empty-test.json";
    let _ = fs::remove_file(json_path);

    let export_input = Cursor::new(format!(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.export --json {json_path}\nsession.exit\n"
    ));
    let mut export_session = CliSession::new(export_input, Vec::new());
    export_session.run().await.unwrap();

    let import_input = Cursor::new(format!(
        "model.define User userId name:string:required\nsession.import --json {json_path}\nsession.exit\n"
    ));
    let mut import_session = CliSession::new(import_input, Vec::new());
    import_session.run().await.unwrap();

    let (_, _, output) = import_session.into_parts();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("constraint violation: session.import requires an empty session"));

    let _ = fs::remove_file(json_path);
}

fn valid_interchange_document() -> Value {
    serde_json::from_str(include_str!("fixtures/interchange_v1_basic.json")).unwrap()
}

async fn assert_import_contract_error(case_name: &str, document: Value, expected: &str) {
    let json_path = format!("/tmp/grm-import-contract-{case_name}.json");
    let _ = fs::remove_file(&json_path);
    fs::write(&json_path, serde_json::to_string_pretty(&document).unwrap()).unwrap();

    let input = Cursor::new(format!("session.import --json {json_path}\nsession.exit\n"));
    let mut session = CliSession::new(input, Vec::new());
    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains(expected),
        "expected output to contain {expected:?}, got:\n{output}"
    );

    let _ = fs::remove_file(json_path);
}

#[tokio::test]
async fn session_import_rejects_invalid_interchange_headers() {
    let mut document = valid_interchange_document();
    document["format"] = json!("not.grm");
    assert_import_contract_error(
        "wrong-format",
        document,
        "constraint violation: import file is not a grm.interchange document",
    )
    .await;

    let mut document = valid_interchange_document();
    document["version"] = json!(2);
    assert_import_contract_error(
        "unsupported-version",
        document,
        "constraint violation: unsupported import version '2'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["kind"] = json!("schema");
    assert_import_contract_error(
        "unsupported-kind",
        document,
        "constraint violation: unsupported import kind 'schema'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["identity"]["node"] = json!("uuid");
    assert_import_contract_error(
        "unsupported-node-id-type",
        document,
        "constraint violation: unsupported import id type 'uuid'",
    )
    .await;
}

#[tokio::test]
async fn session_import_rejects_invalid_interchange_schema() {
    let mut document = valid_interchange_document();
    document["schema"]["nodes"][0]["fields"][0]["type"] = json!("date");
    assert_import_contract_error(
        "unsupported-field-type",
        document,
        "constraint violation: unsupported import field type 'date'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["schema"]["edges"][0]["from"] = json!("Author");
    assert_import_contract_error(
        "missing-from-model",
        document,
        "constraint violation: from model 'Author' is not defined in import schema",
    )
    .await;

    let mut document = valid_interchange_document();
    document["schema"]["edges"][0]["to"] = json!("Article");
    assert_import_contract_error(
        "missing-to-model",
        document,
        "constraint violation: to model 'Article' is not defined in import schema",
    )
    .await;
}

#[tokio::test]
async fn session_import_rejects_invalid_interchange_node_data() {
    let mut document = valid_interchange_document();
    document["data"]["nodes"][0]["id"] = json!(0);
    assert_import_contract_error(
        "non-positive-node-id",
        document,
        "constraint violation: imported node id '0' must be positive",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["nodes"][0]["model"] = json!("Person");
    assert_import_contract_error(
        "unknown-node-model",
        document,
        "constraint violation: node model 'Person' is not defined in import schema",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "id": 1, "model": "Post", "props": { "title": "Duplicate" } }));
    assert_import_contract_error(
        "duplicate-node-id",
        document,
        "constraint violation: import contains duplicate node id '1'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["nodes"][0]["props"]
        .as_object_mut()
        .unwrap()
        .remove("name");
    assert_import_contract_error(
        "missing-required-node-field",
        document,
        "constraint violation: missing required field 'name' for imported node model 'User'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["nodes"][0]["props"]["name"] = json!(42);
    assert_import_contract_error(
        "wrong-node-field-type",
        document,
        "constraint violation: field 'name' for imported node model 'User' must be string",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["nodes"][0]["props"]["extra"] = json!("surprise");
    assert_import_contract_error(
        "unknown-node-field",
        document,
        "constraint violation: unknown field 'extra' for imported node model 'User'",
    )
    .await;
}

#[tokio::test]
async fn session_import_rejects_invalid_interchange_edge_data() {
    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["id"] = json!(0);
    assert_import_contract_error(
        "non-positive-edge-id",
        document,
        "constraint violation: imported edge id '0' must be positive",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["model"] = json!("Edited");
    assert_import_contract_error(
        "unknown-edge-model",
        document,
        "constraint violation: edge model 'Edited' is not defined in import schema",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"].as_array_mut().unwrap().push(
        json!({ "id": 1, "model": "Authored", "from": 1, "to": 2, "props": { "year": 2025 } }),
    );
    assert_import_contract_error(
        "duplicate-edge-id",
        document,
        "constraint violation: import contains duplicate edge id '1'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["from"] = json!(99);
    assert_import_contract_error(
        "missing-from-node",
        document,
        "constraint violation: edge '1' references missing from node '99'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["to"] = json!(99);
    assert_import_contract_error(
        "missing-to-node",
        document,
        "constraint violation: edge '1' references missing to node '99'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["from"] = json!(2);
    assert_import_contract_error(
        "wrong-from-node-model",
        document,
        "constraint violation: edge '1' from node '2' does not match model 'User'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["to"] = json!(1);
    assert_import_contract_error(
        "wrong-to-node-model",
        document,
        "constraint violation: edge '1' to node '1' does not match model 'Post'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["props"]
        .as_object_mut()
        .unwrap()
        .remove("year");
    assert_import_contract_error(
        "missing-required-edge-field",
        document,
        "constraint violation: missing required field 'year' for imported edge model 'Authored'",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["props"]["year"] = json!("2024");
    assert_import_contract_error(
        "wrong-edge-field-type",
        document,
        "constraint violation: field 'year' for imported edge model 'Authored' must be int",
    )
    .await;

    let mut document = valid_interchange_document();
    document["data"]["edges"][0]["props"]["extra"] = json!(true);
    assert_import_contract_error(
        "unknown-edge-field",
        document,
        "constraint violation: unknown field 'extra' for imported edge model 'Authored'",
    )
    .await;
}

#[tokio::test]
async fn session_autocommit_persists_changes_until_disabled() {
    let json_path = "/tmp/grm-session-autocommit-test.json";
    let backup_path = "/tmp/grm-session-autocommit-test.json.bak";
    let log_path = "/tmp/grm-session-autocommit-test.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input = Cursor::new(format!(
        "session.autocommit status\nsession.autocommit --json {json_path}\nmodel.define User userId name:string:required\nnode.create User name=Alice\nsession.autocommit status\nsession.autocommit off\nnode.create User name=Bob\nsession.autocommit status\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let saved = fs::read_to_string(json_path).unwrap();
    let backup = fs::read_to_string(backup_path).unwrap();
    let log = fs::read_to_string(log_path).unwrap();

    assert!(output.contains("Autocommit is disabled."));
    assert!(output.contains(&format!("Autocommit enabled: --json {json_path}")));
    assert!(output.contains("Autocommit disabled."));
    assert!(saved.contains("\"graph\""));
    assert!(backup.contains("\"graph\""));
    assert!(log.contains("Alice"));
    assert!(log.contains("RegisterNodeModel"));
    assert!(!log.contains("Bob"));

    let load_input = Cursor::new(format!(
        "session.load --json {json_path}\nnode.find User name=Alice\nnode.find User name=Bob\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut loaded_session = CliSession::new(load_input, output);
    loaded_session.run().await.unwrap();

    let (_, _, output) = loaded_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Node User userId=1 {name=Alice}"));
    assert!(!output.contains("Node User userId=2 {name=Bob}"));

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn session_load_triggers_autocommit_target_update() {
    let source_path = "/tmp/grm-session-autocommit-source.json";
    let target_path = "/tmp/grm-session-autocommit-target.json";
    let target_log_path = "/tmp/grm-session-autocommit-target.json.log";
    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(target_path);
    let _ = fs::remove_file(target_log_path);

    let seed_input = Cursor::new(format!(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.save --json {source_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut seed_session = CliSession::new(seed_input, output);
    seed_session.run().await.unwrap();

    let input = Cursor::new(format!(
        "session.autocommit --json {target_path}\nsession.load --json {source_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let saved = fs::read_to_string(target_path).unwrap();

    assert!(output.contains("Loaded session from JSON file"));
    assert!(saved.contains("Alice"));
    assert!(saved.contains("User"));
    assert!(fs::metadata(target_log_path).is_err());

    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(target_path);
}

#[tokio::test]
async fn session_autocommit_supports_binary_targets() {
    let bin_path = "/tmp/grm-session-autocommit-test.bin";
    let backup_path = "/tmp/grm-session-autocommit-test.bin.bak";
    let log_path = "/tmp/grm-session-autocommit-test.bin.log";
    let _ = fs::remove_file(bin_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input = Cursor::new(format!(
        "session.autocommit --bin {bin_path}\nmodel.define User userId name:string:required\nnode.create User name=Alice\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    let load_input = Cursor::new(format!(
        "session.load --bin {bin_path}\nnode.find User name=Alice\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut loaded_session = CliSession::new(load_input, output);
    loaded_session.run().await.unwrap();

    let (_, _, output) = loaded_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(fs::metadata(bin_path).is_ok());
    assert!(fs::metadata(backup_path).is_ok());
    assert!(fs::metadata(log_path).is_ok());
    assert!(output.contains("Node User userId=1 {name=Alice}"));

    let _ = fs::remove_file(bin_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn session_compact_checkpoints_autocommit_log() {
    let json_path = "/tmp/grm-session-compact-test.json";
    let backup_path = "/tmp/grm-session-compact-test.json.bak";
    let log_path = "/tmp/grm-session-compact-test.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input = Cursor::new(format!(
        "session.compact\nsession.autocommit --json {json_path}\nmodel.define User userId name:string:required\nnode.create User name=Alice\nsession.compact\nnode.create User name=Bob\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let compacted = fs::read_to_string(json_path).unwrap();
    let log = fs::read_to_string(log_path).unwrap();

    assert!(
        output.contains("constraint violation: session.compact requires autocommit to be enabled")
    );
    assert!(output.contains(&format!(
        "Compacted session into --json file '{json_path}'."
    )));
    assert!(compacted.contains("Alice"));
    assert!(!compacted.contains("Bob"));
    assert!(log.contains("Bob"));
    assert!(!log.contains("Alice"));

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn session_compact_is_available_from_api() {
    let json_path = "/tmp/grm-session-compact-api-test.json";
    let backup_path = "/tmp/grm-session-compact-api-test.json.bak";
    let log_path = "/tmp/grm-session-compact-api-test.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input =
        Cursor::new("model.define User userId name:string:required\nnode.create User name=Alice\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.enable_autocommit_json(json_path).unwrap();
    session.run().await.unwrap();

    let log_before = fs::read_to_string(log_path).unwrap();
    assert!(log_before.contains("Alice"));

    let summary = session.compact_autocommit().unwrap();
    assert_eq!(summary.format_flag, "--json");
    assert_eq!(summary.path, std::path::PathBuf::from(json_path));
    assert!(fs::metadata(log_path).is_err());

    let load_input = Cursor::new(format!(
        "session.load --json {json_path}\nnode.find User name=Alice\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut loaded_session = CliSession::new(load_input, output);
    loaded_session.run().await.unwrap();
    let (_, _, output) = loaded_session.into_parts();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("Node User userId=1 {name=Alice}"));

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn session_autocommit_checkpoints_after_log_threshold() {
    let json_path = "/tmp/grm-session-autocommit-threshold.json";
    let backup_path = "/tmp/grm-session-autocommit-threshold.json.bak";
    let log_path = "/tmp/grm-session-autocommit-threshold.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input = Cursor::new(format!(
        "session.autocommit --json {json_path}\nmodel.define User userId name:string:required\nnode.create User name=A1\nnode.create User name=A2\nnode.create User name=A3\nnode.create User name=A4\nnode.create User name=A5\nnode.create User name=A6\nnode.create User name=A7\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    let saved = fs::read_to_string(json_path).unwrap();

    assert!(saved.contains("A7"));
    assert!(fs::metadata(log_path).is_err());
    assert!(fs::metadata(backup_path).is_ok());

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
}

#[tokio::test]
async fn session_load_recovers_from_json_backup_when_primary_is_damaged() {
    let json_path = "/tmp/grm-session-recovery-test.json";
    let backup_path = "/tmp/grm-session-recovery-test.json.bak";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);

    let seed_input = Cursor::new(format!(
        "model.define User userId name:string:required\nnode.create User name=Alice\nsession.save --json {json_path}\nnode.create User name=Bob\nsession.save --json {json_path}\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut seed_session = CliSession::new(seed_input, output);
    seed_session.run().await.unwrap();

    fs::write(json_path, "{ not valid json").unwrap();

    let load_input = Cursor::new(format!(
        "session.load --json {json_path}\nnode.find User name=Alice\nnode.find User name=Bob\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut load_session = CliSession::new(load_input, output);
    load_session.run().await.unwrap();

    let (_, _, output) = load_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Recovered session from backup JSON file"));
    assert!(output.contains("Node User userId=1 {name=Alice}"));
    assert!(output.contains("Node User userId=2 {name=Bob}"));

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
}

#[tokio::test]
async fn session_load_recovers_from_backup_and_replays_log_entries() {
    let json_path = "/tmp/grm-session-recovery-log-test.json";
    let backup_path = "/tmp/grm-session-recovery-log-test.json.bak";
    let log_path = "/tmp/grm-session-recovery-log-test.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let input = Cursor::new(format!(
        "session.autocommit --json {json_path}\nmodel.define User userId name:string:required\nnode.create User name=Alice\nnode.create User name=Bob\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);
    session.run().await.unwrap();

    fs::write(json_path, "{ damaged primary").unwrap();

    let load_input = Cursor::new(format!(
        "session.load --json {json_path}\nnode.find User name=Alice\nnode.find User name=Bob\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut load_session = CliSession::new(load_input, output);
    load_session.run().await.unwrap();

    let (_, _, output) = load_session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Recovered session from backup JSON file"));
    assert!(output.contains("Node User userId=1 {name=Alice}"));
    assert!(output.contains("Node User userId=2 {name=Bob}"));
    assert!(fs::metadata(log_path).is_ok());

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn shared_durability_api_recovers_checkpoint_plus_wal_in_order() {
    let json_path = "/tmp/grm-shared-durability.json";
    let backup_path = "/tmp/grm-shared-durability.json.bak";
    let log_path = "/tmp/grm-shared-durability.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let mut state = SessionState::new();
    state
        .checkpoint_durable(DurabilityFormat::Json, json_path)
        .unwrap();

    let model = RuntimeNodeModel::new(
        "User",
        "userId",
        state.node_id_type(),
        vec![RuntimeField {
            name: "name".into(),
            value_type: RuntimeValueType::String,
            required: true,
        }],
    )
    .unwrap();
    state.register_model(model.clone()).unwrap();
    state
        .append_durable_operation(
            json_path,
            &DurableOperation::RegisterNodeModel {
                model: model.clone(),
            },
        )
        .unwrap();

    for name in ["Alice", "Bob"] {
        let node = state
            .create_instance("User", &BTreeMap::from([("name".into(), name.into())]))
            .await
            .unwrap();
        state
            .append_durable_operation(json_path, &DurableOperation::UpsertNode { node })
            .unwrap();
    }

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(DurabilityFormat::Json, json_path)
        .unwrap();

    let users = recovered
        .find_nodes("User", &BTreeMap::new())
        .unwrap()
        .into_iter()
        .map(|node| node.props["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    assert_eq!(users, vec!["Alice", "Bob"]);

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn shared_durability_ignores_truncated_final_wal_record() {
    let json_path = "/tmp/grm-shared-durability-truncated.json";
    let backup_path = "/tmp/grm-shared-durability-truncated.json.bak";
    let log_path = "/tmp/grm-shared-durability-truncated.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let mut state = SessionState::new();
    state
        .checkpoint_durable(DurabilityFormat::Json, json_path)
        .unwrap();
    let model = RuntimeNodeModel::new(
        "User",
        "userId",
        state.node_id_type(),
        vec![RuntimeField {
            name: "name".into(),
            value_type: RuntimeValueType::String,
            required: true,
        }],
    )
    .unwrap();
    state.register_model(model.clone()).unwrap();
    state
        .append_durable_operation(json_path, &DurableOperation::RegisterNodeModel { model })
        .unwrap();
    let node = state
        .create_instance("User", &BTreeMap::from([("name".into(), "Alice".into())]))
        .await
        .unwrap();
    state
        .append_durable_operation(json_path, &DurableOperation::UpsertNode { node })
        .unwrap();
    fs::OpenOptions::new()
        .append(true)
        .open(log_path)
        .unwrap()
        .write_all(br#"{"RegisterNodeModel":"#)
        .unwrap();

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(DurabilityFormat::Json, json_path)
        .unwrap();
    assert_eq!(
        recovered
            .find_nodes("User", &BTreeMap::from([("name".into(), "Alice".into())]))
            .unwrap()
            .len(),
        1
    );

    let indexes = recovered.index_catalog_value();
    assert!(
        indexes["indexes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|index| { index["durable"] == json!(false) })
    );

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[test]
fn shared_durability_ignores_truncated_batch_wal_record() {
    let json_path = "/tmp/grm-shared-durability-truncated-batch.json";
    let backup_path = "/tmp/grm-shared-durability-truncated-batch.json.bak";
    let log_path = "/tmp/grm-shared-durability-truncated-batch.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let state = SessionState::new();
    state
        .checkpoint_durable(DurabilityFormat::Json, json_path)
        .unwrap();
    let model = RuntimeNodeModel::new(
        "User",
        "userId",
        state.node_id_type(),
        vec![RuntimeField {
            name: "name".into(),
            value_type: RuntimeValueType::String,
            required: true,
        }],
    )
    .unwrap();
    let batch = DurableOperation::Batch {
        ops: vec![DurableOperation::RegisterNodeModel { model }],
    };
    let bytes = serde_json::to_vec(&batch).unwrap();
    fs::write(log_path, &bytes[..bytes.len() / 2]).unwrap();

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(DurabilityFormat::Json, json_path)
        .unwrap();
    assert!(recovered.model("User").is_none());

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[test]
fn shared_durability_reports_malformed_complete_wal_record() {
    let json_path = "/tmp/grm-shared-durability-malformed.json";
    let backup_path = "/tmp/grm-shared-durability-malformed.json.bak";
    let log_path = "/tmp/grm-shared-durability-malformed.json.log";
    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);

    let state = SessionState::new();
    state
        .checkpoint_durable(DurabilityFormat::Json, json_path)
        .unwrap();
    fs::write(log_path, b"{bad json}\n").unwrap();

    let mut recovered = SessionState::new();
    let err = recovered
        .recover_durable(DurabilityFormat::Json, json_path)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("malformed durable append log record")
    );

    let _ = fs::remove_file(json_path);
    let _ = fs::remove_file(backup_path);
    let _ = fs::remove_file(log_path);
}

#[tokio::test]
async fn session_describe_summarizes_current_state() {
    let input = Cursor::new(
        "model.define User userId name:string:required\nmodel.define Post postId title:string:required\nlink.define Authored User Post authoredId year:int:required\nnode.create User name=Alice\nnode.create Post title=Hello\nedge.create Authored from=1 to=2 year=2024\nsession.describe\nsession.exit\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    assert!(output.contains("Session Summary"));
    assert!(output.contains("Types defined:"));
    assert!(output.contains("nodes:"));
    assert!(output.contains("User"));
    assert!(output.contains("Post"));
    assert!(output.contains("links: Authored"));
    assert!(output.contains("Stored rows: 2 nodes, 1 edges"));
    assert!(output.contains("| node |"));
    assert!(output.contains("| edge |"));
    assert!(output.contains("Autocommit: off"));
}
