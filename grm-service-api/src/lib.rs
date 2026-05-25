//! Split-ready service API contract artifacts for GRM.
//!
//! This crate intentionally contains the protobuf source contract rather than a
//! daemon, transport policy, or generated client. It is client-facing and can be
//! split from the monorepo later without depending on private daemon internals.

use std::path::{Path, PathBuf};

use serde_json::Value;

#[allow(warnings)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/grm.service.v1.rs"));
}

pub const PROTO_PACKAGE: &str = "grm.service.v1";
pub const PROTO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/proto");

pub const PROTO_FILES: &[&str] = &[
    "grm/service/v1/common.proto",
    "grm/service/v1/schema.proto",
    "grm/service/v1/node.proto",
    "grm/service/v1/edge.proto",
    "grm/service/v1/query.proto",
    "grm/service/v1/batch.proto",
    "grm/service/v1/admin.proto",
    "grm/service/v1/service.proto",
];

pub fn proto_root() -> &'static Path {
    Path::new(PROTO_ROOT)
}

pub fn proto_files() -> impl Iterator<Item = PathBuf> {
    PROTO_FILES.iter().map(|file| proto_root().join(file))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServiceRequest {
    DefineNode(DefineNodeRequest),
    DefineEdge(DefineEdgeRequest),
    SchemaList(SchemaListRequest),
    CreateNode(NodeCreateRequest),
    UpdateNode(NodeUpdateRequest),
    DeleteNode(NodeDeleteRequest),
    FindNodes(NodeFindRequest),
    CreateEdge(EdgeCreateRequest),
    UpdateEdge(EdgeUpdateRequest),
    DeleteEdge(EdgeDeleteRequest),
    FindEdges(EdgeFindRequest),
    Query(QueryRequest),
    Explain(ExplainRequest),
    Profile(ProfileRequest),
    ApplyBatch(BatchRequest),
    Save(SaveRequest),
    Load(LoadRequest),
    Export(ExportRequest),
    Import(ImportRequest),
    IndexList(IndexListRequest),
    Summary(SummaryRequest),
}

impl ServiceRequest {
    pub fn into_runtime_request(self) -> grm_rs::Result<grm_rs::RuntimeRequest> {
        self.try_into()
    }

    pub async fn execute(
        self,
        state: &mut grm_rs::SessionState,
    ) -> grm_rs::Result<grm_rs::RuntimeDispatchOutcome> {
        state.execute_runtime(self.into_runtime_request()?).await
    }
}

impl TryFrom<ServiceRequest> for grm_rs::RuntimeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: ServiceRequest) -> grm_rs::Result<Self> {
        Ok(match request {
            ServiceRequest::DefineNode(request) => {
                Self::Schema(grm_rs::SchemaRequest::DefineNode(request.try_into()?))
            }
            ServiceRequest::DefineEdge(request) => {
                Self::Schema(grm_rs::SchemaRequest::DefineEdge(request.try_into()?))
            }
            ServiceRequest::SchemaList(_) => Self::Admin(grm_rs::AdminRequest::SchemaList),
            ServiceRequest::CreateNode(request) => {
                Self::Node(grm_rs::NodeRequest::Create(request.try_into()?))
            }
            ServiceRequest::UpdateNode(request) => {
                Self::Node(grm_rs::NodeRequest::Update(request.try_into()?))
            }
            ServiceRequest::DeleteNode(request) => {
                Self::Node(grm_rs::NodeRequest::Delete(request.into()))
            }
            ServiceRequest::FindNodes(request) => {
                Self::Node(grm_rs::NodeRequest::Find(request.try_into()?))
            }
            ServiceRequest::CreateEdge(request) => {
                Self::Edge(grm_rs::EdgeRequest::Create(request.try_into()?))
            }
            ServiceRequest::UpdateEdge(request) => {
                Self::Edge(grm_rs::EdgeRequest::Update(request.try_into()?))
            }
            ServiceRequest::DeleteEdge(request) => {
                Self::Edge(grm_rs::EdgeRequest::Delete(request.into()))
            }
            ServiceRequest::FindEdges(request) => {
                Self::Edge(grm_rs::EdgeRequest::Find(request.try_into()?))
            }
            ServiceRequest::Query(request) => Self::Query(request.try_into()?),
            ServiceRequest::Explain(request) => Self::Explain(request.try_into()?),
            ServiceRequest::Profile(request) => Self::Profile(request.try_into()?),
            ServiceRequest::ApplyBatch(request) => Self::Batch(request.try_into()?),
            ServiceRequest::IndexList(_) => Self::Admin(grm_rs::AdminRequest::IndexList),
            ServiceRequest::Summary(_) => Self::Admin(grm_rs::AdminRequest::Summary),
            ServiceRequest::Save(_)
            | ServiceRequest::Load(_)
            | ServiceRequest::Export(_)
            | ServiceRequest::Import(_) => {
                return Err(grm_rs::GrmError::NotSupported(
                    "service snapshot handle and document admin requests are not mapped to local runtime file operations",
                ));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefineNodeRequest {
    pub name: String,
    pub id_field: String,
    pub fields: Vec<FieldSpec>,
}

impl TryFrom<DefineNodeRequest> for grm_rs::DefineNodeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: DefineNodeRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            name: request.name,
            id_field: request.id_field,
            fields: convert_fields(request.fields)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DefineEdgeRequest {
    pub name: String,
    pub from_model: String,
    pub to_model: String,
    pub id_field: String,
    pub fields: Vec<FieldSpec>,
}

impl TryFrom<DefineEdgeRequest> for grm_rs::DefineEdgeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: DefineEdgeRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            name: request.name,
            from_model: request.from_model,
            to_model: request.to_model,
            id_field: request.id_field,
            fields: convert_fields(request.fields)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaListRequest {}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub value_type: FieldValueType,
    pub required: bool,
}

impl TryFrom<FieldSpec> for grm_rs::FieldSpec {
    type Error = grm_rs::GrmError;

    fn try_from(field: FieldSpec) -> grm_rs::Result<Self> {
        Ok(Self {
            name: field.name,
            value_type: field.value_type.try_into()?,
            required: field.required,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldValueType {
    Unspecified,
    String,
    Int,
    Float,
    Bool,
}

impl TryFrom<FieldValueType> for grm_rs::FieldValueType {
    type Error = grm_rs::GrmError;

    fn try_from(value_type: FieldValueType) -> grm_rs::Result<Self> {
        match value_type {
            FieldValueType::Unspecified => Err(grm_rs::GrmError::Constraint(
                "field value type must be specified".into(),
            )),
            FieldValueType::String => Ok(Self::String),
            FieldValueType::Int => Ok(Self::Int),
            FieldValueType::Float => Ok(Self::Float),
            FieldValueType::Bool => Ok(Self::Bool),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeCreateRequest {
    pub model: String,
    pub props: PropertyMap,
}

impl TryFrom<NodeCreateRequest> for grm_rs::NodeCreateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: NodeCreateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            props: request.props.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeUpdateRequest {
    pub model: String,
    pub id: i64,
    pub props: PropertyMap,
}

impl TryFrom<NodeUpdateRequest> for grm_rs::NodeUpdateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: NodeUpdateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            id: request.id,
            props: request.props.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeDeleteRequest {
    pub model: String,
    pub id: i64,
}

impl From<NodeDeleteRequest> for grm_rs::NodeDeleteRequest {
    fn from(request: NodeDeleteRequest) -> Self {
        Self {
            model: request.model,
            id: request.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeFindRequest {
    pub model: String,
    pub predicates: Vec<PropertyPredicate>,
    pub end_predicates: Vec<PropertyPredicate>,
    pub edge_predicates: Vec<PropertyPredicate>,
    pub traversals: Vec<TraversalStep>,
    pub order: Vec<OrderSpec>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub id: Option<i64>,
    pub return_mode: Option<TraversalReturn>,
}

impl TryFrom<NodeFindRequest> for grm_rs::NodeFindRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: NodeFindRequest) -> grm_rs::Result<Self> {
        node_find_shape_to_runtime(NodeFindShape {
            model: request.model,
            predicates: request.predicates,
            end_predicates: request.end_predicates,
            edge_predicates: request.edge_predicates,
            traversals: request.traversals,
            order: request.order,
            limit: request.limit,
            offset: request.offset,
            id: request.id,
            return_mode: request.return_mode,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeCreateRequest {
    pub model: String,
    pub from: i64,
    pub to: i64,
    pub props: PropertyMap,
}

impl TryFrom<EdgeCreateRequest> for grm_rs::EdgeCreateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: EdgeCreateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            from: request.from,
            to: request.to,
            props: request.props.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeUpdateRequest {
    pub model: String,
    pub id: i64,
    pub props: PropertyMap,
}

impl TryFrom<EdgeUpdateRequest> for grm_rs::EdgeUpdateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: EdgeUpdateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            id: request.id,
            props: request.props.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeDeleteRequest {
    pub model: String,
    pub id: i64,
}

impl From<EdgeDeleteRequest> for grm_rs::EdgeDeleteRequest {
    fn from(request: EdgeDeleteRequest) -> Self {
        Self {
            model: request.model,
            id: request.id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeFindRequest {
    pub model: String,
    pub predicates: Vec<PropertyPredicate>,
    pub order: Vec<OrderSpec>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub id: Option<i64>,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

impl TryFrom<EdgeFindRequest> for grm_rs::EdgeFindRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: EdgeFindRequest) -> grm_rs::Result<Self> {
        edge_find_shape_to_runtime(EdgeFindShape {
            model: request.model,
            predicates: request.predicates,
            order: request.order,
            limit: request.limit,
            offset: request.offset,
            id: request.id,
            from: request.from,
            to: request.to,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyMap {
    pub properties: Vec<Property>,
}

impl TryFrom<PropertyMap> for std::collections::BTreeMap<String, Value> {
    type Error = grm_rs::GrmError;

    fn try_from(map: PropertyMap) -> grm_rs::Result<Self> {
        let mut props = Self::new();
        for property in map.properties {
            if props
                .insert(property.name.clone(), property.value.try_into()?)
                .is_some()
            {
                return Err(grm_rs::GrmError::Constraint(format!(
                    "duplicate property '{}'",
                    property.name
                )));
            }
        }
        Ok(props)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub value: PropertyValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl TryFrom<PropertyValue> for Value {
    type Error = grm_rs::GrmError;

    fn try_from(value: PropertyValue) -> grm_rs::Result<Self> {
        Ok(match value {
            PropertyValue::String(value) => Self::String(value),
            PropertyValue::Int(value) => value.into(),
            PropertyValue::Float(value) => serde_json::Number::from_f64(value)
                .map(Self::Number)
                .ok_or_else(|| grm_rs::GrmError::Constraint("float value must be finite".into()))?,
            PropertyValue::Bool(value) => value.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyPredicate {
    pub field: String,
    pub op: PredicateOp,
    pub value: PropertyValue,
}

impl TryFrom<PropertyPredicate> for grm_rs::PropertyPredicate {
    type Error = grm_rs::GrmError;

    fn try_from(predicate: PropertyPredicate) -> grm_rs::Result<Self> {
        Ok(Self {
            field: predicate.field,
            op: predicate.op.into(),
            value: predicate.value.try_into()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
}

impl From<PredicateOp> for grm_rs::PredicateOp {
    fn from(op: PredicateOp) -> Self {
        match op {
            PredicateOp::Eq => Self::Eq,
            PredicateOp::Ne => Self::Ne,
            PredicateOp::Gt => Self::Gt,
            PredicateOp::Ge => Self::Ge,
            PredicateOp::Lt => Self::Lt,
            PredicateOp::Le => Self::Le,
            PredicateOp::Contains => Self::Contains,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderSpec {
    pub field: String,
    pub direction: OrderDirection,
}

impl From<OrderSpec> for grm_rs::OrderSpec {
    fn from(order: OrderSpec) -> Self {
        Self {
            field: order.field,
            direction: order.direction.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

impl From<OrderDirection> for grm_rs::OrderDirection {
    fn from(direction: OrderDirection) -> Self {
        match direction {
            OrderDirection::Asc => Self::Asc,
            OrderDirection::Desc => Self::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraversalStep {
    pub direction: TraversalDirection,
    pub edge_model: Option<String>,
    pub end_model: String,
}

impl From<TraversalStep> for grm_rs::TraversalStepRequest {
    fn from(step: TraversalStep) -> Self {
        Self {
            direction: step.direction.into(),
            edge_model: step.edge_model,
            end_model: step.end_model,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    Out,
    In,
    Both,
}

impl From<TraversalDirection> for grm_rs::TraversalDirection {
    fn from(direction: TraversalDirection) -> Self {
        match direction {
            TraversalDirection::Out => Self::Out,
            TraversalDirection::In => Self::In,
            TraversalDirection::Both => Self::Both,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalReturn {
    End,
    Root,
    Edge,
}

impl From<TraversalReturn> for grm_rs::TraversalReturn {
    fn from(return_mode: TraversalReturn) -> Self {
        match return_mode {
            TraversalReturn::End => Self::End,
            TraversalReturn::Root => Self::Root,
            TraversalReturn::Edge => Self::Edge,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryRequest {
    pub query: Query,
}

impl TryFrom<QueryRequest> for grm_rs::QueryRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: QueryRequest) -> grm_rs::Result<Self> {
        match request.query {
            Query::NodeFind(shape) => Ok(Self::NodeFind(node_find_shape_to_runtime(shape)?)),
            Query::EdgeFind(shape) => Ok(Self::EdgeFind(edge_find_shape_to_runtime(shape)?)),
            Query::Traversal(request) => Ok(Self::Traversal(grm_rs::TraversalRequest {
                root: node_find_shape_to_runtime(request.root)?,
            })),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    NodeFind(NodeFindShape),
    EdgeFind(EdgeFindShape),
    Traversal(TraversalRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodeFindShape {
    pub model: String,
    pub predicates: Vec<PropertyPredicate>,
    pub end_predicates: Vec<PropertyPredicate>,
    pub edge_predicates: Vec<PropertyPredicate>,
    pub traversals: Vec<TraversalStep>,
    pub order: Vec<OrderSpec>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub id: Option<i64>,
    pub return_mode: Option<TraversalReturn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeFindShape {
    pub model: String,
    pub predicates: Vec<PropertyPredicate>,
    pub order: Vec<OrderSpec>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub id: Option<i64>,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraversalRequest {
    pub root: NodeFindShape,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExplainRequest {
    pub query: QueryRequest,
}

impl TryFrom<ExplainRequest> for grm_rs::ExplainRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: ExplainRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            query: request.query.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileRequest {
    pub query: QueryRequest,
}

impl TryFrom<ProfileRequest> for grm_rs::ProfileRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: ProfileRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            query: request.query.try_into()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchRequest {
    pub atomic: bool,
    pub allow_deletes: bool,
    pub response_mode: BatchResponseMode,
    pub ops: Vec<BatchOperation>,
}

impl TryFrom<BatchRequest> for grm_rs::BatchRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: BatchRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            atomic: request.atomic,
            allow_deletes: request.allow_deletes,
            response: request.response_mode.into(),
            ops: request
                .ops
                .into_iter()
                .map(TryInto::try_into)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchResponseMode {
    Summary,
    Detailed,
}

impl From<BatchResponseMode> for grm_rs::SessionBatchResponse {
    fn from(mode: BatchResponseMode) -> Self {
        match mode {
            BatchResponseMode::Summary => Self::Summary,
            BatchResponseMode::Detailed => Self::Detailed,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BatchOperation {
    SchemaDefineNode(DefineNodeRequest),
    SchemaDefineEdge(DefineEdgeRequest),
    NodeCreate(BatchNodeCreate),
    NodeUpdate(NodeUpdateRequest),
    NodeDelete(NodeDeleteRequest),
    EdgeCreate(BatchEdgeCreate),
    EdgeUpdate(EdgeUpdateRequest),
    EdgeDelete(EdgeDeleteRequest),
}

impl TryFrom<BatchOperation> for grm_rs::SessionBatchOp {
    type Error = grm_rs::GrmError;

    fn try_from(op: BatchOperation) -> grm_rs::Result<Self> {
        Ok(match op {
            BatchOperation::SchemaDefineNode(request) => {
                Self::SchemaDefineNode(grm_rs::SessionBatchDefineNodeParams {
                    name: request.name,
                    id_field: request.id_field,
                    fields: service_fields_to_batch_fields(request.fields)?,
                })
            }
            BatchOperation::SchemaDefineEdge(request) => {
                Self::SchemaDefineEdge(grm_rs::SessionBatchDefineEdgeParams {
                    name: request.name,
                    from_model: request.from_model,
                    to_model: request.to_model,
                    id_field: request.id_field,
                    fields: service_fields_to_batch_fields(request.fields)?,
                })
            }
            BatchOperation::NodeCreate(request) => {
                Self::NodeCreate(grm_rs::SessionBatchNodeCreateParams {
                    model: request.model,
                    props: request.props.try_into()?,
                    local_ref: request.local_ref,
                })
            }
            BatchOperation::NodeUpdate(request) => {
                Self::NodeUpdate(grm_rs::SessionBatchNodeUpdateParams {
                    model: request.model,
                    id: request.id,
                    props: request.props.try_into()?,
                })
            }
            BatchOperation::NodeDelete(request) => {
                Self::NodeDelete(grm_rs::SessionBatchNodeDeleteParams {
                    model: request.model,
                    id: request.id,
                })
            }
            BatchOperation::EdgeCreate(request) => {
                Self::EdgeCreate(grm_rs::SessionBatchEdgeCreateParams {
                    model: request.model,
                    from: request.from.into(),
                    to: request.to.into(),
                    props: request.props.try_into()?,
                })
            }
            BatchOperation::EdgeUpdate(request) => {
                Self::EdgeUpdate(grm_rs::SessionBatchEdgeUpdateParams {
                    model: request.model,
                    id: request.id,
                    props: request.props.try_into()?,
                })
            }
            BatchOperation::EdgeDelete(request) => {
                Self::EdgeDelete(grm_rs::SessionBatchEdgeDeleteParams {
                    model: request.model,
                    id: request.id,
                })
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchNodeCreate {
    pub model: String,
    pub props: PropertyMap,
    pub local_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BatchEdgeCreate {
    pub model: String,
    pub from: BatchEndpoint,
    pub to: BatchEndpoint,
    pub props: PropertyMap,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BatchEndpoint {
    Id(i64),
    LocalRef(String),
}

impl From<BatchEndpoint> for grm_rs::SessionBatchEndpoint {
    fn from(endpoint: BatchEndpoint) -> Self {
        match endpoint {
            BatchEndpoint::Id(id) => Self::Id(id),
            BatchEndpoint::LocalRef(local_ref) => Self::Ref(local_ref),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SaveRequest {
    pub format: DurabilityFormat,
    pub requested_snapshot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadRequest {
    pub format: DurabilityFormat,
    pub snapshot: SnapshotHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    pub snapshot: SnapshotHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportRequest {
    pub document: Vec<u8>,
    pub format: DurabilityFormat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexListRequest {}

#[derive(Debug, Clone, PartialEq)]
pub struct SummaryRequest {}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotHandle {
    pub id: String,
    pub etag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityFormat {
    Json,
    Binary,
}

impl TryFrom<proto::DefineNodeRequest> for DefineNodeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::DefineNodeRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            name: request.name,
            id_field: request.id_field,
            fields: request
                .fields
                .into_iter()
                .map(TryInto::try_into)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        })
    }
}

impl TryFrom<proto::DefineEdgeRequest> for DefineEdgeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::DefineEdgeRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            name: request.name,
            from_model: request.from_model,
            to_model: request.to_model,
            id_field: request.id_field,
            fields: request
                .fields
                .into_iter()
                .map(TryInto::try_into)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        })
    }
}

impl From<proto::SchemaListRequest> for SchemaListRequest {
    fn from(_: proto::SchemaListRequest) -> Self {
        Self {}
    }
}

impl TryFrom<proto::FieldSpec> for FieldSpec {
    type Error = grm_rs::GrmError;

    fn try_from(field: proto::FieldSpec) -> grm_rs::Result<Self> {
        Ok(Self {
            name: field.name,
            value_type: proto_field_value_type(field.value_type)?,
            required: field.required,
        })
    }
}

impl TryFrom<proto::NodeCreateRequest> for NodeCreateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::NodeCreateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            props: proto_property_map_or_empty(request.props)?,
        })
    }
}

impl TryFrom<proto::NodeUpdateRequest> for NodeUpdateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::NodeUpdateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            id: request.id,
            props: proto_property_map_or_empty(request.props)?,
        })
    }
}

impl From<proto::NodeDeleteRequest> for NodeDeleteRequest {
    fn from(request: proto::NodeDeleteRequest) -> Self {
        Self {
            model: request.model,
            id: request.id,
        }
    }
}

impl TryFrom<proto::NodeFindRequest> for NodeFindRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::NodeFindRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            predicates: convert_proto_vec(request.predicates)?,
            end_predicates: convert_proto_vec(request.end_predicates)?,
            edge_predicates: convert_proto_vec(request.edge_predicates)?,
            traversals: convert_proto_vec(request.traversals)?,
            order: convert_proto_vec(request.order)?,
            limit: request.limit,
            offset: request.offset,
            id: request.id,
            return_mode: request
                .return_mode
                .map(proto_traversal_return)
                .transpose()?,
        })
    }
}

impl TryFrom<proto::EdgeCreateRequest> for EdgeCreateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::EdgeCreateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            from: request.from,
            to: request.to,
            props: proto_property_map_or_empty(request.props)?,
        })
    }
}

impl TryFrom<proto::EdgeUpdateRequest> for EdgeUpdateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::EdgeUpdateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            id: request.id,
            props: proto_property_map_or_empty(request.props)?,
        })
    }
}

impl From<proto::EdgeDeleteRequest> for EdgeDeleteRequest {
    fn from(request: proto::EdgeDeleteRequest) -> Self {
        Self {
            model: request.model,
            id: request.id,
        }
    }
}

impl TryFrom<proto::EdgeFindRequest> for EdgeFindRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::EdgeFindRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            predicates: convert_proto_vec(request.predicates)?,
            order: convert_proto_vec(request.order)?,
            limit: request.limit,
            offset: request.offset,
            id: request.id,
            from: request.from,
            to: request.to,
        })
    }
}

impl TryFrom<proto::PropertyMap> for PropertyMap {
    type Error = grm_rs::GrmError;

    fn try_from(map: proto::PropertyMap) -> grm_rs::Result<Self> {
        Ok(Self {
            properties: map
                .properties
                .into_iter()
                .map(TryInto::try_into)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        })
    }
}

impl TryFrom<proto::Property> for Property {
    type Error = grm_rs::GrmError;

    fn try_from(property: proto::Property) -> grm_rs::Result<Self> {
        Ok(Self {
            name: property.name,
            value: property
                .value
                .ok_or_else(|| missing_proto_field("Property.value"))?
                .try_into()?,
        })
    }
}

impl TryFrom<proto::PropertyValue> for PropertyValue {
    type Error = grm_rs::GrmError;

    fn try_from(value: proto::PropertyValue) -> grm_rs::Result<Self> {
        use proto::property_value::Kind;

        match value
            .kind
            .ok_or_else(|| missing_proto_field("PropertyValue.kind"))?
        {
            Kind::StringValue(value) => Ok(Self::String(value)),
            Kind::IntValue(value) => Ok(Self::Int(value)),
            Kind::FloatValue(value) => Ok(Self::Float(value)),
            Kind::BoolValue(value) => Ok(Self::Bool(value)),
        }
    }
}

impl TryFrom<proto::PropertyPredicate> for PropertyPredicate {
    type Error = grm_rs::GrmError;

    fn try_from(predicate: proto::PropertyPredicate) -> grm_rs::Result<Self> {
        Ok(Self {
            field: predicate.field,
            op: proto_predicate_op(predicate.op)?,
            value: predicate
                .value
                .ok_or_else(|| missing_proto_field("PropertyPredicate.value"))?
                .try_into()?,
        })
    }
}

impl TryFrom<proto::OrderSpec> for OrderSpec {
    type Error = grm_rs::GrmError;

    fn try_from(order: proto::OrderSpec) -> grm_rs::Result<Self> {
        Ok(Self {
            field: order.field,
            direction: proto_order_direction(order.direction)?,
        })
    }
}

impl TryFrom<proto::TraversalStep> for TraversalStep {
    type Error = grm_rs::GrmError;

    fn try_from(step: proto::TraversalStep) -> grm_rs::Result<Self> {
        Ok(Self {
            direction: proto_traversal_direction(step.direction)?,
            edge_model: step.edge_model,
            end_model: step.end_model,
        })
    }
}

impl TryFrom<proto::QueryRequest> for QueryRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::QueryRequest) -> grm_rs::Result<Self> {
        use proto::query_request::Query as ProtoQuery;

        let query = match request
            .query
            .ok_or_else(|| missing_proto_field("QueryRequest.query"))?
        {
            ProtoQuery::NodeFind(shape) => Query::NodeFind(shape.try_into()?),
            ProtoQuery::EdgeFind(shape) => Query::EdgeFind(shape.try_into()?),
            ProtoQuery::Traversal(request) => Query::Traversal(request.try_into()?),
        };
        Ok(Self { query })
    }
}

impl TryFrom<proto::NodeFindShape> for NodeFindShape {
    type Error = grm_rs::GrmError;

    fn try_from(shape: proto::NodeFindShape) -> grm_rs::Result<Self> {
        Ok(Self {
            model: shape.model,
            predicates: convert_proto_vec(shape.predicates)?,
            end_predicates: convert_proto_vec(shape.end_predicates)?,
            edge_predicates: convert_proto_vec(shape.edge_predicates)?,
            traversals: convert_proto_vec(shape.traversals)?,
            order: convert_proto_vec(shape.order)?,
            limit: shape.limit,
            offset: shape.offset,
            id: shape.id,
            return_mode: shape.return_mode.map(proto_traversal_return).transpose()?,
        })
    }
}

impl TryFrom<proto::EdgeFindShape> for EdgeFindShape {
    type Error = grm_rs::GrmError;

    fn try_from(shape: proto::EdgeFindShape) -> grm_rs::Result<Self> {
        Ok(Self {
            model: shape.model,
            predicates: convert_proto_vec(shape.predicates)?,
            order: convert_proto_vec(shape.order)?,
            limit: shape.limit,
            offset: shape.offset,
            id: shape.id,
            from: shape.from,
            to: shape.to,
        })
    }
}

impl TryFrom<proto::TraversalRequest> for TraversalRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::TraversalRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            root: request
                .root
                .ok_or_else(|| missing_proto_field("TraversalRequest.root"))?
                .try_into()?,
        })
    }
}

impl TryFrom<proto::ExplainRequest> for ExplainRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::ExplainRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            query: request
                .query
                .ok_or_else(|| missing_proto_field("ExplainRequest.query"))?
                .try_into()?,
        })
    }
}

impl TryFrom<proto::ProfileRequest> for ProfileRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::ProfileRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            query: request
                .query
                .ok_or_else(|| missing_proto_field("ProfileRequest.query"))?
                .try_into()?,
        })
    }
}

impl TryFrom<proto::BatchRequest> for BatchRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::BatchRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            atomic: request.atomic,
            allow_deletes: request.allow_deletes,
            response_mode: proto_batch_response_mode(request.response_mode)?,
            ops: convert_proto_vec(request.ops)?,
        })
    }
}

impl TryFrom<proto::BatchOperation> for BatchOperation {
    type Error = grm_rs::GrmError;

    fn try_from(op: proto::BatchOperation) -> grm_rs::Result<Self> {
        use proto::batch_operation::Op as ProtoOp;

        match op
            .op
            .ok_or_else(|| missing_proto_field("BatchOperation.op"))?
        {
            ProtoOp::SchemaDefineNode(request) => Ok(Self::SchemaDefineNode(request.try_into()?)),
            ProtoOp::SchemaDefineEdge(request) => Ok(Self::SchemaDefineEdge(request.try_into()?)),
            ProtoOp::NodeCreate(request) => Ok(Self::NodeCreate(request.try_into()?)),
            ProtoOp::NodeUpdate(request) => Ok(Self::NodeUpdate(request.try_into()?)),
            ProtoOp::NodeDelete(request) => Ok(Self::NodeDelete(request.into())),
            ProtoOp::EdgeCreate(request) => Ok(Self::EdgeCreate(request.try_into()?)),
            ProtoOp::EdgeUpdate(request) => Ok(Self::EdgeUpdate(request.try_into()?)),
            ProtoOp::EdgeDelete(request) => Ok(Self::EdgeDelete(request.into())),
        }
    }
}

impl TryFrom<proto::BatchNodeCreate> for BatchNodeCreate {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::BatchNodeCreate) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            props: proto_property_map_or_empty(request.props)?,
            local_ref: request.local_ref,
        })
    }
}

impl TryFrom<proto::BatchEdgeCreate> for BatchEdgeCreate {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::BatchEdgeCreate) -> grm_rs::Result<Self> {
        Ok(Self {
            model: request.model,
            from: request
                .from
                .ok_or_else(|| missing_proto_field("BatchEdgeCreate.from"))?
                .try_into()?,
            to: request
                .to
                .ok_or_else(|| missing_proto_field("BatchEdgeCreate.to"))?
                .try_into()?,
            props: proto_property_map_or_empty(request.props)?,
        })
    }
}

impl TryFrom<proto::BatchEndpoint> for BatchEndpoint {
    type Error = grm_rs::GrmError;

    fn try_from(endpoint: proto::BatchEndpoint) -> grm_rs::Result<Self> {
        use proto::batch_endpoint::Endpoint;

        match endpoint
            .endpoint
            .ok_or_else(|| missing_proto_field("BatchEndpoint.endpoint"))?
        {
            Endpoint::Id(id) => Ok(Self::Id(id)),
            Endpoint::LocalRef(local_ref) => Ok(Self::LocalRef(local_ref)),
        }
    }
}

impl TryFrom<proto::SaveRequest> for SaveRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::SaveRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            format: proto_durability_format(request.format)?,
            requested_snapshot_id: request.requested_snapshot_id,
        })
    }
}

impl TryFrom<proto::LoadRequest> for LoadRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::LoadRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            format: proto_durability_format(request.format)?,
            snapshot: request
                .snapshot
                .ok_or_else(|| missing_proto_field("LoadRequest.snapshot"))?
                .into(),
        })
    }
}

impl From<proto::SnapshotHandle> for SnapshotHandle {
    fn from(snapshot: proto::SnapshotHandle) -> Self {
        Self {
            id: snapshot.id,
            etag: snapshot.etag,
        }
    }
}

impl TryFrom<proto::ExportRequest> for ExportRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::ExportRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            snapshot: request
                .snapshot
                .ok_or_else(|| missing_proto_field("ExportRequest.snapshot"))?
                .into(),
        })
    }
}

impl TryFrom<proto::ImportRequest> for ImportRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::ImportRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            document: request.document,
            format: proto_durability_format(request.format)?,
        })
    }
}

