use grm_service_api::proto;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "http://127.0.0.1:50051".into());

    let mut client = proto::grm_service_client::GrmServiceClient::connect(endpoint).await?;

    let created = client
        .create_workspace(proto::WorkspaceCreateRequest {
            mode: proto::WorkspaceCreateMode::InMemory as i32,
            workspace: None,
            format: proto::DurabilityFormat::Json as i32,
        })
        .await?
        .into_inner();
    let handle = created
        .handle
        .expect("create workspace should return a handle");
    println!("created workspace handle: {}", handle.id);

    let schema = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::DefineNode(
                    proto::DefineNodeRequest {
                        name: "User".into(),
                        id_field: "userId".into(),
                        fields: vec![proto::FieldSpec {
                            name: "name".into(),
                            value_type: proto::FieldValueType::String as i32,
                            required: true,
                        }],
                    },
                )),
            }),
        })
        .await?
        .into_inner();
    println!(
        "schema response: {}",
        response_name(schema.response.as_ref())
    );

    let batch = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(handle.clone()),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::ApplyBatch(
                    proto::BatchRequest {
                        atomic: true,
                        allow_deletes: false,
                        response_mode: proto::BatchResponseMode::Detailed as i32,
                        ops: vec![proto::BatchOperation {
                            op: Some(proto::batch_operation::Op::NodeCreate(
                                proto::BatchNodeCreate {
                                    model: "User".into(),
                                    props: Some(property_map([(
                                        "name",
                                        proto::property_value::Kind::StringValue("Ada".into()),
                                    )])),
                                    local_ref: Some("ada".into()),
                                },
                            )),
                        }],
                    },
                )),
            }),
        })
        .await?
        .into_inner();
    println!("batch response: {}", response_name(batch.response.as_ref()));

    let missing = client
        .execute_workspace(proto::WorkspaceRuntimeRequest {
            handle: Some(proto::WorkspaceHandle {
                id: "missing-workspace".into(),
            }),
            request: Some(proto::RuntimeRequest {
                request: Some(proto::runtime_request::Request::SchemaList(
                    proto::SchemaListRequest {},
                )),
            }),
        })
        .await
        .expect_err("unknown handles should return an error");
    println!(
        "demo error, unknown handle: {} {}",
        missing.code(),
        missing.message()
    );

    let unsupported = client
        .schema_list(proto::SchemaListRequest {})
        .await
        .expect_err("direct non-workspace RPCs are unsupported in the shell");
    println!(
        "demo error, unsupported direct RPC: {} {}",
        unsupported.code(),
        unsupported.message()
    );

    let closed = client
        .close_workspace(proto::WorkspaceCloseRequest {
            handle: Some(handle.clone()),
        })
        .await?
        .into_inner();
    println!(
        "closed workspace handle: {}",
        closed.handle.expect("close should echo handle").id
    );

    Ok(())
}

fn response_name(response: Option<&proto::RuntimeResponse>) -> &'static str {
    match response.and_then(|response| response.response.as_ref()) {
        Some(proto::runtime_response::Response::DefineNode(_)) => "define_node",
        Some(proto::runtime_response::Response::ApplyBatch(_)) => "apply_batch",
        Some(_) => "other",
        None => "missing",
    }
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
