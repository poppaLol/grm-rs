use grm_service_api::proto;

type GrmClient = proto::grm_service_client::GrmServiceClient<tonic::transport::Channel>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let endpoint = args
        .next()
        .unwrap_or_else(|| "http://127.0.0.1:50051".into());
    let workspace_id = args.next().unwrap_or_else(|| "demo-workspace".into());
    let workspace = proto::WorkspaceRef { id: workspace_id };

    let mut client = proto::grm_service_client::GrmServiceClient::connect(endpoint).await?;

    let unsupported = client
        .schema_list(proto::SchemaListRequest {})
        .await
        .expect_err("direct non-workspace RPCs are unsupported in the shell");
    println!(
        "direct SchemaList RPC: {} {}",
        unsupported.code(),
        unsupported.message()
    );

    let created = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
            workspace: Some(workspace.clone()),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await?
        .into_inner();
    let handle = created
        .handle
        .expect("create workspace should return a handle");
    println!("created local autocommit workspace: {}", handle.id);

    define_demo_schema(&mut client, &handle).await?;
    let schema = execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::SchemaList(proto::SchemaListRequest {}),
    )
    .await?;
    print_schema(schema);

    let user_id = created_node_id(
        execute_workspace(
            &mut client,
            &handle,
            proto::runtime_request::Request::CreateNode(node_create(
                "User",
                [(
                    "name",
                    proto::property_value::Kind::StringValue("Ada".into()),
                )],
            )),
        )
        .await?,
    );
    let post_id = created_node_id(
        execute_workspace(
            &mut client,
            &handle,
            proto::runtime_request::Request::CreateNode(node_create(
                "Post",
                [(
                    "title",
                    proto::property_value::Kind::StringValue("Service workspace notes".into()),
                )],
            )),
        )
        .await?,
    );
    println!("created user {user_id} and post {post_id}");

    execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::UpdateNode(proto::NodeUpdateRequest {
            model: "User".into(),
            id: user_id,
            props: Some(property_map([(
                "name",
                proto::property_value::Kind::StringValue("Ada Lovelace".into()),
            )])),
        }),
    )
    .await?;
    let users = execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::FindNodes(find_nodes_by_id("User", user_id)),
    )
    .await?;
    println!(
        "found users after update: {:?}",
        node_string_props(users, "name")
    );

    let edge_id = created_edge_id(
        execute_workspace(
            &mut client,
            &handle,
            proto::runtime_request::Request::CreateEdge(proto::EdgeCreateRequest {
                model: "Authored".into(),
                from: user_id,
                to: post_id,
                props: Some(property_map([(
                    "year",
                    proto::property_value::Kind::IntValue(2026),
                )])),
            }),
        )
        .await?,
    );
    execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::UpdateEdge(proto::EdgeUpdateRequest {
            model: "Authored".into(),
            id: edge_id,
            props: Some(property_map([(
                "year",
                proto::property_value::Kind::IntValue(2027),
            )])),
        }),
    )
    .await?;
    let edges = execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::FindEdges(find_edges_by_id("Authored", edge_id)),
    )
    .await?;
    println!(
        "found authored edge years: {:?}",
        edge_int_props(edges, "year")
    );

    let temp_post = created_node_id(
        execute_workspace(
            &mut client,
            &handle,
            proto::runtime_request::Request::CreateNode(node_create(
                "Post",
                [(
                    "title",
                    proto::property_value::Kind::StringValue("Temporary".into()),
                )],
            )),
        )
        .await?,
    );
    let temp_edge = created_edge_id(
        execute_workspace(
            &mut client,
            &handle,
            proto::runtime_request::Request::CreateEdge(proto::EdgeCreateRequest {
                model: "Authored".into(),
                from: user_id,
                to: temp_post,
                props: Some(property_map([(
                    "year",
                    proto::property_value::Kind::IntValue(2026),
                )])),
            }),
        )
        .await?,
    );
    execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::DeleteEdge(proto::EdgeDeleteRequest {
            model: "Authored".into(),
            id: temp_edge,
        }),
    )
    .await?;
    execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::DeleteNode(proto::NodeDeleteRequest {
            model: "Post".into(),
            id: temp_post,
        }),
    )
    .await?;
    println!("deleted temporary edge {temp_edge} and post {temp_post}");

    let batch = execute_workspace(
        &mut client,
        &handle,
        proto::runtime_request::Request::ApplyBatch(proto::BatchRequest {
            atomic: true,
            allow_deletes: false,
            response_mode: proto::BatchResponseMode::Detailed as i32,
            ops: vec![
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineNode(define_node(
                        "Tag",
                        "tagId",
                        [("label", proto::FieldValueType::String, true)],
                    ))),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::SchemaDefineEdge(define_edge(
                        "Tagged",
                        "User",
                        "Tag",
                        "taggedId",
                        [],
                    ))),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::NodeCreate(
                        proto::BatchNodeCreate {
                            model: "Tag".into(),
                            props: Some(property_map([(
                                "label",
                                proto::property_value::Kind::StringValue("service".into()),
                            )])),
                            local_ref: Some("service-tag".into()),
                        },
                    )),
                },
                proto::BatchOperation {
                    op: Some(proto::batch_operation::Op::EdgeCreate(
                        proto::BatchEdgeCreate {
                            model: "Tagged".into(),
                            from: Some(proto::BatchEndpoint {
                                endpoint: Some(proto::batch_endpoint::Endpoint::Id(user_id)),
                            }),
                            to: Some(proto::BatchEndpoint {
                                endpoint: Some(proto::batch_endpoint::Endpoint::LocalRef(
                                    "service-tag".into(),
                                )),
                            }),
                            props: None,
                        },
                    )),
                },
            ],
        }),
    )
    .await?;
    print_batch(batch);

    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle),
        })
        .await?;
    println!("closed workspace");

    let reopened = client
        .open_workspace(proto::WorkspaceOpenRequest {
            snapshot: None,
            workspace: Some(workspace),
            format: proto::DurabilityFormat::Binary as i32,
        })
        .await?
        .into_inner();
    let reopened_handle = reopened
        .handle
        .expect("open workspace should return a handle");
    let users = execute_workspace(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindNodes(find_all_nodes("User")),
    )
    .await?;
    println!(
        "reopened workspace users: {:?}",
        node_string_props(users, "name")
    );
    let tags = execute_workspace(
        &mut client,
        &reopened_handle,
        proto::runtime_request::Request::FindNodes(find_all_nodes("Tag")),
    )
    .await?;
    println!(
        "reopened workspace tags: {:?}",
        node_string_props(tags, "label")
    );

    client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(reopened_handle),
        })
        .await?;

    Ok(())
}