impl From<proto::IndexListRequest> for IndexListRequest {
    fn from(_: proto::IndexListRequest) -> Self {
        Self {}
    }
}

impl From<proto::SummaryRequest> for SummaryRequest {
    fn from(_: proto::SummaryRequest) -> Self {
        Self {}
    }
}

fn convert_fields(fields: Vec<FieldSpec>) -> grm_rs::Result<Vec<grm_rs::FieldSpec>> {
    fields.into_iter().map(TryInto::try_into).collect()
}

fn convert_proto_vec<T, U>(values: Vec<T>) -> grm_rs::Result<Vec<U>>
where
    U: TryFrom<T, Error = grm_rs::GrmError>,
{
    values.into_iter().map(TryInto::try_into).collect()
}

fn proto_property_map_or_empty(map: Option<proto::PropertyMap>) -> grm_rs::Result<PropertyMap> {
    map.map(TryInto::try_into).transpose().map(|map| {
        map.unwrap_or(PropertyMap {
            properties: Vec::new(),
        })
    })
}

fn service_fields_to_batch_fields(
    fields: Vec<FieldSpec>,
) -> grm_rs::Result<Vec<grm_rs::SessionBatchFieldParam>> {
    fields
        .into_iter()
        .map(|field| {
            Ok(grm_rs::SessionBatchFieldParam {
                name: field.name,
                value_type: field_value_type_keyword(field.value_type)?.to_string(),
                required: field.required,
            })
        })
        .collect()
}

