use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;

use grm_rs::{
    BackendIdType, CliSession, RuntimeField, RuntimeNodeModel, RuntimeValueType,
    RuntimeRelModel, SessionModelCatalog, SessionState,
};

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
        .register_model(RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap())
        .unwrap();
    state
        .register_model(RuntimeNodeModel::new("Post", "postId", BackendIdType::Int64, vec![]).unwrap())
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
        .register_model(RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap())
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
    let err =
        RuntimeNodeModel::new("user", "userId", BackendIdType::Int64, vec![]).unwrap_err();
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
    let err = state.create_instance("User", &wrong_type).await.unwrap_err();
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

    let user = state.create_instance("User", &BTreeMap::new()).await.unwrap();
    let wrong_to = state.create_instance("User", &BTreeMap::new()).await.unwrap();

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
    let input = Cursor::new("model.define\nUser\nuserId\nname\nstring\ny\ndone\ny\ny\nAlice\nsession.exit\n");
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
    assert!(output.contains("Model 'User' created from script."));
    assert!(output.contains("Link 'Authored' created from script."));
    assert!(output.contains("Id: userId (int)"));
    assert!(output.contains("Session links:"));
    assert!(output.contains("Link: Authored"));
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

    let alice_pos = output.find("Node User userId=2 {age=42 name=Alice}").unwrap();
    let bob_pos = output.find("Node User userId=1 {age=42 name=Bob}").unwrap();
    let carol_pos = output.find("Node User userId=3 {age=35 name=Carol}").unwrap();

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

    let rel_2025 = output.find("Edge Authored authoredId=3 from=1 to=4 {year=2025}").unwrap();
    let rel_to_2 = output.find("Edge Authored authoredId=1 from=1 to=2 {year=2024}").unwrap();
    let rel_to_3 = output.find("Edge Authored authoredId=2 from=1 to=3 {year=2024}").unwrap();

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

    assert!(output.contains("Updated edge Authored authoredId=1 from=1 to=2 {authoredOn=2026-04-12}"));
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
async fn session_autocommit_persists_changes_until_disabled() {
    let json_path = "/tmp/grm-session-autocommit-test.json";
    let _ = fs::remove_file(json_path);

    let input = Cursor::new(format!(
        "session.autocommit status\nsession.autocommit --json {json_path}\nmodel.define User userId name:string:required\nnode.create User name=Alice\nsession.autocommit status\nsession.autocommit off\nnode.create User name=Bob\nsession.autocommit status\nsession.exit\n"
    ));
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run().await.unwrap();

    let (_, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();
    let saved = fs::read_to_string(json_path).unwrap();

    assert!(output.contains("Autocommit is disabled."));
    assert!(output.contains(&format!("Autocommit enabled: --json {json_path}")));
    assert!(output.contains("Autocommit disabled."));
    assert!(saved.contains("Alice"));
    assert!(saved.contains("User"));
    assert!(!saved.contains("Bob"));

    let _ = fs::remove_file(json_path);
}

#[tokio::test]
async fn session_load_triggers_autocommit_target_update() {
    let source_path = "/tmp/grm-session-autocommit-source.json";
    let target_path = "/tmp/grm-session-autocommit-target.json";
    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(target_path);

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

    let _ = fs::remove_file(source_path);
    let _ = fs::remove_file(target_path);
}

#[tokio::test]
async fn session_autocommit_supports_binary_targets() {
    let bin_path = "/tmp/grm-session-autocommit-test.bin";
    let _ = fs::remove_file(bin_path);

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
    assert!(output.contains("Node User userId=1 {name=Alice}"));

    let _ = fs::remove_file(bin_path);
}
