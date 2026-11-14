use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::backend::{GraphTx, Neo4jBackend, Neo4jTx, StoredNode, StoredRel};
use crate::client::{GraphClient, Transaction};
use crate::dsl::KernelValue;
use crate::error::{GrmError, Result};

use super::{
    DurableOperation, EdgeCreateRequest, EdgeDeleteRequest, EdgeFindRequest, EdgeUpdateRequest,
    NodeCreateRequest, NodeDeleteRequest, NodeFindRequest, NodeUpdateRequest, OrderDirection,
    OrderSpec, PredicateOp, PrimitiveKind, PropertyPredicate, RuntimeField, RuntimeNodeModel,
    RuntimeRelModel, RuntimeValueType, SessionBatchEndpoint, SessionBatchFieldParam,
    SessionBatchOp, SessionBatchParams, SessionBatchResponse, SessionState,
    compare_typed_value_order, compare_typed_values, parse_typed_value, typed_value_kind,
    typed_value_payload, validate_value_for_kind,
};

#[derive(Debug)]
pub struct Neo4jBatchOutcome {
    pub value: Value,
    pub schema_ops: Vec<DurableOperation>,
}

pub async fn neo4j_node_create(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: NodeCreateRequest,
) -> Result<StoredNode> {
    let model = node_model(state, &request.model)?;
    let props = encode_props_for_neo4j(
        &model.fields,
        &normalize_props_for_fields("model", &model.name, &model.fields, &request.props, true)?,
    )?;
    let mut tx = client.transaction().await?;
    let node = tx
        .tx_mut()?
        .create_node(vec![model.label.clone()], props)
        .await?;
    tx.commit().await?;
    decode_node_props(node, &model)
}

pub async fn neo4j_node_update(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: NodeUpdateRequest,
) -> Result<StoredNode> {
    let model = node_model(state, &request.model)?;
    let props = node_update_props(&model, &request.props)?;
    let mut tx = client.transaction().await?;
    let node = update_node(&mut tx, request.id, &model, props).await?;
    tx.commit().await?;
    decode_node_props(node, &model)
}

pub async fn neo4j_node_delete(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: NodeDeleteRequest,
) -> Result<()> {
    let model = node_model(state, &request.model)?;
    let mut tx = client.transaction().await?;
    delete_node(&mut tx, request.id, &model).await?;
    tx.commit().await
}

pub async fn neo4j_node_find(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    mut request: NodeFindRequest,
) -> Result<Vec<StoredNode>> {
    reject_node_find_traversal(&request)?;
    let model = node_model(state, &request.model)?;
    normalize_model_id_alias(
        &mut request.id,
        &mut request.predicates,
        &model.id_field_name,
        "node",
    )?;
    let predicates = typed_predicates(&request.predicates, &model.fields, &model.name)?;
    validate_node_order_fields(&request.order, &model)?;

    let mut clauses = vec![format!("MATCH (n:{})", cypher_name(&model.label))];
    let mut params = serde_json::Map::new();
    let mut filters = Vec::new();
    if let Some(id) = request.id {
        filters.push("id(n) = $grm_id".to_string());
        params.insert("grm_id".to_string(), Value::from(id));
    }
    append_pushdown_predicates(&mut filters, &mut params, "n", &predicates, &model.fields)?;
    if !filters.is_empty() {
        clauses.push(format!("WHERE {}", filters.join(" AND ")));
    }
    clauses.push("RETURN n".to_string());
    let requires_post_processing = predicates
        .iter()
        .any(|(predicate, _)| !predicate_can_push_down(predicate, &model.fields))
        || !node_order_can_push_down(&request.order, &model);
    if !requires_post_processing {
        append_order_page(
            &mut clauses,
            &mut params,
            &request.order,
            request.offset,
            request.limit,
            |spec| cypher_node_order_expression(spec, &model.id_field_name),
        );
    }

    let mut tx = client.transaction().await?;
    let result = tx
        .tx_mut()?
        .execute_query(&clauses.join(" "), Value::Object(params))
        .await?;
    tx.commit().await?;
    let mut nodes = result
        .rows
        .into_iter()
        .filter_map(|row| row.values.into_values().next())
        .map(|value| match value {
            KernelValue::Node(node) => decode_node_props(
                StoredNode {
                    id: node.id,
                    labels: node.labels,
                    props: node.props,
                },
                &model,
            ),
            _ => Err(GrmError::Mapping(
                "Neo4j node find returned a non-node value".into(),
            )),
        })
        .collect::<Result<Vec<_>>>()?;
    if requires_post_processing {
        nodes.retain(|node| predicates_match(&node.props, &predicates));
        sort_nodes(&mut nodes, &model, &request.order);
        Ok(page(nodes, request.offset, request.limit))
    } else {
        Ok(nodes)
    }
}

pub async fn neo4j_edge_create(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: EdgeCreateRequest,
) -> Result<StoredRel> {
    let (model, from_label, to_label, props) =
        validated_edge_create(state, &request.model, &request.props)?;
    let mut tx = client.transaction().await?;
    validate_endpoint(
        &mut tx,
        request.from,
        &from_label,
        "from",
        &model.from_model,
    )
    .await?;
    validate_endpoint(&mut tx, request.to, &to_label, "to", &model.to_model).await?;
    let edge = tx
        .tx_mut()?
        .create_relationship(request.from, request.to, &model.rel_type, props)
        .await?;
    tx.commit().await?;
    decode_rel_props(edge, &model)
}

pub async fn neo4j_edge_update(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: EdgeUpdateRequest,
) -> Result<StoredRel> {
    let model = rel_model(state, &request.model)?;
    let props = edge_update_props(&model, &request.props)?;
    let mut tx = client.transaction().await?;
    let edge = update_edge(&mut tx, request.id, &model, props).await?;
    tx.commit().await?;
    decode_rel_props(edge, &model)
}