fn node_find_shape_to_runtime(shape: NodeFindShape) -> grm_rs::Result<grm_rs::NodeFindRequest> {
    Ok(grm_rs::NodeFindRequest {
        model: shape.model,
        predicates: shape
            .predicates
            .into_iter()
            .map(TryInto::try_into)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        end_predicates: shape
            .end_predicates
            .into_iter()
            .map(TryInto::try_into)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        edge_predicates: shape
            .edge_predicates
            .into_iter()
            .map(TryInto::try_into)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        traversals: shape.traversals.into_iter().map(Into::into).collect(),
        order: shape.order.into_iter().map(Into::into).collect(),
        limit: convert_u64_option_to_usize(shape.limit, "limit")?,
        offset: convert_u64_option_to_usize(shape.offset, "offset")?,
        id: shape.id,
        return_mode: shape.return_mode.map(Into::into),
    })
}

fn edge_find_shape_to_runtime(shape: EdgeFindShape) -> grm_rs::Result<grm_rs::EdgeFindRequest> {
    Ok(grm_rs::EdgeFindRequest {
        model: shape.model,
        predicates: shape
            .predicates
            .into_iter()
            .map(TryInto::try_into)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        order: shape.order.into_iter().map(Into::into).collect(),
        limit: convert_u64_option_to_usize(shape.limit, "limit")?,
        offset: convert_u64_option_to_usize(shape.offset, "offset")?,
        id: shape.id,
        from: shape.from,
        to: shape.to,
    })
}

