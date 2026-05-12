use std::collections::BTreeMap;

use grm_rs::{SessionBatchParams, SessionState, apply_session_batch};
use serde_json::json;

#[tokio::test]
async fn batch_creates_graph_with_refs_and_details() {
    let mut state = SessionState::new();
    let outcome = apply_session_batch(
        &mut state,
        serde_json::from_value::<SessionBatchParams>(json!({
            "response": "detailed",
            "ops": [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "User",
                        "id_field": "userId",
                        "fields": [{ "name": "name", "type": "string", "required": true }]
                    }
                },
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "Post",
                        "id_field": "postId",
                        "fields": [{ "name": "title", "type": "string", "required": true }]
                    }
                },
                {
                    "op": "schema_define_edge",
                    "args": {
                        "name": "Authored",
                        "from_model": "User",
                        "to_model": "Post",
                        "id_field": "authoredId",
                        "fields": []
                    }
                },
                { "op": "node_create", "args": { "model": "User", "props": { "name": "Alice" }, "ref": "alice" } },
                { "op": "node_create", "args": { "model": "Post", "props": { "title": "Hello" }, "ref": "post" } },
                { "op": "edge_create", "args": { "model": "Authored", "from": "alice", "to": "post" } }
            ]
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    assert!(outcome.should_persist);
    assert_eq!(outcome.value["applied"], true);
    assert_eq!(outcome.value["counts"]["edge_create"]["Authored"], 1);
    assert_eq!(outcome.value["ids"].as_array().unwrap().len(), 3);
    assert_eq!(
        state
            .find_relationships("Authored", &BTreeMap::new())
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn failed_atomic_batch_restores_state() {
    let mut state = SessionState::new();
    apply_session_batch(
        &mut state,
        serde_json::from_value::<SessionBatchParams>(json!({
            "ops": [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "Note",
                        "id_field": "noteId",
                        "fields": [{ "name": "title", "type": "string", "required": true }]
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    let outcome = apply_session_batch(
        &mut state,
        serde_json::from_value::<SessionBatchParams>(json!({
            "atomic": true,
            "ops": [
                { "op": "node_create", "args": { "model": "Note", "props": { "title": "Rollback" } } },
                { "op": "node_create", "args": { "model": "Note", "props": {} } }
            ]
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    assert!(!outcome.should_persist);
    assert_eq!(outcome.value["applied"], false);
    assert_eq!(outcome.value["errors"][0]["index"], 1);
    assert_eq!(
        state
            .find_nodes(
                "Note",
                &BTreeMap::from([("title".into(), "Rollback".into())])
            )
            .unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn non_atomic_batch_keeps_successes() {
    let mut state = SessionState::new();
    apply_session_batch(
        &mut state,
        serde_json::from_value::<SessionBatchParams>(json!({
            "ops": [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "Note",
                        "id_field": "noteId",
                        "fields": [{ "name": "title", "type": "string", "required": true }]
                    }
                }
            ]
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    let outcome = apply_session_batch(
        &mut state,
        serde_json::from_value::<SessionBatchParams>(json!({
            "atomic": false,
            "ops": [
                { "op": "node_create", "args": { "model": "Note", "props": { "title": "Kept" } } },
                { "op": "node_create", "args": { "model": "Note", "props": {} } }
            ]
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    assert!(outcome.should_persist);
    assert_eq!(outcome.value["applied"], false);
    assert_eq!(outcome.value["counts"]["node_create"]["Note"], 1);
    assert_eq!(
        state
            .find_nodes("Note", &BTreeMap::from([("title".into(), "Kept".into())]))
            .unwrap()
            .len(),
        1
    );
}