pub async fn neo4j_edge_delete(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    request: EdgeDeleteRequest,
) -> Result<()> {
    let model = rel_model(state, &request.model)?;
    let mut tx = client.transaction().await?;
    delete_edge(&mut tx, request.id, &model).await?;
    tx.commit().await
}

pub async fn neo4j_edge_find(
    client: &GraphClient<Neo4jBackend>,
    state: &SessionState,
    mut request: EdgeFindRequest,
) -> Result<Vec<StoredRel>> {
    let model = rel_model(state, &request.model)?;
    normalize_model_id_alias(
        &mut request.id,
        &mut request.predicates,
        &model.id_field_name,
        "edge",
    )?;
    let predicates = typed_predicates(&request.predicates, &model.fields, &model.name)?;
    validate_edge_order_fields(&request.order, &model)?;

    let mut clauses = vec![format!("MATCH ()-[r:{}]->()", cypher_name(&model.rel_type))];
    let mut params = serde_json::Map::new();
    let mut filters = Vec::new();
    if let Some(id) = request.id {
        filters.push("id(r) = $grm_id".to_string());
        params.insert("grm_id".to_string(), Value::from(id));
    }
    if let Some(id) = request.from {
        filters.push("id(startNode(r)) = $from_id".to_string());
        params.insert("from_id".to_string(), Value::from(id));
    }
    if let Some(id) = request.to {
        filters.push("id(endNode(r)) = $to_id".to_string());
        params.insert("to_id".to_string(), Value::from(id));
    }
    append_pushdown_predicates(&mut filters, &mut params, "r", &predicates, &model.fields)?;
    if !filters.is_empty() {
        clauses.push(format!("WHERE {}", filters.join(" AND ")));
    }
    clauses.push("RETURN r".to_string());
    let requires_post_processing = predicates
        .iter()
        .any(|(predicate, _)| !predicate_can_push_down(predicate, &model.fields))
        || !edge_order_can_push_down(&request.order, &model);
    if !requires_post_processing {
        append_order_page(
            &mut clauses,
            &mut params,
            &request.order,
            request.offset,
            request.limit,
            |spec| cypher_edge_order_expression(spec, &model.id_field_name),
        );
    }

    let mut tx = client.transaction().await?;
    let result = tx
        .tx_mut()?
        .execute_query(&clauses.join(" "), Value::Object(params))
        .await?;
    tx.commit().await?;
    let mut rels = result
        .rows
        .into_iter()
        .filter_map(|row| row.values.into_values().next())
        .map(|value| match value {
            KernelValue::Rel(rel) => decode_rel_props(
                StoredRel {
                    id: rel.id,
                    rel_type: rel.ty,
                    from: rel.from,
                    to: rel.to,
                    props: rel.props,
                },
                &model,
            ),
            _ => Err(GrmError::Mapping(
                "Neo4j edge find returned a non-edge value".into(),
            )),
        })
        .collect::<Result<Vec<_>>>()?;
    if requires_post_processing {
        rels.retain(|rel| predicates_match(&rel.props, &predicates));
        sort_rels(&mut rels, &model, &request.order);
        Ok(page(rels, request.offset, request.limit))
    } else {
        Ok(rels)
    }
}

pub async fn apply_neo4j_batch(
    client: &GraphClient<Neo4jBackend>,
    state: &mut SessionState,
    params: SessionBatchParams,
) -> Result<Neo4jBatchOutcome> {
    if !params.atomic {
        return Err(GrmError::NotSupported(
            "Neo4j batch currently requires atomic=true; graph writes are committed only after every supported operation succeeds",
        ));
    }

    let mut staged = state.snapshot();
    let mut refs = BTreeMap::<String, i64>::new();
    let mut schema_ops = Vec::new();
    let mut summary = BatchSummary::new(
        matches!(params.response, SessionBatchResponse::Detailed),
        params.ops.len(),
    );
    let mut tx = client.transaction().await?;

    for (index, op) in params.ops.into_iter().enumerate() {
        if op.is_delete() && !params.allow_deletes {
            let _ = tx.rollback().await;
            summary.record_error(
                index,
                format!("{} requires allow_deletes=true on batch", op.op_name()),
            );
            return Ok(Neo4jBatchOutcome {
                value: summary.into_value(),
                schema_ops: Vec::new(),
            });
        }
        let op_name = op.op_name();
        let result = match op {
            SessionBatchOp::SchemaDefineNode(params) => (|| {
                let model = RuntimeNodeModel::new(
                    params.name.clone(),
                    params.id_field,
                    staged.node_id_type(),
                    parse_batch_fields(params.fields)?,
                )?;
                staged.register_model(model.clone())?;
                schema_ops.push(DurableOperation::RegisterNodeModel { model });
                Ok(BatchApplied::schema(op_name, params.name))
            })(),
            SessionBatchOp::SchemaDefineEdge(params) => (|| {
                let model = RuntimeRelModel::new(
                    params.name.clone(),
                    params.from_model,
                    params.to_model,
                    params.id_field,
                    staged.rel_id_type(),
                    parse_batch_fields(params.fields)?,
                )?;
                staged.register_rel_model(model.clone())?;
                schema_ops.push(DurableOperation::RegisterRelModel { model });
                Ok(BatchApplied::schema(op_name, params.name))
            })(),
            SessionBatchOp::NodeCreate(params) => {
                if params
                    .local_ref
                    .as_ref()
                    .is_some_and(|local_ref| refs.contains_key(local_ref))
                {
                    Err(GrmError::Constraint(format!(
                        "duplicate batch ref '{}'",
                        params.local_ref.as_deref().unwrap_or_default()
                    )))
                } else {
                    create_batch_node(&mut tx, &staged, &mut refs, params, op_name).await
                }
            }
            SessionBatchOp::NodeUpdate(params) => {
                async {
                    let model = node_model(&staged, &params.model)?;
                    let props = node_update_props(&model, &params.props)?;
                    let node = update_node(&mut tx, params.id, &model, props).await?;
                    Ok(BatchApplied::entity(op_name, params.model, node.id, None))
                }
                .await
            }
            SessionBatchOp::NodeDelete(params) => {
                async {
                    let model = node_model(&staged, &params.model)?;
                    delete_node(&mut tx, params.id, &model).await?;
                    Ok(BatchApplied::entity(op_name, params.model, params.id, None))
                }
                .await
            }
            SessionBatchOp::EdgeCreate(params) => {
                create_batch_edge(&mut tx, &staged, &refs, params, op_name).await
            }
            SessionBatchOp::EdgeUpdate(params) => {
                async {
                    let model = rel_model(&staged, &params.model)?;
                    let props = edge_update_props(&model, &params.props)?;
                    let edge = update_edge(&mut tx, params.id, &model, props).await?;
                    Ok(BatchApplied::entity(op_name, params.model, edge.id, None))
                }
                .await
            }
            SessionBatchOp::EdgeDelete(params) => {
                async {
                    let model = rel_model(&staged, &params.model)?;
                    delete_edge(&mut tx, params.id, &model).await?;
                    Ok(BatchApplied::entity(op_name, params.model, params.id, None))
                }
                .await
            }
        };

        match result {
            Ok(applied) => summary.record(applied),
            Err(err) => {
                let _ = tx.rollback().await;
                summary.record_error(index, err.to_string());
                return Ok(Neo4jBatchOutcome {
                    value: summary.into_value(),
                    schema_ops: Vec::new(),
                });
            }
        }
    }

    tx.commit().await?;
    *state = staged;
    Ok(Neo4jBatchOutcome {
        value: summary.into_value(),
        schema_ops,
    })
}