fn convert_u64_option_to_usize(value: Option<u64>, field: &str) -> grm_rs::Result<Option<usize>> {
    value
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| grm_rs::GrmError::Constraint(format!("{field} is too large")))
        })
        .transpose()
}

fn field_value_type_keyword(value_type: FieldValueType) -> grm_rs::Result<&'static str> {
    match value_type {
        FieldValueType::Unspecified => Err(grm_rs::GrmError::Constraint(
            "field value type must be specified".into(),
        )),
        FieldValueType::String => Ok("string"),
        FieldValueType::Int => Ok("int"),
        FieldValueType::Float => Ok("float"),
        FieldValueType::Bool => Ok("bool"),
    }
}

fn missing_proto_field(field: &'static str) -> grm_rs::GrmError {
    grm_rs::GrmError::Constraint(format!("missing required protobuf field '{field}'"))
}

fn unknown_proto_enum(enum_name: &'static str, value: i32) -> grm_rs::GrmError {
    grm_rs::GrmError::Constraint(format!("unknown {enum_name} enum value {value}"))
}

fn proto_field_value_type(value: i32) -> grm_rs::Result<FieldValueType> {
    match proto::FieldValueType::try_from(value)
        .map_err(|_| unknown_proto_enum("FieldValueType", value))?
    {
        proto::FieldValueType::Unspecified => Ok(FieldValueType::Unspecified),
        proto::FieldValueType::String => Ok(FieldValueType::String),
        proto::FieldValueType::Int => Ok(FieldValueType::Int),
        proto::FieldValueType::Float => Ok(FieldValueType::Float),
        proto::FieldValueType::Bool => Ok(FieldValueType::Bool),
    }
}