async fn define_demo_schema(
    client: &mut GrmClient,
    handle: &proto::WorkspaceHandle,
) -> Result<(), tonic::Status> {
    execute_workspace(
        client,
        handle,
        proto::runtime_request::Request::DefineNode(define_node(
            "User",
            "userId",
            [("name", proto::FieldValueType::String, true)],
        )),
    )
    .await?;
    execute_workspace(
        client,
        handle,
        proto::runtime_request::Request::DefineNode(define_node(
            "Post",
            "postId",
            [("title", proto::FieldValueType::String, true)],
        )),
    )
    .await?;
    execute_workspace(
        client,
        handle,
        proto::runtime_request::Request::DefineEdge(define_edge(
            "Authored",
            "User",
            "Post",
            "authoredId",
            [("year", proto::FieldValueType::Int, false)],
        )),
    )
    .await?;
    Ok(())
}

async fn execute_workspace(
    client: &mut GrmClient,
    handle: &proto::WorkspaceHandle,
    request: proto::runtime_request::Request,
) -> Result<proto::WorkspaceRuntimeResponse, tonic::Status> {
    client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(request),
            }),
        })
        .await
        .map(|response| response.into_inner())
}

fn define_node<const N: usize>(
    name: &str,
    id_field: &str,
    fields: [(&str, proto::FieldValueType, bool); N],
) -> proto::DefineNodeRequest {
    proto::DefineNodeRequest {
        name: name.into(),
        id_field: id_field.into(),
        fields: fields
            .into_iter()
            .map(|(name, value_type, required)| proto::FieldSpec {
                name: name.into(),
                value_type: value_type as i32,
                required,
            })
            .collect(),
    }
}

fn define_edge<const N: usize>(
    name: &str,
    from_model: &str,
    to_model: &str,
    id_field: &str,
    fields: [(&str, proto::FieldValueType, bool); N],
) -> proto::DefineEdgeRequest {
    proto::DefineEdgeRequest {
        name: name.into(),
        from_model: from_model.into(),
        to_model: to_model.into(),
        id_field: id_field.into(),
        fields: fields
            .into_iter()
            .map(|(name, value_type, required)| proto::FieldSpec {
                name: name.into(),
                value_type: value_type as i32,
                required,
            })
            .collect(),
    }
}