async fn create_batch_node(
    tx: &mut Transaction<Neo4jTx>,
    state: &SessionState,
    refs: &mut BTreeMap<String, i64>,
    params: super::SessionBatchNodeCreateParams,
    op: &'static str,
) -> Result<BatchApplied> {
    let model = node_model(state, &params.model)?;
    let props = encode_props_for_neo4j(
        &model.fields,
        &model.validate_instance_props(&params.props)?,
    )?;
    let node = tx
        .tx_mut()?
        .create_node(vec![model.label.clone()], props)
        .await?;
    if let Some(local_ref) = &params.local_ref {
        refs.insert(local_ref.clone(), node.id);
    }
    Ok(BatchApplied::entity(
        op,
        params.model,
        node.id,
        params.local_ref,
    ))
}

async fn create_batch_edge(
    tx: &mut Transaction<Neo4jTx>,
    state: &SessionState,
    refs: &BTreeMap<String, i64>,
    params: super::SessionBatchEdgeCreateParams,
    op: &'static str,
) -> Result<BatchApplied> {
    let from = resolve_endpoint(&params.from, refs, "from")?;
    let to = resolve_endpoint(&params.to, refs, "to")?;
    let (model, from_label, to_label, props) =
        validated_edge_create(state, &params.model, &params.props)?;
    validate_endpoint(tx, from, &from_label, "from", &model.from_model).await?;
    validate_endpoint(tx, to, &to_label, "to", &model.to_model).await?;
    let edge = tx
        .tx_mut()?
        .create_relationship(from, to, &model.rel_type, props)
        .await?;
    Ok(BatchApplied::entity(op, params.model, edge.id, None))
}

async fn validate_endpoint(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    label: &str,
    endpoint: &str,
    model_name: &str,
) -> Result<()> {
    let node = tx
        .tx_mut()?
        .find_node_by_id(id)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("{endpoint} node '{id}' was not found")))?;
    if node.labels.iter().any(|candidate| candidate == label) {
        Ok(())
    } else {
        Err(GrmError::Constraint(format!(
            "{endpoint} node '{id}' does not match model '{model_name}'"
        )))
    }
}

async fn update_node(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model: &RuntimeNodeModel,
    props: BTreeMap<String, Value>,
) -> Result<StoredNode> {
    validate_endpoint(tx, id, &model.label, "node", &model.name).await?;
    tx.tx_mut()?
        .update_node(id, props)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("node '{id}' was not found")))
}

async fn delete_node(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model: &RuntimeNodeModel,
) -> Result<()> {
    validate_endpoint(tx, id, &model.label, "node", &model.name).await?;
    tx.tx_mut()?.delete_node(id).await
}

async fn update_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model: &RuntimeRelModel,
    props: BTreeMap<String, Value>,
) -> Result<StoredRel> {
    find_edge(tx, id, &model.rel_type).await?.ok_or_else(|| {
        GrmError::Constraint(format!(
            "edge '{id}' was not found for model '{}'",
            model.name
        ))
    })?;
    tx.tx_mut()?
        .update_relationship(id, props)
        .await?
        .ok_or_else(|| GrmError::Constraint(format!("edge '{id}' was not found")))
}

async fn delete_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    model: &RuntimeRelModel,
) -> Result<()> {
    find_edge(tx, id, &model.rel_type).await?.ok_or_else(|| {
        GrmError::Constraint(format!(
            "edge '{id}' was not found for model '{}'",
            model.name
        ))
    })?;
    tx.tx_mut()?.delete_relationship(id).await
}

