use std::fs;

use grm_rs::{
    BackendIdType, DurableOperation, RuntimeField, RuntimeNodeModel, RuntimeRelModel,
    RuntimeValueType, SessionState,
};
use serde_json::json;

#[test]
fn schema_memory_file_recovers_node_and_edge_models_from_durable_ops() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("project-memory-schema.json");

    let state = SessionState::new();
    state
        .checkpoint_durable(grm_rs::DurabilityFormat::Json, &path)
        .unwrap();

    let roadmap = RuntimeNodeModel::new(
        "RoadmapItem",
        "roadmapItemId",
        BackendIdType::Int64,
        vec![
            RuntimeField {
                name: "title".into(),
                value_type: RuntimeValueType::String,
                required: true,
            },
            RuntimeField {
                name: "status".into(),
                value_type: RuntimeValueType::String,
                required: false,
            },
        ],
    )
    .unwrap();
    let work_slice =
        RuntimeNodeModel::new("WorkSlice", "workSliceId", BackendIdType::Int64, vec![]).unwrap();
    let next_step = RuntimeRelModel::new(
        "NEXT_STEP",
        "RoadmapItem",
        "WorkSlice",
        "nextStepId",
        BackendIdType::Int64,
        vec![RuntimeField {
            name: "reason".into(),
            value_type: RuntimeValueType::String,
            required: false,
        }],
    )
    .unwrap();

    state
        .append_durable_operation(
            &path,
            &DurableOperation::Batch {
                ops: vec![
                    DurableOperation::RegisterNodeModel { model: roadmap },
                    DurableOperation::RegisterNodeModel { model: work_slice },
                    DurableOperation::RegisterRelModel { model: next_step },
                ],
            },
        )
        .unwrap();

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(grm_rs::DurabilityFormat::Json, &path)
        .unwrap();

    assert!(recovered.model("RoadmapItem").is_some());
    assert!(recovered.model("WorkSlice").is_some());
    let edge = recovered.rel_model("NEXT_STEP").unwrap();
    assert_eq!(edge.from_model, "RoadmapItem");
    assert_eq!(edge.to_model, "WorkSlice");
    assert_eq!(recovered.summary_value()["nodes"]["total"], json!(0));
    assert_eq!(recovered.summary_value()["edges"]["total"], json!(0));
}

#[test]
fn schema_memory_missing_file_can_start_fresh_with_checkpoint() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("new-schema-memory.json");

    let state = SessionState::new();
    state
        .checkpoint_durable(grm_rs::DurabilityFormat::Json, &path)
        .unwrap();

    let mut recovered = SessionState::new();
    recovered
        .recover_durable(grm_rs::DurabilityFormat::Json, &path)
        .unwrap();

    assert!(path.exists());
    assert!(recovered.catalog().is_empty());
}

#[test]
fn invalid_schema_memory_file_fails_on_recover() {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join("bad-schema-memory.json");
    fs::write(&path, "{ not json").unwrap();

    let mut state = SessionState::new();
    let err = state
        .recover_durable(grm_rs::DurabilityFormat::Json, &path)
        .unwrap_err();

    assert!(err.to_string().contains("cannot load from file"));
}
