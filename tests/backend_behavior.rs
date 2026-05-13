mod common;

use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

use grm_rs::{InMemoryBackend, Neo4jBackend, Neo4jConfig, Result};
use neo4rs::{Graph, query};

#[tokio::test]
async fn in_memory_backend_satisfies_shared_behavior() -> Result<()> {
    common::run_shared_backend_behavior(InMemoryBackend::new()).await
}

#[tokio::test]
#[ignore = "requires a running Neo4j Bolt endpoint and NEO4J_* env vars"]
async fn neo4j_backend_satisfies_shared_behavior_when_env_is_set() -> Result<()> {
    let Some(config) = neo4j_config_from_env() else {
        eprintln!("skipping Neo4j behavior test; set NEO4J_URI, NEO4J_USER, and NEO4J_PASSWORD");
        return Ok(());
    };

    let run_id = unique_behavior_run_id();
    let graph = Graph::new(&config.uri, &config.user, &config.password)
        .await
        .expect("connect Neo4j graph for cleanup");
    cleanup_behavior_graph(&graph, &run_id).await;

    let backend = Neo4jBackend::connect(config)
        .await
        .expect("connect Neo4j backend");
    let result = common::run_shared_backend_behavior_with_config(
        backend,
        common::BackendBehaviorConfig {
            run_id: run_id.clone(),
            native_query: common::NativeQueryExpectation::Supported,
        },
    )
    .await;

    cleanup_behavior_graph(&graph, &run_id).await;
    result
}

fn neo4j_config_from_env() -> Option<Neo4jConfig> {
    Some(Neo4jConfig {
        uri: env::var("NEO4J_URI").ok()?,
        user: env::var("NEO4J_USER").ok()?,
        password: env::var("NEO4J_PASSWORD").ok()?,
    })
}

async fn cleanup_behavior_graph(graph: &Graph, run_id: &str) {
    let text = format!(
        "MATCH (n {{{}: $run_id}}) DETACH DELETE n",
        common::BEHAVIOR_RUN_ID_PROP
    );
    graph
        .run(query(&text).param("run_id", run_id.to_string()))
        .await
        .expect("cleanup Neo4j behavior graph");
}

fn unique_behavior_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("grm-rs-behavior-{nanos}")
}