async fn find_edge(
    tx: &mut Transaction<Neo4jTx>,
    id: i64,
    rel_type: &str,
) -> Result<Option<StoredRel>> {
    let result = tx
        .tx_mut()?
        .execute_query(
            &format!(
                "MATCH ()-[r:{}]->() WHERE id(r) = $grm_id RETURN r",
                cypher_name(rel_type)
            ),
            json!({ "grm_id": id }),
        )
        .await?;
    result
        .rows
        .into_iter()
        .next()
        .and_then(|row| row.values.into_values().next())
        .map(|value| match value {
            KernelValue::Rel(rel) => Ok(StoredRel {
                id: rel.id,
                rel_type: rel.ty,
                from: rel.from,
                to: rel.to,
                props: rel.props,
            }),
            _ => Err(GrmError::Mapping(
                "Neo4j edge lookup returned a non-edge value".into(),
            )),
        })
        .transpose()
}

fn node_model(state: &SessionState, name: &str) -> Result<RuntimeNodeModel> {
    state
        .catalog()
        .get_node_model(name)
        .cloned()
        .ok_or_else(|| missing_schema("node", name))
}

fn rel_model(state: &SessionState, name: &str) -> Result<RuntimeRelModel> {
    state
        .catalog()
        .get_rel_model(name)
        .cloned()
        .ok_or_else(|| missing_schema("edge", name))
}

fn missing_schema(kind: &str, model: &str) -> GrmError {
    GrmError::Constraint(format!(
        "{kind} model '{model}' is not registered in the GRM-owned session schema; list or define schema before accessing typed Neo4j data"
    ))
}

fn validated_edge_create(
    state: &SessionState,
    model_name: &str,
    values: &BTreeMap<String, Value>,
) -> Result<(RuntimeRelModel, String, String, BTreeMap<String, Value>)> {
    let model = rel_model(state, model_name)?;
    let from_label = node_model(state, &model.from_model)?.label;
    let to_label = node_model(state, &model.to_model)?.label;
    let props = encode_props_for_neo4j(
        &model.fields,
        &normalize_props_for_fields(
            "relationship model",
            &model.name,
            &model.fields,
            values,
            true,
        )?,
    )?;
    Ok((model, from_label, to_label, props))
}

fn node_update_props(
    model: &RuntimeNodeModel,
    values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    model_update_props(&model.fields, &model.name, values, &[&model.id_field_name])
}

fn edge_update_props(
    model: &RuntimeRelModel,
    values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    model_update_props(
        &model.fields,
        &model.name,
        values,
        &[&model.id_field_name, "from", "to"],
    )
}

fn model_update_props(
    fields: &[RuntimeField],
    model_name: &str,
    values: &BTreeMap<String, Value>,
    special_keys: &[&str],
) -> Result<BTreeMap<String, Value>> {
    let mut props = BTreeMap::new();
    for (key, value) in values
        .iter()
        .filter(|(key, _)| key.as_str() != "id" && !special_keys.contains(&key.as_str()))
    {
        let field = fields
            .iter()
            .find(|field| field.name == *key)
            .ok_or_else(|| {
                GrmError::Constraint(format!("unknown field '{key}' for model '{model_name}'"))
            })?;
        let value = normalize_value_for_field("model", model_name, field, value)?;
        props.insert(key.clone(), encode_value_for_neo4j(field, &value)?);
    }
    Ok(props)
}

fn typed_predicates(
    predicates: &[PropertyPredicate],
    fields: &[RuntimeField],
    model_name: &str,
) -> Result<Vec<(PropertyPredicate, Value)>> {
    predicates
        .iter()
        .map(|predicate| {
            let field = fields
                .iter()
                .find(|field| field.name == predicate.field)
                .ok_or_else(|| {
                    GrmError::Constraint(format!(
                        "unknown field '{}' for model '{model_name}'",
                        predicate.field
                    ))
                })?;
            if predicate.op == PredicateOp::Contains
                && !matches!(field.value_type, RuntimeValueType::String)
            {
                return Err(GrmError::Constraint(format!(
                    "contains filter '{}' requires a string field",
                    predicate.field
                )));
            }
            let value = normalize_value_for_field("model", model_name, field, &predicate.value)?;
            Ok((predicate.clone(), value))
        })
        .collect()
}

fn normalize_props_for_fields(
    kind: &str,
    model_name: &str,
    fields: &[RuntimeField],
    values: &BTreeMap<String, Value>,
    require_missing: bool,
) -> Result<BTreeMap<String, Value>> {
    for key in values.keys() {
        if !fields.iter().any(|field| field.name == *key) {
            return Err(GrmError::Constraint(format!(
                "unknown field '{key}' for {kind} '{model_name}'"
            )));
        }
    }

    let mut props = BTreeMap::new();
    for field in fields {
        match values.get(&field.name) {
            Some(value) => {
                let value = normalize_value_for_field(kind, model_name, field, value)?;
                props.insert(field.name.clone(), value);
            }
            None if require_missing && field.required => {
                return Err(GrmError::Constraint(format!(
                    "missing required field '{}'",
                    field.name
                )));
            }
            None => {}
        }
    }
    Ok(props)
}

fn normalize_value_for_field(
    kind: &str,
    model_name: &str,
    field: &RuntimeField,
    value: &Value,
) -> Result<Value> {
    if validate_value_for_kind(field.value_type.into(), value) {
        return Ok(value.clone());
    }
    if let Some(actual) = typed_value_kind(value) {
        return Err(GrmError::Constraint(format!(
            "field '{}' for {} '{}' must be {}, got {}",
            field.name,
            kind,
            model_name,
            field.value_type.keyword(),
            actual
        )));
    }
    Err(GrmError::Constraint(format!(
        "field '{}' for {} '{}' must be {}",
        field.name,
        kind,
        model_name,
        field.value_type.keyword()
    )))
}

fn predicates_match(
    props: &BTreeMap<String, Value>,
    predicates: &[(PropertyPredicate, Value)],
) -> bool {
    predicates.iter().all(|(predicate, expected)| {
        props
            .get(&predicate.field)
            .is_some_and(|actual| compare_typed_values(actual, predicate.op.into(), expected))
    })
}