fn proto_predicate_op(value: i32) -> grm_rs::Result<PredicateOp> {
    match proto::PredicateOp::try_from(value)
        .map_err(|_| unknown_proto_enum("PredicateOp", value))?
    {
        proto::PredicateOp::Eq => Ok(PredicateOp::Eq),
        proto::PredicateOp::Ne => Ok(PredicateOp::Ne),
        proto::PredicateOp::Gt => Ok(PredicateOp::Gt),
        proto::PredicateOp::Ge => Ok(PredicateOp::Ge),
        proto::PredicateOp::Lt => Ok(PredicateOp::Lt),
        proto::PredicateOp::Le => Ok(PredicateOp::Le),
        proto::PredicateOp::Contains => Ok(PredicateOp::Contains),
    }
}

fn proto_order_direction(value: i32) -> grm_rs::Result<OrderDirection> {
    match proto::OrderDirection::try_from(value)
        .map_err(|_| unknown_proto_enum("OrderDirection", value))?
    {
        proto::OrderDirection::Asc => Ok(OrderDirection::Asc),
        proto::OrderDirection::Desc => Ok(OrderDirection::Desc),
    }
}

fn proto_traversal_direction(value: i32) -> grm_rs::Result<TraversalDirection> {
    match proto::TraversalDirection::try_from(value)
        .map_err(|_| unknown_proto_enum("TraversalDirection", value))?
    {
        proto::TraversalDirection::Out => Ok(TraversalDirection::Out),
        proto::TraversalDirection::In => Ok(TraversalDirection::In),
        proto::TraversalDirection::Both => Ok(TraversalDirection::Both),
    }
}

