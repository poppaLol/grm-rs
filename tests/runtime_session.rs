use std::collections::BTreeMap;
use std::io::Cursor;

use grm_rs::{
    BackendIdType, CliSession, RuntimeField, RuntimeNodeModel, RuntimeValueType,
    SessionModelCatalog, SessionState,
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
fn model_name_collisions_are_rejected() {
    let mut catalog = SessionModelCatalog::new();
    let model = RuntimeNodeModel::new("User", "userId", BackendIdType::Int64, vec![]).unwrap();
    catalog.register(model.clone()).unwrap();

    let err = catalog.register(model).unwrap_err();
    assert!(err.to_string().contains("already exists"));
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
async fn guided_model_creation_and_listing_work() {
    let input = Cursor::new(
        "model create\nUser\nuserId\nname\nstring\ny\nage\nint\nn\ndone\ny\nn\nmodel list\nmodel show User\nexit\n",
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
    let input = Cursor::new("model create\nUser\nuserId\nname\nstring\ny\ndone\nn\nexit\n");
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
    let input = Cursor::new("model create\nUser\nuserId\nname\nstring\ny\ndone\ny\ny\nAlice\nexit\n");
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
        "# setup models\n\nmodel define User userId name:string:required age:int:optional\nmodel list\nmodel show User\n",
    );
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    session.run_script().await.unwrap();

    let (state, _, output) = session.into_parts();
    let output = String::from_utf8(output).unwrap();

    let model = state.model("User").unwrap();
    assert_eq!(model.id_field_name, "userId");
    assert_eq!(model.fields.len(), 2);
    assert!(output.contains("Model 'User' created from script."));
    assert!(output.contains("Id: userId (int)"));
}

#[tokio::test]
async fn script_mode_rejects_bad_field_specs() {
    let input = Cursor::new("model define User userId name:string:maybe\n");
    let output = Vec::new();
    let mut session = CliSession::new(input, output);

    let err = session.run_script().await.unwrap_err();
    assert!(err.to_string().contains("invalid field requirement"));
}