fn encode_props_for_neo4j(
    fields: &[RuntimeField],
    values: &BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>> {
    values
        .iter()
        .map(|(key, value)| {
            let field = fields
                .iter()
                .find(|field| field.name == *key)
                .ok_or_else(|| GrmError::Constraint(format!("unknown field '{key}'")))?;
            Ok((key.clone(), encode_value_for_neo4j(field, value)?))
        })
        .collect()
}

fn sort_nodes(nodes: &mut [StoredNode], model: &RuntimeNodeModel, order: &[OrderSpec]) {
    nodes.sort_by(|left, right| compare_node_order_values(left, right, model, order));
}

fn compare_node_order_values(
    left: &StoredNode,
    right: &StoredNode,
    model: &RuntimeNodeModel,
    order: &[OrderSpec],
) -> Ordering {
    for spec in order {
        let ordering = if spec.field == "id" || spec.field == model.id_field_name {
            left.id.cmp(&right.id)
        } else {
            compare_optional_values(left.props.get(&spec.field), right.props.get(&spec.field))
        };
        let ordering = apply_order_direction(ordering, spec.direction);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn sort_rels(rels: &mut [StoredRel], model: &RuntimeRelModel, order: &[OrderSpec]) {
    rels.sort_by(|left, right| compare_rel_order_values(left, right, model, order));
}

fn compare_rel_order_values(
    left: &StoredRel,
    right: &StoredRel,
    model: &RuntimeRelModel,
    order: &[OrderSpec],
) -> Ordering {
    for spec in order {
        let ordering = match spec.field.as_str() {
            "id" => left.id.cmp(&right.id),
            "from" => left.from.cmp(&right.from),
            "to" => left.to.cmp(&right.to),
            field if field == model.id_field_name => left.id.cmp(&right.id),
            _ => compare_optional_values(left.props.get(&spec.field), right.props.get(&spec.field)),
        };
        let ordering = apply_order_direction(ordering, spec.direction);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn compare_optional_values(left: Option<&Value>, right: Option<&Value>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            compare_typed_value_order(left, right).unwrap_or(Ordering::Equal)
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn apply_order_direction(ordering: Ordering, direction: OrderDirection) -> Ordering {
    match direction {
        OrderDirection::Asc => ordering,
        OrderDirection::Desc => ordering.reverse(),
    }
}

fn page<T>(values: Vec<T>, offset: Option<usize>, limit: Option<usize>) -> Vec<T> {
    values
        .into_iter()
        .skip(offset.unwrap_or(0))
        .take(limit.unwrap_or(usize::MAX))
        .collect()
}

fn encode_value_for_neo4j(field: &RuntimeField, value: &Value) -> Result<Value> {
    match field.value_type {
        RuntimeValueType::String
        | RuntimeValueType::Int
        | RuntimeValueType::Float
        | RuntimeValueType::Bool => Ok(value.clone()),
        RuntimeValueType::Bytes
        | RuntimeValueType::Decimal
        | RuntimeValueType::Date
        | RuntimeValueType::DateTime
        | RuntimeValueType::Duration
        | RuntimeValueType::Uuid => {
            let expected = field.value_type.keyword();
            let actual = typed_value_kind(value).ok_or_else(|| {
                GrmError::Constraint(format!(
                    "field '{}' must be canonical tagged {}",
                    field.name, expected
                ))
            })?;
            if actual != expected {
                return Err(GrmError::Constraint(format!(
                    "field '{}' must be {}, got {}",
                    field.name, expected, actual
                )));
            }
            let payload = typed_value_payload(value).ok_or_else(|| {
                GrmError::Constraint(format!(
                    "field '{}' must be canonical tagged {}",
                    field.name, expected
                ))
            })?;
            Ok(Value::String(payload.to_string()))
        }
    }
}

fn decode_node_props(mut node: StoredNode, model: &RuntimeNodeModel) -> Result<StoredNode> {
    decode_props(&mut node.props, &model.fields)?;
    Ok(node)
}

fn decode_rel_props(mut rel: StoredRel, model: &RuntimeRelModel) -> Result<StoredRel> {
    decode_props(&mut rel.props, &model.fields)?;
    Ok(rel)
}

fn decode_props(props: &mut BTreeMap<String, Value>, fields: &[RuntimeField]) -> Result<()> {
    for field in fields {
        let Some(value) = props.get(&field.name).cloned() else {
            continue;
        };
        let decoded = match field.value_type {
            RuntimeValueType::String
            | RuntimeValueType::Int
            | RuntimeValueType::Float
            | RuntimeValueType::Bool => {
                if validate_value_for_kind(field.value_type.into(), &value) {
                    value
                } else {
                    return Err(GrmError::Mapping(format!(
                        "Neo4j property '{}' is not a valid {} value",
                        field.name,
                        field.value_type.keyword()
                    )));
                }
            }
            RuntimeValueType::Bytes
            | RuntimeValueType::Decimal
            | RuntimeValueType::Date
            | RuntimeValueType::DateTime
            | RuntimeValueType::Duration
            | RuntimeValueType::Uuid => {
                let raw = value.as_str().ok_or_else(|| {
                    GrmError::Mapping(format!(
                        "Neo4j property '{}' is not a canonical {} payload",
                        field.name,
                        field.value_type.keyword()
                    ))
                })?;
                parse_typed_value(PrimitiveKind::from(field.value_type), raw)?
            }
        };
        props.insert(field.name.clone(), decoded);
    }
    Ok(())
}

#[cfg(test)]
mod primitive_support_tests {
    use super::*;

    #[test]
    fn extended_soml_primitives_encode_to_scalars_and_decode_to_tagged_values() {
        let fields = vec![
            RuntimeField {
                name: "event_id".into(),
                value_type: RuntimeValueType::Uuid,
                required: true,
            },
            RuntimeField {
                name: "amount".into(),
                value_type: RuntimeValueType::Decimal,
                required: true,
            },
        ];
        let event_id =
            parse_typed_value(PrimitiveKind::Uuid, "123e4567-e89b-12d3-a456-426614174000").unwrap();
        let amount = parse_typed_value(PrimitiveKind::Decimal, "12.5").unwrap();

        let encoded = encode_props_for_neo4j(
            &fields,
            &BTreeMap::from([
                ("event_id".into(), event_id.clone()),
                ("amount".into(), amount.clone()),
            ]),
        )
        .unwrap();
        assert_eq!(
            encoded["event_id"],
            json!("123e4567-e89b-12d3-a456-426614174000")
        );
        assert_eq!(encoded["amount"], json!("12.5"));

        let mut decoded = encoded;
        decode_props(&mut decoded, &fields).unwrap();
        assert_eq!(decoded["event_id"], event_id);
        assert_eq!(decoded["amount"], amount);
    }

    #[test]
    fn extended_primitive_decode_rejects_mismatched_backend_payloads() {
        let fields = vec![RuntimeField {
            name: "event_id".into(),
            value_type: RuntimeValueType::Uuid,
            required: true,
        }];
        let mut props = BTreeMap::from([("event_id".into(), json!("not-a-uuid"))]);
        let err = decode_props(&mut props, &fields).unwrap_err();
        assert!(err.to_string().contains("expected canonical uuid value"));
    }

    #[test]
    fn extended_primitive_input_rejects_mismatched_tag_before_encoding() {
        let fields = vec![RuntimeField {
            name: "amount".into(),
            value_type: RuntimeValueType::Decimal,
            required: true,
        }];
        let err = normalize_props_for_fields(
            "model",
            "Evidence",
            &fields,
            &BTreeMap::from([(
                "amount".into(),
                parse_typed_value(PrimitiveKind::Uuid, "123e4567-e89b-12d3-a456-426614174000")
                    .unwrap(),
            )]),
            true,
        )
        .unwrap_err();
        assert!(err.to_string().contains("must be decimal, got uuid"));
    }
}

fn normalize_model_id_alias(
    request_id: &mut Option<i64>,
    predicates: &mut Vec<PropertyPredicate>,
    id_field_name: &str,
    subject: &str,
) -> Result<()> {
    if id_field_name == "id" {
        return Ok(());
    }

    let mut alias_id = None;
    let mut retained = Vec::with_capacity(predicates.len());
    for predicate in predicates.drain(..) {
        if predicate.field != id_field_name {
            retained.push(predicate);
            continue;
        }
        if predicate.op != PredicateOp::Eq {
            return Err(GrmError::Constraint(format!(
                "{subject} id filter '{id_field_name}' only supports equality"
            )));
        }
        let parsed = parse_backend_id_value(&predicate.value, id_field_name)?;
        if alias_id.replace(parsed).is_some() {
            return Err(GrmError::Constraint(format!(
                "duplicate {subject} id filter '{id_field_name}'"
            )));
        }
    }
    *predicates = retained;

    match (*request_id, alias_id) {
        (Some(left), Some(right)) if left != right => Err(GrmError::Constraint(format!(
            "conflicting {subject} id filters 'id' and '{id_field_name}'"
        ))),
        (None, Some(id)) => {
            *request_id = Some(id);
            Ok(())
        }
        _ => Ok(()),
    }
}

fn parse_backend_id_value(value: &Value, field: &str) -> Result<i64> {
    match value {
        Value::Number(value) => value.as_i64().ok_or_else(|| {
            GrmError::Constraint(format!("{field} filter must be an integer backend id"))
        }),
        Value::String(value) => value.parse::<i64>().map_err(|_| {
            GrmError::Constraint(format!("{field} filter must be an integer backend id"))
        }),
        _ => Err(GrmError::Constraint(format!(
            "{field} filter must be an integer backend id"
        ))),
    }
}

fn reject_node_find_traversal(request: &NodeFindRequest) -> Result<()> {
    if request.traversals.is_empty()
        && request.end_predicates.is_empty()
        && request.edge_predicates.is_empty()
        && request.return_mode.is_none()
    {
        Ok(())
    } else {
        Err(GrmError::NotSupported(
            "Neo4j graph sessions support simple node_find only; traversal is not portable",
        ))
    }
}

fn validate_node_order_fields(order: &[OrderSpec], model: &RuntimeNodeModel) -> Result<()> {
    for spec in order {
        if spec.field != "id"
            && spec.field != model.id_field_name
            && model.field(&spec.field).is_none()
        {
            return Err(GrmError::Constraint(format!(
                "unknown order field '{}' for model '{}'",
                spec.field, model.name
            )));
        }
    }
    Ok(())
}

fn validate_edge_order_fields(order: &[OrderSpec], model: &RuntimeRelModel) -> Result<()> {
    for spec in order {
        if spec.field != "id"
            && spec.field != model.id_field_name
            && spec.field != "from"
            && spec.field != "to"
            && model.field(&spec.field).is_none()
        {
            return Err(GrmError::Constraint(format!(
                "unknown order field '{}' for link '{}'",
                spec.field, model.name
            )));
        }
    }
    Ok(())
}

fn append_pushdown_predicates(
    filters: &mut Vec<String>,
    params: &mut serde_json::Map<String, Value>,
    variable: &str,
    predicates: &[(PropertyPredicate, Value)],
    fields: &[RuntimeField],
) -> Result<()> {
    for (index, (predicate, value)) in predicates.iter().enumerate() {
        if !predicate_can_push_down(predicate, fields) {
            continue;
        }
        let field = fields
            .iter()
            .find(|field| field.name == predicate.field)
            .expect("typed predicates reference declared fields");
        filters.push(format!(
            "{variable}.{} {} $p{index}",
            cypher_name(&predicate.field),
            cypher_predicate_op(predicate.op)
        ));
        params.insert(format!("p{index}"), encode_value_for_neo4j(field, value)?);
    }
    Ok(())
}

fn predicate_can_push_down(predicate: &PropertyPredicate, fields: &[RuntimeField]) -> bool {
    fields
        .iter()
        .find(|field| field.name == predicate.field)
        .is_some_and(|field| {
            is_native_neo4j_scalar(field.value_type)
                || matches!(predicate.op, PredicateOp::Eq | PredicateOp::Ne)
        })
}

fn node_order_can_push_down(order: &[OrderSpec], model: &RuntimeNodeModel) -> bool {
    order.iter().all(|spec| {
        spec.field == "id"
            || spec.field == model.id_field_name
            || model
                .field(&spec.field)
                .is_some_and(|field| is_native_neo4j_scalar(field.value_type))
    })
}

fn edge_order_can_push_down(order: &[OrderSpec], model: &RuntimeRelModel) -> bool {
    order.iter().all(|spec| {
        matches!(spec.field.as_str(), "id" | "from" | "to")
            || spec.field == model.id_field_name
            || model
                .field(&spec.field)
                .is_some_and(|field| is_native_neo4j_scalar(field.value_type))
    })
}

fn is_native_neo4j_scalar(value_type: RuntimeValueType) -> bool {
    matches!(
        value_type,
        RuntimeValueType::String
            | RuntimeValueType::Int
            | RuntimeValueType::Float
            | RuntimeValueType::Bool
    )
}

fn append_order_page(
    clauses: &mut Vec<String>,
    params: &mut serde_json::Map<String, Value>,
    order: &[OrderSpec],
    offset: Option<usize>,
    limit: Option<usize>,
    expression: impl Fn(&OrderSpec) -> String,
) {
    if !order.is_empty() {
        clauses.push(format!(
            "ORDER BY {}",
            order
                .iter()
                .map(|spec| format!(
                    "{} {}",
                    expression(spec),
                    match spec.direction {
                        OrderDirection::Asc => "ASC",
                        OrderDirection::Desc => "DESC",
                    }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(offset) = offset {
        clauses.push("SKIP $grm_offset".to_string());
        params.insert("grm_offset".to_string(), Value::from(offset as i64));
    }
    if let Some(limit) = limit {
        clauses.push("LIMIT $grm_limit".to_string());
        params.insert("grm_limit".to_string(), Value::from(limit as i64));
    }
}

fn cypher_node_order_expression(spec: &OrderSpec, id_field: &str) -> String {
    if spec.field == "id" || spec.field == id_field {
        "id(n)".to_string()
    } else {
        format!("n.{}", cypher_name(&spec.field))
    }
}

fn cypher_edge_order_expression(spec: &OrderSpec, id_field: &str) -> String {
    match spec.field.as_str() {
        "id" => "id(r)".to_string(),
        "from" => "id(startNode(r))".to_string(),
        "to" => "id(endNode(r))".to_string(),
        field if field == id_field => "id(r)".to_string(),
        _ => format!("r.{}", cypher_name(&spec.field)),
    }
}

fn cypher_predicate_op(op: PredicateOp) -> &'static str {
    match op {
        PredicateOp::Eq => "=",
        PredicateOp::Ne => "<>",
        PredicateOp::Gt => ">",
        PredicateOp::Ge => ">=",
        PredicateOp::Lt => "<",
        PredicateOp::Le => "<=",
        PredicateOp::Contains => "CONTAINS",
    }
}

fn parse_batch_fields(fields: Vec<SessionBatchFieldParam>) -> Result<Vec<RuntimeField>> {
    fields
        .into_iter()
        .map(|field| {
            let value_type =
                RuntimeValueType::parse_keyword(&field.value_type).ok_or_else(|| {
                    GrmError::Constraint(format!(
                        "unsupported field type '{}', expected one of: string, int, float, bool, bytes, decimal, date, datetime, duration, or uuid",
                        field.value_type
                    ))
                })?;
            Ok(RuntimeField {
                name: field.name,
                value_type,
                required: field.required,
            })
        })
        .collect()
}

fn resolve_endpoint(
    endpoint: &SessionBatchEndpoint,
    refs: &BTreeMap<String, i64>,
    field: &str,
) -> Result<i64> {
    match endpoint {
        SessionBatchEndpoint::Id(id) => Ok(*id),
        SessionBatchEndpoint::Ref(local_ref) => refs.get(local_ref).copied().ok_or_else(|| {
            GrmError::Constraint(format!(
                "{field} ref '{local_ref}' was not created earlier in this batch"
            ))
        }),
    }
}

fn cypher_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

struct BatchApplied {
    op: &'static str,
    model: String,
    id: Option<i64>,
    local_ref: Option<String>,
}

impl BatchApplied {
    fn schema(op: &'static str, model: String) -> Self {
        Self {
            op,
            model,
            id: None,
            local_ref: None,
        }
    }

    fn entity(op: &'static str, model: String, id: i64, local_ref: Option<String>) -> Self {
        Self {
            op,
            model,
            id: Some(id),
            local_ref,
        }
    }
}

struct BatchSummary {
    applied: bool,
    detailed: bool,
    operation_count: usize,
    counts: BTreeMap<String, BTreeMap<String, usize>>,
    errors: Vec<Value>,
    ids: Vec<Value>,
}

impl BatchSummary {
    fn new(detailed: bool, operation_count: usize) -> Self {
        Self {
            applied: true,
            detailed,
            operation_count,
            counts: BTreeMap::new(),
            errors: Vec::new(),
            ids: Vec::new(),
        }
    }

    fn record(&mut self, applied: BatchApplied) {
        *self
            .counts
            .entry(applied.op.to_string())
            .or_default()
            .entry(applied.model.clone())
            .or_default() += 1;
        if self.detailed {
            if let Some(id) = applied.id {
                let mut value = json!({
                    "op": applied.op,
                    "model": applied.model,
                    "id": id,
                });
                if let Some(local_ref) = applied.local_ref {
                    value["ref"] = json!(local_ref);
                }
                self.ids.push(value);
            }
        }
    }

    fn record_error(&mut self, index: usize, message: String) {
        self.applied = false;
        self.errors.push(json!({
            "index": index,
            "message": message,
            "recovery": "Inspect the failed operation and current GRM-owned session schema, then retry the batch."
        }));
    }

    fn into_value(self) -> Value {
        let mut value = json!({
            "applied": self.applied,
            "atomic": true,
            "operation_count": self.operation_count,
            "counts": self.counts,
            "errors": self.errors,
        });
        if self.detailed {
            value["ids"] = json!(self.ids);
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::BackendIdType;

    fn user_model() -> RuntimeNodeModel {
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
        .unwrap()
    }

    #[test]
    fn update_validation_ignores_identity_and_requires_exact_field_types() {
        let props = node_update_props(
            &user_model(),
            &BTreeMap::from([("userId".into(), "42".into()), ("age".into(), json!(43))]),
        )
        .unwrap();
        assert_eq!(props, BTreeMap::from([("age".into(), json!(43))]));

        let err = node_update_props(
            &user_model(),
            &BTreeMap::from([("age".into(), json!("43"))]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("must be int"));
    }

    #[test]
    fn neo4j_pushdown_preserves_native_scalars_and_defers_logical_primitives() {
        let fields = vec![
            RuntimeField {
                name: "name".into(),
                value_type: RuntimeValueType::String,
                required: false,
            },
            RuntimeField {
                name: "amount".into(),
                value_type: RuntimeValueType::Decimal,
                required: false,
            },
        ];
        let scalar = PropertyPredicate {
            field: "name".into(),
            op: PredicateOp::Contains,
            value: json!("Ada"),
        };
        let decimal_eq = PropertyPredicate {
            field: "amount".into(),
            op: PredicateOp::Eq,
            value: parse_typed_value(PrimitiveKind::Decimal, "12.5").unwrap(),
        };
        let decimal_gt = PropertyPredicate {
            field: "amount".into(),
            op: PredicateOp::Gt,
            value: parse_typed_value(PrimitiveKind::Decimal, "10").unwrap(),
        };

        assert!(predicate_can_push_down(&scalar, &fields));
        assert!(predicate_can_push_down(&decimal_eq, &fields));
        assert!(!predicate_can_push_down(&decimal_gt, &fields));

        let model =
            RuntimeNodeModel::new("Reading", "readingId", BackendIdType::Int64, fields).unwrap();
        assert!(node_order_can_push_down(
            &[OrderSpec {
                field: "name".into(),
                direction: OrderDirection::Asc,
            }],
            &model
        ));
        assert!(!node_order_can_push_down(
            &[OrderSpec {
                field: "amount".into(),
                direction: OrderDirection::Asc,
            }],
            &model
        ));
    }

    #[test]
    fn simple_find_rejects_workspace_traversal_shape() {
        let mut request =
            NodeFindRequest::from_adapter_filter_values("User", BTreeMap::new()).unwrap();
        request.return_mode = Some(super::super::TraversalReturn::End);
        let err = reject_node_find_traversal(&request).unwrap_err();
        assert!(err.to_string().contains("traversal is not portable"));
    }

    #[test]
    fn model_id_alias_becomes_backend_id_filter() {
        let mut request = NodeFindRequest::from_adapter_filter_values(
            "User",
            BTreeMap::from([("userId".into(), json!(42)), ("name".into(), json!("Ada"))]),
        )
        .unwrap();
        normalize_model_id_alias(&mut request.id, &mut request.predicates, "userId", "node")
            .unwrap();

        assert_eq!(request.id, Some(42));
        assert_eq!(request.predicates.len(), 1);
        assert_eq!(request.predicates[0].field, "name");
    }

    #[test]
    fn model_id_alias_rejects_conflicting_physical_id_filter() {
        let mut request = EdgeFindRequest::from_adapter_filter_values(
            "Authored",
            BTreeMap::from([("id".into(), json!(7)), ("authoredId".into(), json!(8))]),
        )
        .unwrap();
        let err = normalize_model_id_alias(
            &mut request.id,
            &mut request.predicates,
            "authoredId",
            "edge",
        )
        .unwrap_err();

        assert!(err.to_string().contains("conflicting edge id filters"));
    }

    #[test]
    fn cypher_name_escapes_malicious_identifier_fragments() {
        let escaped = cypher_name("User`) MATCH (m) DETACH DELETE m //");
        assert_eq!(escaped, "`User``) MATCH (m) DETACH DELETE m //`");
    }

    #[test]
    fn neo4j_predicates_and_order_reject_unknown_raw_fields() {
        let model = user_model();
        let malicious_field = "name`) DETACH DELETE n //".to_string();
        let predicates = vec![PropertyPredicate {
            field: malicious_field.clone(),
            op: PredicateOp::Eq,
            value: json!("Ada"),
        }];

        let err = typed_predicates(&predicates, &model.fields, &model.name).unwrap_err();
        assert!(err.to_string().contains("unknown field"));

        let order = vec![OrderSpec {
            field: malicious_field,
            direction: OrderDirection::Asc,
        }];
        let err = validate_node_order_fields(&order, &model).unwrap_err();
        assert!(err.to_string().contains("unknown order field"));
    }
}