fn proto_traversal_return(value: i32) -> grm_rs::Result<TraversalReturn> {
    match proto::TraversalReturn::try_from(value)
        .map_err(|_| unknown_proto_enum("TraversalReturn", value))?
    {
        proto::TraversalReturn::End => Ok(TraversalReturn::End),
        proto::TraversalReturn::Root => Ok(TraversalReturn::Root),
        proto::TraversalReturn::Edge => Ok(TraversalReturn::Edge),
    }
}

fn proto_batch_response_mode(value: i32) -> grm_rs::Result<BatchResponseMode> {
    match proto::BatchResponseMode::try_from(value)
        .map_err(|_| unknown_proto_enum("BatchResponseMode", value))?
    {
        proto::BatchResponseMode::Summary => Ok(BatchResponseMode::Summary),
        proto::BatchResponseMode::Detailed => Ok(BatchResponseMode::Detailed),
    }
}

fn proto_durability_format(value: i32) -> grm_rs::Result<DurabilityFormat> {
    match proto::DurabilityFormat::try_from(value)
        .map_err(|_| unknown_proto_enum("DurabilityFormat", value))?
    {
        proto::DurabilityFormat::Json => Ok(DurabilityFormat::Json),
        proto::DurabilityFormat::Binary => Ok(DurabilityFormat::Binary),
    }
}