fn node_create<const N: usize>(
    model: &str,
    props: [(&str, proto::property_value::Kind); N],
) -> proto::NodeCreateRequest {
    proto::NodeCreateRequest {
        model: model.into(),
        props: Some(property_map(props)),
    }
}

fn find_all_nodes(model: &str) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        model: model.into(),
        predicates: Vec::new(),
        end_predicates: Vec::new(),
        edge_predicates: Vec::new(),
        traversals: Vec::new(),
        order: Vec::new(),
        limit: None,
        offset: None,
        id: None,
        return_mode: None,
    }
}

fn find_nodes_by_id(model: &str, id: i64) -> proto::NodeFindRequest {
    proto::NodeFindRequest {
        id: Some(id),
        ..find_all_nodes(model)
    }
}

fn find_edges_by_id(model: &str, id: i64) -> proto::EdgeFindRequest {
    proto::EdgeFindRequest {
        model: model.into(),
        predicates: Vec::new(),
        order: Vec::new(),
        limit: None,
        offset: None,
        id: Some(id),
        from: None,
        to: None,
    }
}

fn created_node_id(response: proto::WorkspaceRuntimeResponse) -> i64 {
    match response.response.and_then(|response| response.response) {
        Some(proto::runtime_response::Response::CreateNode(response)) => {
            response.node.expect("create node should return node").id
        }
        other => panic!("expected create node response, got {other:?}"),
    }
}

fn created_edge_id(response: proto::WorkspaceRuntimeResponse) -> i64 {
    match response.response.and_then(|response| response.response) {
        Some(proto::runtime_response::Response::CreateEdge(response)) => {
            response.edge.expect("create edge should return edge").id
        }
        other => panic!("expected create edge response, got {other:?}"),
    }
}

fn print_schema(response: proto::WorkspaceRuntimeResponse) {
    let Some(proto::runtime_response::Response::SchemaList(schema)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected schema list response");
    };
    let nodes = schema
        .node_models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    let edges = schema
        .edge_models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    println!("schema nodes: {nodes:?}; edges: {edges:?}");
}

fn print_batch(response: proto::WorkspaceRuntimeResponse) {
    let Some(proto::runtime_response::Response::ApplyBatch(batch)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected batch response");
    };
    println!(
        "batch applied: {}; ids: {:?}",
        batch.applied,
        batch
            .ids
            .iter()
            .map(|id| (&id.op, &id.model, id.id, id.local_ref.as_deref()))
            .collect::<Vec<_>>()
    );
}

fn node_string_props(response: proto::WorkspaceRuntimeResponse, field: &str) -> Vec<String> {
    let Some(proto::runtime_response::Response::FindNodes(response)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected node find response");
    };
    response
        .nodes
        .into_iter()
        .filter_map(|node| string_prop(node.props, field))
        .collect()
}

fn edge_int_props(response: proto::WorkspaceRuntimeResponse, field: &str) -> Vec<i64> {
    let Some(proto::runtime_response::Response::FindEdges(response)) =
        response.response.and_then(|response| response.response)
    else {
        panic!("expected edge find response");
    };
    response
        .edges
        .into_iter()
        .filter_map(|edge| int_prop(edge.props, field))
        .collect()
}

fn string_prop(props: Option<proto::PropertyMap>, field: &str) -> Option<String> {
    props?
        .properties
        .into_iter()
        .find(|property| property.name == field)
        .and_then(|property| property.value)
        .and_then(|value| match value.kind {
            Some(proto::property_value::Kind::StringValue(value)) => Some(value),
            _ => None,
        })
}

fn int_prop(props: Option<proto::PropertyMap>, field: &str) -> Option<i64> {
    props?
        .properties
        .into_iter()
        .find(|property| property.name == field)
        .and_then(|property| property.value)
        .and_then(|value| match value.kind {
            Some(proto::property_value::Kind::IntValue(value)) => Some(value),
            _ => None,
        })
}

fn property_map<const N: usize>(
    properties: [(&str, proto::property_value::Kind); N],
) -> proto::PropertyMap {
    proto::PropertyMap {
        properties: properties
            .into_iter()
            .map(|(name, kind)| proto::Property {
                name: name.into(),
                value: Some(proto::PropertyValue { kind: Some(kind) }),
            })
            .collect(),
    }
}
