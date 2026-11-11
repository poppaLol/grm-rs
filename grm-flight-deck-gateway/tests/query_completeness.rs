use std::collections::BTreeMap;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use grm_rs::{
    DefineEdgeRequest, DefineNodeRequest, EdgeCreateRequest, FieldSpec, FieldValueType,
    NodeCreateRequest,
};
use grm_service_api::{
    GrpcWorkspaceClient, GrpcWorkspaceMode, GrpcWorkspaceService, ServiceSecurityConfig,
};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

struct Gateway {
    process: Child,
    service: tokio::task::JoinHandle<()>,
    _workspace_root: tempfile::TempDir,
    client: reqwest::Client,
    url: String,
}

impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        self.service.abort();
    }
}

impl Gateway {
    async fn start() -> Self {
        // Both servers and all data belong exclusively to this test.
        let workspace_root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let service = GrpcWorkspaceService::with_local_workspace_root(
            workspace_root.path(),
            ServiceSecurityConfig::anonymous_local(),
        );
        let service = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(service.into_server())
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let mut workspace = GrpcWorkspaceClient::connect(
            &endpoint,
            "query-completeness",
            GrpcWorkspaceMode::Create,
        )
        .await
        .unwrap();
        for model in ["Item", "Other"] {
            workspace
                .define_node(DefineNodeRequest {
                    name: model.into(),
                    id_field: "itemId".into(),
                    fields: vec![FieldSpec {
                        name: "status".into(),
                        value_type: FieldValueType::String,
                        required: true,
                    }],
                })
                .await
                .unwrap();
        }
        workspace
            .define_edge(DefineEdgeRequest {
                name: "LINKS".into(),
                from_model: "Item".into(),
                to_model: "Other".into(),
                id_field: "linkId".into(),
                fields: vec![FieldSpec {
                    name: "kind".into(),
                    value_type: FieldValueType::String,
                    required: true,
                }],
            })
            .await
            .unwrap();
        let other = workspace
            .create_node(NodeCreateRequest {
                model: "Other".into(),
                props: BTreeMap::from([("status".into(), json!("unrelated"))]),
            })
            .await
            .unwrap();
        for (status, kind) in [("active", "keep"), ("active", "keep"), ("paused", "skip")] {
            let item = workspace
                .create_node(NodeCreateRequest {
                    model: "Item".into(),
                    props: BTreeMap::from([("status".into(), json!(status))]),
                })
                .await
                .unwrap();
            workspace
                .create_edge(EdgeCreateRequest {
                    model: "LINKS".into(),
                    from: item.id,
                    to: other.id,
                    props: BTreeMap::from([("kind".into(), json!(kind))]),
                })
                .await
                .unwrap();
            if status == "paused" {
                workspace
                    .define_edge(DefineEdgeRequest {
                        name: "OUTSIDE".into(),
                        from_model: "Item".into(),
                        to_model: "Other".into(),
                        id_field: "outsideId".into(),
                        fields: Vec::new(),
                    })
                    .await
                    .unwrap();
                workspace
                    .create_edge(EdgeCreateRequest {
                        model: "OUTSIDE".into(),
                        from: item.id,
                        to: other.id,
                        props: BTreeMap::new(),
                    })
                    .await
                    .unwrap();
            }
        }
        workspace.close().await.unwrap();

        let http_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let http_addr = http_listener.local_addr().unwrap();
        drop(http_listener);
        let process = Command::new(env!("CARGO_BIN_EXE_grm-flight-deck-gateway"))
            .env_clear()
            .env("GRM_SERVICE_ENDPOINT", endpoint)
            .env("GRM_FLIGHT_DECK_GATEWAY_BIND", http_addr.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let gateway = Self {
            process,
            service,
            _workspace_root: workspace_root,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
            url: format!("http://{http_addr}/api/workspaces/query-completeness"),
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if gateway
                    .client
                    .get(format!("{}/snapshot", gateway.url))
                    .send()
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("gateway did not start");
        gateway
    }

    async fn query(&self, command: &str) -> Value {
        self.client
            .post(format!("{}/query", self.url))
            .json(&json!({"command": command}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn query_completeness_uses_matching_rows_instead_of_workspace_totals() {
    let gateway = Gateway::start().await;
    for (command, nodes, edges, truncated_nodes, truncated_edges) in [
        (
            "node.find Item status=active order=itemId:asc limit=5",
            2,
            0,
            json!([]),
            json!([]),
        ),
        (
            "node.find Item status=active order=itemId:asc limit=2",
            2,
            0,
            json!([]),
            json!([]),
        ),
        (
            "node.find Item status=active order=itemId:asc limit=1",
            1,
            0,
            json!(["Item"]),
            json!([]),
        ),
        (
            "node.find Item status=active order=itemId:asc offset=1 limit=1",
            1,
            0,
            json!([]),
            json!([]),
        ),
        (
            "node.find Item status=missing limit=1",
            0,
            0,
            json!([]),
            json!([]),
        ),
        (
            "node.find Other via=in:LINKS:Item return=end order=itemId:asc limit=5",
            3,
            0,
            json!([]),
            json!([]),
        ),
        (
            "node.find Other via=in:LINKS:Item return=end order=itemId:asc limit=1",
            1,
            0,
            json!(["Item"]),
            json!([]),
        ),
        (
            "edge.find LINKS kind=keep order=linkId:asc limit=2",
            3,
            2,
            json!([]),
            json!([]),
        ),
        (
            "edge.find LINKS kind=keep order=linkId:asc limit=1",
            2,
            1,
            json!([]),
            json!(["LINKS"]),
        ),
        (
            "edge.find LINKS kind=keep order=linkId:asc offset=1 limit=1",
            2,
            1,
            json!([]),
            json!([]),
        ),
        (
            "edge.find LINKS kind=missing limit=1",
            0,
            0,
            json!([]),
            json!([]),
        ),
        (
            "session.explain node.find Item status=active limit=5",
            2,
            0,
            json!([]),
            json!([]),
        ),
        (
            "session.profile edge.find LINKS kind=keep limit=1",
            2,
            1,
            json!([]),
            json!(["LINKS"]),
        ),
    ] {
        let response = gateway.query(command).await;
        let result = &response["result"];
        let summary = &result["summary"];
        let completeness = &summary["completeness"];
        assert_eq!(
            result["nodes"].as_array().unwrap().len(),
            nodes,
            "{command}"
        );
        assert_eq!(
            result["edges"].as_array().unwrap().len(),
            edges,
            "{command}"
        );
        assert_eq!(summary["fullWorkspace"]["nodeCount"], 4, "{command}");
        assert_eq!(summary["fullWorkspace"]["edgeCount"], 4, "{command}");
        assert_eq!(summary["loadedSnapshot"]["nodeCount"], nodes, "{command}");
        assert_eq!(summary["loadedSnapshot"]["edgeCount"], edges, "{command}");
        assert_eq!(
            completeness["truncatedNodeModels"], truncated_nodes,
            "{command}"
        );
        assert_eq!(
            completeness["truncatedEdgeModels"], truncated_edges,
            "{command}"
        );
        let complete = truncated_nodes == json!([]) && truncated_edges == json!([]);
        assert_eq!(completeness["complete"], complete, "{command}");
        if complete {
            assert!(response["partialReason"].is_null(), "{command}: {response}");
            assert!(result["partialReason"].is_null(), "{command}");
            assert!(completeness["reason"].is_null(), "{command}");
        } else {
            let reason = completeness["reason"].as_str().unwrap();
            assert!(reason.contains("query result limit"), "{command}: {reason}");
            assert!(!reason.contains("snapshot limit"), "{command}: {reason}");
            assert_eq!(
                response["partialReason"], completeness["reason"],
                "{command}"
            );
            assert_eq!(result["partialReason"], completeness["reason"], "{command}");
        }
    }

    // A workspace snapshot still reports limits against the entire workspace.
    let snapshot: Value = gateway
        .client
        .get(format!("{}/snapshot?limit=1", gateway.url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let completeness = &snapshot["summary"]["completeness"];
    assert_eq!(completeness["complete"], false);
    assert_eq!(completeness["truncatedNodeModels"], json!(["Item"]));
    assert_eq!(completeness["truncatedEdgeModels"], json!(["LINKS"]));
    assert!(
        completeness["reason"]
            .as_str()
            .unwrap()
            .contains("per-model snapshot limit")
    );
}

#[tokio::test]
async fn snapshot_counts_only_loaded_edges_and_separates_omissions_from_fetch_limits() {
    let gateway = Gateway::start().await;
    for limit in [1, 5] {
        let snapshot: Value = gateway
            .client
            .get(format!("{}/snapshot?limit={limit}", gateway.url))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let summary = &snapshot["summary"];
        let loaded = &summary["loadedSnapshot"];
        let completeness = &summary["completeness"];
        for (entities, counts, total) in [
            ("nodes", "nodeCounts", "nodeCount"),
            ("edges", "edgeCounts", "edgeCount"),
        ] {
            let actual = snapshot[entities].as_array().unwrap();
            assert_eq!(loaded[total], actual.len());
            let per_model = loaded[counts].as_object().unwrap();
            assert_eq!(
                per_model
                    .values()
                    .map(|count| count.as_u64().unwrap())
                    .sum::<u64>(),
                actual.len() as u64
            );
            for (model, count) in per_model {
                assert_eq!(
                    count,
                    &json!(
                        actual
                            .iter()
                            .filter(|entity| entity["model"] == *model)
                            .count()
                    )
                );
            }
        }
        assert_eq!(summary["fullWorkspace"]["edgeCounts"]["OUTSIDE"], 1);
        if limit == 1 {
            assert_eq!(loaded["edgeCounts"]["OUTSIDE"], 0);
            assert_eq!(snapshot["omittedEdges"], 1);
            assert_eq!(completeness["omittedEdges"], 1);
            assert_eq!(completeness["complete"], false);
            assert_eq!(completeness["truncatedEdgeModels"], json!(["LINKS"]));
            let reason = completeness["reason"].as_str().unwrap();
            assert!(reason.contains("1 relationship models exceeded"));
            assert!(reason.contains("1 relationships were omitted"));
        } else {
            assert_eq!(loaded["edgeCounts"]["OUTSIDE"], 1);
            assert_eq!(completeness["complete"], true);
            assert_eq!(completeness["truncatedEdgeModels"], json!([]));
            assert_eq!(snapshot["omittedEdges"], 0);
            assert!(snapshot["partialReason"].is_null());
        }
    }
}
