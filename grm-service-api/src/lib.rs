//! Split-ready service API contract artifacts for GRM.
//!
//! This crate intentionally contains the protobuf source contract rather than a
//! daemon or hosted transport policy. It is client-facing and can be split from
//! the monorepo later without depending on private daemon internals. The local
//! gRPC shell is a transport proof over the in-process workspace service.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

#[allow(warnings)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/grm.service.v1.rs"));
}

pub use proto::grm_service_server::GrmServiceServer;

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
    "grm/service/v1/workspace.proto",
    "grm/service/v1/service.proto",
];

#[derive(Debug)]
pub enum WorkspaceServiceError {
    Runtime(grm_rs::GrmError),
    UnknownWorkspaceHandle { handle: WorkspaceHandle },
    UnknownSnapshotHandle { snapshot: SnapshotHandle },
    UnsupportedWorkspaceOperation(&'static str),
}

impl fmt::Display for WorkspaceServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => write!(f, "{error}"),
            Self::UnknownWorkspaceHandle { handle } => {
                write!(f, "unknown workspace handle '{}'", handle.id)
            }
            Self::UnknownSnapshotHandle { snapshot } => {
                write!(f, "unknown workspace snapshot handle '{}'", snapshot.id)
            }
            Self::UnsupportedWorkspaceOperation(operation) => {
                write!(f, "workspace operation is not supported: {operation}")
            }
        }
    }
}

impl Error for WorkspaceServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
            Self::UnknownWorkspaceHandle { .. }
            | Self::UnknownSnapshotHandle { .. }
            | Self::UnsupportedWorkspaceOperation(_) => None,
        }
    }
}

impl From<grm_rs::GrmError> for WorkspaceServiceError {
    fn from(error: grm_rs::GrmError) -> Self {
        Self::Runtime(error)
    }
}

pub type WorkspaceServiceResult<T> = std::result::Result<T, WorkspaceServiceError>;

#[derive(Debug)]
pub enum GrpcWorkspaceClientError {
    Transport(tonic::transport::Error),
    Status(tonic::Status),
    MissingField(&'static str),
}

impl fmt::Display for GrpcWorkspaceClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "gRPC transport error: {error}"),
            Self::Status(status) => write!(f, "gRPC service error: {status}"),
            Self::MissingField(field) => write!(f, "gRPC response missing required field {field}"),
        }
    }
}

impl Error for GrpcWorkspaceClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Status(status) => Some(status),
            Self::MissingField(_) => None,
        }
    }
}

impl From<tonic::transport::Error> for GrpcWorkspaceClientError {
    fn from(error: tonic::transport::Error) -> Self {
        Self::Transport(error)
    }
}

impl From<tonic::Status> for GrpcWorkspaceClientError {
    fn from(status: tonic::Status) -> Self {
        Self::Status(status)
    }
}

pub type GrpcWorkspaceClientResult<T> = std::result::Result<T, GrpcWorkspaceClientError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrpcWorkspaceMode {
    Create,
    Open,
}

#[derive(Clone)]
pub struct GrpcWorkspaceClient {
    endpoint: String,
    workspace: proto::WorkspaceRef,
    handle: proto::WorkspaceHandle,
    client: proto::grm_service_client::GrmServiceClient<Channel>,
}

impl GrpcWorkspaceClient {
    pub async fn connect(
        endpoint: impl Into<String>,
        workspace_id: impl Into<String>,
        mode: GrpcWorkspaceMode,
    ) -> GrpcWorkspaceClientResult<Self> {
        let endpoint = endpoint.into();
        let workspace = proto::WorkspaceRef {
            id: workspace_id.into(),
        };
        let mut client = proto::grm_service_client::GrmServiceClient::connect(endpoint.clone())
            .await
            .map_err(GrpcWorkspaceClientError::Transport)?;
        let format = proto::DurabilityFormat::Json as i32;
        let handle = match mode {
            GrpcWorkspaceMode::Create => client
                .create_workspace(proto::WorkspaceCreateRequest {
                    mode: proto::WorkspaceCreateMode::LocalAutocommit as i32,
                    workspace: Some(workspace.clone()),
                    format,
                })
                .await?
                .into_inner()
                .handle
                .ok_or(GrpcWorkspaceClientError::MissingField(
                    "WorkspaceCreateResponse.handle",
                ))?,
            GrpcWorkspaceMode::Open => client
                .open_workspace(proto::WorkspaceOpenRequest {
                    snapshot: None,
                    workspace: Some(workspace.clone()),
                    format,
                })
                .await?
                .into_inner()
                .handle
                .ok_or(GrpcWorkspaceClientError::MissingField(
                    "WorkspaceOpenResponse.handle",
                ))?,
        };

        Ok(Self {
            endpoint,
            workspace,
            handle,
            client,
        })
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn workspace_ref(&self) -> &proto::WorkspaceRef {
        &self.workspace
    }

    pub fn handle(&self) -> &proto::WorkspaceHandle {
        &self.handle
    }

    pub async fn execute_proto(
        &mut self,
        request: proto::runtime_request::Request,
    ) -> GrpcWorkspaceClientResult<proto::WorkspaceRuntimeResponse> {
        self.client
            .execute_workspace(proto::WorkspaceRuntimeRequest {
                handle: Some(self.handle.clone()),
                request: Some(proto::RuntimeRequest {
                    request: Some(request),
                }),
            })
            .await
            .map(|response| response.into_inner())
            .map_err(GrpcWorkspaceClientError::Status)
    }

    pub async fn close(mut self) -> GrpcWorkspaceClientResult<proto::WorkspaceCloseResponse> {
        self.client
            .close_workspace(proto::WorkspaceCloseRequest {
                handle: Some(self.handle.clone()),
            })
            .await
            .map(|response| response.into_inner())
            .map_err(GrpcWorkspaceClientError::Status)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceHandle {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceRef {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCreateRequest {
    pub mode: WorkspaceCreateMode,
    pub workspace: Option<WorkspaceRef>,
    pub format: DurabilityFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCreateMode {
    InMemory,
    LocalAutocommit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCreateResponse {
    pub handle: WorkspaceHandle,
    pub workspace: Option<WorkspaceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOpenRequest {
    pub snapshot: Option<SnapshotHandle>,
    pub workspace: Option<WorkspaceRef>,
    pub format: DurabilityFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOpenResponse {
    pub handle: WorkspaceHandle,
    pub workspace: Option<WorkspaceRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceRuntimeRequest {
    pub handle: WorkspaceHandle,
    pub request: ServiceRequest,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeResponse {
    pub handle: WorkspaceHandle,
    pub response: grm_rs::RuntimeResponse,
    pub durable_operations: Vec<grm_rs::DurableOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCloseRequest {
    pub handle: WorkspaceHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCloseResponse {
    pub handle: WorkspaceHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceUnsupportedRequest {
    pub operation: WorkspaceUnsupportedOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceUnsupportedOperation {
    OpenLoopExternalInference,
    HostedDurability,
    DaemonLifecycle,
}

impl WorkspaceUnsupportedOperation {
    fn name(self) -> &'static str {
        match self {
            Self::OpenLoopExternalInference => "open-loop external inference",
            Self::HostedDurability => "hosted durability",
            Self::DaemonLifecycle => "daemon lifecycle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalWorkspaceSnapshotRequest {
    pub format: DurabilityFormat,
    pub path: PathBuf,
}

#[derive(Default)]
pub struct InProcessWorkspaceService {
    next_workspace_id: u64,
    next_workspace_ref_id: u64,
    next_snapshot_id: u64,
    workspaces: BTreeMap<String, grm_rs::Workspace>,
    local_snapshots: BTreeMap<String, LocalWorkspaceSnapshot>,
    local_workspace_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct LocalWorkspaceSnapshot {
    format: DurabilityFormat,
    path: PathBuf,
}

impl InProcessWorkspaceService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_local_workspace_root(root: impl Into<PathBuf>) -> Self {
        Self {
            local_workspace_root: Some(root.into()),
            ..Self::default()
        }
    }

    pub fn set_local_workspace_root(&mut self, root: impl Into<PathBuf>) {
        self.local_workspace_root = Some(root.into());
    }

    pub fn create_workspace(
        &mut self,
        request: WorkspaceCreateRequest,
    ) -> WorkspaceServiceResult<WorkspaceCreateResponse> {
        match request.mode {
            WorkspaceCreateMode::InMemory => {
                let handle = self.next_workspace_handle();
                self.workspaces
                    .insert(handle.id.clone(), grm_rs::Workspace::new());
                Ok(WorkspaceCreateResponse {
                    handle,
                    workspace: None,
                })
            }
            WorkspaceCreateMode::LocalAutocommit => {
                let workspace_ref = self.normalize_workspace_ref(request.workspace)?;
                let path = self.local_workspace_path(&workspace_ref, request.format)?;
                let mut workspace = grm_rs::Workspace::new();
                workspace.enable_autocommit(request.format.into(), path)?;
                let handle = self.next_workspace_handle();
                self.workspaces.insert(handle.id.clone(), workspace);
                Ok(WorkspaceCreateResponse {
                    handle,
                    workspace: Some(workspace_ref),
                })
            }
        }
    }

    pub fn open_workspace(
        &mut self,
        request: WorkspaceOpenRequest,
    ) -> WorkspaceServiceResult<WorkspaceOpenResponse> {
        let (workspace, workspace_ref) = match (request.workspace, request.snapshot) {
            (Some(_), Some(_)) => {
                return Err(WorkspaceServiceError::Runtime(
                    grm_rs::GrmError::Constraint(
                        "workspace open accepts either a workspace ref or snapshot handle, not both"
                            .into(),
                    ),
                ));
            }
            (Some(workspace_ref), None) => {
                let path = self.local_workspace_path(&workspace_ref, request.format)?;
                (
                    grm_rs::Workspace::open_autocommit(request.format.into(), path)?,
                    Some(workspace_ref),
                )
            }
            (None, Some(snapshot_handle)) => {
                let snapshot = self
                    .local_snapshots
                    .get(&snapshot_handle.id)
                    .ok_or_else(|| WorkspaceServiceError::UnknownSnapshotHandle {
                        snapshot: snapshot_handle.clone(),
                    })?;
                if snapshot.format != request.format {
                    return Err(WorkspaceServiceError::Runtime(
                        grm_rs::GrmError::Constraint(
                            "workspace open format does not match registered snapshot".into(),
                        ),
                    ));
                }

                (
                    grm_rs::Workspace::open(request.format.into(), &snapshot.path)?,
                    None,
                )
            }
            (None, None) => {
                return Err(WorkspaceServiceError::Runtime(
                    grm_rs::GrmError::Constraint(
                        "workspace open requires a workspace ref or snapshot handle".into(),
                    ),
                ));
            }
        };
        let handle = self.next_workspace_handle();
        self.workspaces.insert(handle.id.clone(), workspace);
        Ok(WorkspaceOpenResponse {
            handle,
            workspace: workspace_ref,
        })
    }

    pub async fn execute_runtime(
        &mut self,
        request: WorkspaceRuntimeRequest,
    ) -> WorkspaceServiceResult<WorkspaceRuntimeResponse> {
        let handle = request.handle;
        let workspace = self.workspace_mut(&handle)?;
        let outcome = workspace
            .execute_runtime(request.request.into_runtime_request()?)
            .await?;
        Ok(WorkspaceRuntimeResponse {
            handle,
            response: outcome.response,
            durable_operations: outcome.durable_ops,
        })
    }

    pub fn close_workspace(
        &mut self,
        request: WorkspaceCloseRequest,
    ) -> WorkspaceServiceResult<WorkspaceCloseResponse> {
        self.workspaces
            .remove(&request.handle.id)
            .map(|_| WorkspaceCloseResponse {
                handle: request.handle.clone(),
            })
            .ok_or(WorkspaceServiceError::UnknownWorkspaceHandle {
                handle: request.handle,
            })
    }

    pub fn unsupported_workspace_operation(
        &self,
        request: WorkspaceUnsupportedRequest,
    ) -> WorkspaceServiceResult<()> {
        Err(WorkspaceServiceError::UnsupportedWorkspaceOperation(
            request.operation.name(),
        ))
    }

    /// Local adapter hook for tests and transitional single-process tools.
    ///
    /// The public workspace open request consumes only the returned snapshot
    /// handle; server-local paths intentionally do not appear in the service
    /// contract proto.
    pub fn register_local_workspace_snapshot(
        &mut self,
        request: LocalWorkspaceSnapshotRequest,
    ) -> SnapshotHandle {
        let snapshot = self.next_snapshot_handle();
        self.local_snapshots.insert(
            snapshot.id.clone(),
            LocalWorkspaceSnapshot {
                format: request.format,
                path: request.path,
            },
        );
        snapshot
    }

    /// Local adapter hook for tests and transitional single-process tools.
    pub fn save_workspace_to_local_snapshot(
        &mut self,
        handle: &WorkspaceHandle,
        request: LocalWorkspaceSnapshotRequest,
    ) -> WorkspaceServiceResult<SnapshotHandle> {
        self.workspace(handle)?
            .save(request.format.into(), &request.path)
            .map_err(WorkspaceServiceError::Runtime)?;
        Ok(self.register_local_workspace_snapshot(request))
    }

    pub fn workspace(
        &self,
        handle: &WorkspaceHandle,
    ) -> WorkspaceServiceResult<&grm_rs::Workspace> {
        self.workspaces.get(&handle.id).ok_or_else(|| {
            WorkspaceServiceError::UnknownWorkspaceHandle {
                handle: handle.clone(),
            }
        })
    }

    pub fn workspace_mut(
        &mut self,
        handle: &WorkspaceHandle,
    ) -> WorkspaceServiceResult<&mut grm_rs::Workspace> {
        self.workspaces.get_mut(&handle.id).ok_or_else(|| {
            WorkspaceServiceError::UnknownWorkspaceHandle {
                handle: handle.clone(),
            }
        })
    }

    fn next_workspace_handle(&mut self) -> WorkspaceHandle {
        self.next_workspace_id += 1;
        WorkspaceHandle {
            id: format!("workspace-{}", self.next_workspace_id),
        }
    }

    fn next_workspace_ref(&mut self) -> WorkspaceRef {
        self.next_workspace_ref_id += 1;
        WorkspaceRef {
            id: format!("workspace-{}", self.next_workspace_ref_id),
        }
    }

    fn next_snapshot_handle(&mut self) -> SnapshotHandle {
        self.next_snapshot_id += 1;
        let id = format!("local-snapshot-{}", self.next_snapshot_id);
        SnapshotHandle {
            id,
            etag: String::new(),
        }
    }

    fn normalize_workspace_ref(
        &mut self,
        workspace: Option<WorkspaceRef>,
    ) -> WorkspaceServiceResult<WorkspaceRef> {
        match workspace {
            Some(workspace) => {
                validate_workspace_ref(&workspace)?;
                Ok(workspace)
            }
            None => Ok(self.next_workspace_ref()),
        }
    }

    fn local_workspace_path(
        &self,
        workspace: &WorkspaceRef,
        format: DurabilityFormat,
    ) -> WorkspaceServiceResult<PathBuf> {
        validate_workspace_ref(workspace)?;
        let root = self.local_workspace_root.as_ref().ok_or_else(|| {
            WorkspaceServiceError::Runtime(grm_rs::GrmError::Constraint(
                "local autocommit workspaces require a configured local workspace root".into(),
            ))
        })?;
        let extension = match format {
            DurabilityFormat::Json => "json",
            DurabilityFormat::Binary => "bin",
        };
        Ok(root.join(format!("{}.{}", workspace.id, extension)))
    }
}

fn validate_workspace_ref(workspace: &WorkspaceRef) -> WorkspaceServiceResult<()> {
    if workspace.id.is_empty() {
        return Err(WorkspaceServiceError::Runtime(
            grm_rs::GrmError::Constraint("workspace ref id must not be empty".into()),
        ));
    }
    if !workspace
        .id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(WorkspaceServiceError::Runtime(
            grm_rs::GrmError::Constraint(
                "workspace ref id may contain only ASCII letters, digits, '-' and '_'".into(),
            ),
        ));
    }
    Ok(())
}

#[derive(Clone, Default)]
pub struct GrpcWorkspaceService {
    inner: Arc<Mutex<InProcessWorkspaceService>>,
}

impl GrpcWorkspaceService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_local_workspace_root(root: impl Into<PathBuf>) -> Self {
        Self::from_in_process(InProcessWorkspaceService::with_local_workspace_root(root))
    }

    pub fn from_in_process(service: InProcessWorkspaceService) -> Self {
        Self {
            inner: Arc::new(Mutex::new(service)),
        }
    }

    pub fn into_server(self) -> GrmServiceServer<Self> {
        GrmServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl proto::grm_service_server::GrmService for GrpcWorkspaceService {
    async fn create_workspace(
        &self,
        request: Request<proto::WorkspaceCreateRequest>,
    ) -> Result<Response<proto::WorkspaceCreateResponse>, Status> {
        let request = request.into_inner().try_into().map_err(proto_status)?;
        let response = self
            .inner
            .lock()
            .await
            .create_workspace(request)
            .map_err(workspace_status)?;
        Ok(Response::new(response.into()))
    }

    async fn open_workspace(
        &self,
        request: Request<proto::WorkspaceOpenRequest>,
    ) -> Result<Response<proto::WorkspaceOpenResponse>, Status> {
        let request = request.into_inner().try_into().map_err(proto_status)?;
        let response = self
            .inner
            .lock()
            .await
            .open_workspace(request)
            .map_err(workspace_status)?;
        Ok(Response::new(response.into()))
    }

    async fn execute_workspace(
        &self,
        request: Request<proto::WorkspaceRuntimeRequest>,
    ) -> Result<Response<proto::WorkspaceRuntimeResponse>, Status> {
        let request = request.into_inner().try_into().map_err(proto_status)?;
        let response = self
            .inner
            .lock()
            .await
            .execute_runtime(request)
            .await
            .map_err(workspace_status)?
            .try_into()
            .map_err(proto_status)?;
        Ok(Response::new(response))
    }

    async fn close_workspace(
        &self,
        request: Request<proto::WorkspaceCloseRequest>,
    ) -> Result<Response<proto::WorkspaceCloseResponse>, Status> {
        let request = request.into_inner().try_into().map_err(proto_status)?;
        let response = self
            .inner
            .lock()
            .await
            .close_workspace(request)
            .map_err(workspace_status)?;
        Ok(Response::new(response.into()))
    }

    async fn define_node(
        &self,
        _request: Request<proto::DefineNodeRequest>,
    ) -> Result<Response<proto::DefineNodeResponse>, Status> {
        Err(unsupported_rpc("DefineNode"))
    }

    async fn define_edge(
        &self,
        _request: Request<proto::DefineEdgeRequest>,
    ) -> Result<Response<proto::DefineEdgeResponse>, Status> {
        Err(unsupported_rpc("DefineEdge"))
    }

    async fn schema_list(
        &self,
        _request: Request<proto::SchemaListRequest>,
    ) -> Result<Response<proto::SchemaListResponse>, Status> {
        Err(unsupported_rpc("SchemaList"))
    }

    async fn create_node(
        &self,
        _request: Request<proto::NodeCreateRequest>,
    ) -> Result<Response<proto::NodeCreateResponse>, Status> {
        Err(unsupported_rpc("CreateNode"))
    }

    async fn update_node(
        &self,
        _request: Request<proto::NodeUpdateRequest>,
    ) -> Result<Response<proto::NodeUpdateResponse>, Status> {
        Err(unsupported_rpc("UpdateNode"))
    }

    async fn delete_node(
        &self,
        _request: Request<proto::NodeDeleteRequest>,
    ) -> Result<Response<proto::NodeDeleteResponse>, Status> {
        Err(unsupported_rpc("DeleteNode"))
    }

    async fn find_nodes(
        &self,
        _request: Request<proto::NodeFindRequest>,
    ) -> Result<Response<proto::NodeFindResponse>, Status> {
        Err(unsupported_rpc("FindNodes"))
    }

    async fn create_edge(
        &self,
        _request: Request<proto::EdgeCreateRequest>,
    ) -> Result<Response<proto::EdgeCreateResponse>, Status> {
        Err(unsupported_rpc("CreateEdge"))
    }

    async fn update_edge(
        &self,
        _request: Request<proto::EdgeUpdateRequest>,
    ) -> Result<Response<proto::EdgeUpdateResponse>, Status> {
        Err(unsupported_rpc("UpdateEdge"))
    }

    async fn delete_edge(
        &self,
        _request: Request<proto::EdgeDeleteRequest>,
    ) -> Result<Response<proto::EdgeDeleteResponse>, Status> {
        Err(unsupported_rpc("DeleteEdge"))
    }

    async fn find_edges(
        &self,
        _request: Request<proto::EdgeFindRequest>,
    ) -> Result<Response<proto::EdgeFindResponse>, Status> {
        Err(unsupported_rpc("FindEdges"))
    }

    async fn query(
        &self,
        _request: Request<proto::QueryRequest>,
    ) -> Result<Response<proto::QueryResponse>, Status> {
        Err(unsupported_rpc("Query"))
    }

    async fn explain(
        &self,
        _request: Request<proto::ExplainRequest>,
    ) -> Result<Response<proto::ExplainResponse>, Status> {
        Err(unsupported_rpc("Explain"))
    }

    async fn profile(
        &self,
        _request: Request<proto::ProfileRequest>,
    ) -> Result<Response<proto::ProfileResponse>, Status> {
        Err(unsupported_rpc("Profile"))
    }

    async fn apply_batch(
        &self,
        _request: Request<proto::BatchRequest>,
    ) -> Result<Response<proto::BatchResponse>, Status> {
        Err(unsupported_rpc("ApplyBatch"))
    }

    async fn save(
        &self,
        _request: Request<proto::SaveRequest>,
    ) -> Result<Response<proto::SaveResponse>, Status> {
        Err(unsupported_rpc("Save"))
    }

    async fn load(
        &self,
        _request: Request<proto::LoadRequest>,
    ) -> Result<Response<proto::LoadResponse>, Status> {
        Err(unsupported_rpc("Load"))
    }

    async fn export(
        &self,
        _request: Request<proto::ExportRequest>,
    ) -> Result<Response<proto::ExportResponse>, Status> {
        Err(unsupported_rpc("Export"))
    }

    async fn import(
        &self,
        _request: Request<proto::ImportRequest>,
    ) -> Result<Response<proto::ImportResponse>, Status> {
        Err(unsupported_rpc("Import"))
    }

    async fn index_list(
        &self,
        _request: Request<proto::IndexListRequest>,
    ) -> Result<Response<proto::IndexListResponse>, Status> {
        Err(unsupported_rpc("IndexList"))
    }

    async fn summary(
        &self,
        _request: Request<proto::SummaryRequest>,
    ) -> Result<Response<proto::SummaryResponse>, Status> {
        Err(unsupported_rpc("Summary"))
    }
}

fn workspace_status(error: WorkspaceServiceError) -> Status {
    match error {
        WorkspaceServiceError::UnknownWorkspaceHandle { .. }
        | WorkspaceServiceError::UnknownSnapshotHandle { .. } => {
            Status::not_found(error.to_string())
        }
        WorkspaceServiceError::UnsupportedWorkspaceOperation(_) => {
            Status::unimplemented(error.to_string())
        }
        WorkspaceServiceError::Runtime(error) => proto_status(error),
    }
}

fn proto_status(error: grm_rs::GrmError) -> Status {
    match error {
        grm_rs::GrmError::NotSupported(message) => Status::unimplemented(message),
        other => Status::invalid_argument(other.to_string()),
    }
}

fn unsupported_rpc(name: &'static str) -> Status {
    Status::unimplemented(format!(
        "{name} is not exposed by this local gRPC workspace shell; use ExecuteWorkspace for workspace-scoped runtime requests"
    ))
}

impl TryFrom<proto::WorkspaceCreateRequest> for WorkspaceCreateRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::WorkspaceCreateRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            mode: proto_workspace_create_mode(request.mode)?,
            workspace: request.workspace.map(Into::into),
            format: proto_durability_format(request.format)?,
        })
    }
}

impl From<WorkspaceCreateResponse> for proto::WorkspaceCreateResponse {
    fn from(response: WorkspaceCreateResponse) -> Self {
        Self {
            handle: Some(response.handle.into()),
            workspace: response.workspace.map(Into::into),
        }
    }
}

impl TryFrom<proto::WorkspaceOpenRequest> for WorkspaceOpenRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::WorkspaceOpenRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            snapshot: request.snapshot.map(Into::into),
            workspace: request.workspace.map(Into::into),
            format: proto_durability_format(request.format)?,
        })
    }
}

impl From<WorkspaceOpenResponse> for proto::WorkspaceOpenResponse {
    fn from(response: WorkspaceOpenResponse) -> Self {
        Self {
            handle: Some(response.handle.into()),
            workspace: response.workspace.map(Into::into),
        }
    }
}

impl TryFrom<proto::WorkspaceRuntimeRequest> for WorkspaceRuntimeRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::WorkspaceRuntimeRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            handle: request
                .handle
                .ok_or_else(|| missing_proto_field("WorkspaceRuntimeRequest.handle"))?
                .into(),
            request: request
                .request
                .ok_or_else(|| missing_proto_field("WorkspaceRuntimeRequest.request"))?
                .try_into()?,
        })
    }
}

impl TryFrom<WorkspaceRuntimeResponse> for proto::WorkspaceRuntimeResponse {
    type Error = grm_rs::GrmError;

    fn try_from(response: WorkspaceRuntimeResponse) -> grm_rs::Result<Self> {
        let runtime_response =
            proto_runtime_response(response.response, response.durable_operations.as_slice())?;
        Ok(Self {
            handle: Some(response.handle.into()),
            response: Some(runtime_response),
            durable_operations: response
                .durable_operations
                .into_iter()
                .map(proto_durable_operation)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        })
    }
}

impl TryFrom<proto::WorkspaceCloseRequest> for WorkspaceCloseRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::WorkspaceCloseRequest) -> grm_rs::Result<Self> {
        Ok(Self {
            handle: request
                .handle
                .ok_or_else(|| missing_proto_field("WorkspaceCloseRequest.handle"))?
                .into(),
        })
    }
}

impl From<WorkspaceCloseResponse> for proto::WorkspaceCloseResponse {
    fn from(response: WorkspaceCloseResponse) -> Self {
        Self {
            handle: Some(response.handle.into()),
        }
    }
}

impl From<proto::WorkspaceHandle> for WorkspaceHandle {
    fn from(handle: proto::WorkspaceHandle) -> Self {
        Self { id: handle.id }
    }
}

impl From<WorkspaceHandle> for proto::WorkspaceHandle {
    fn from(handle: WorkspaceHandle) -> Self {
        Self { id: handle.id }
    }
}

impl From<proto::WorkspaceRef> for WorkspaceRef {
    fn from(workspace: proto::WorkspaceRef) -> Self {
        Self { id: workspace.id }
    }
}

impl From<WorkspaceRef> for proto::WorkspaceRef {
    fn from(workspace: WorkspaceRef) -> Self {
        Self { id: workspace.id }
    }
}

impl TryFrom<proto::RuntimeRequest> for ServiceRequest {
    type Error = grm_rs::GrmError;

    fn try_from(request: proto::RuntimeRequest) -> grm_rs::Result<Self> {
        use proto::runtime_request::Request as ProtoRequest;

        match request
            .request
            .ok_or_else(|| missing_proto_field("RuntimeRequest.request"))?
        {
            ProtoRequest::DefineNode(request) => Ok(Self::DefineNode(request.try_into()?)),
            ProtoRequest::DefineEdge(request) => Ok(Self::DefineEdge(request.try_into()?)),
            ProtoRequest::SchemaList(request) => Ok(Self::SchemaList(request.into())),
            ProtoRequest::CreateNode(request) => Ok(Self::CreateNode(request.try_into()?)),
            ProtoRequest::UpdateNode(request) => Ok(Self::UpdateNode(request.try_into()?)),
            ProtoRequest::DeleteNode(request) => Ok(Self::DeleteNode(request.into())),
            ProtoRequest::FindNodes(request) => Ok(Self::FindNodes(request.try_into()?)),
            ProtoRequest::CreateEdge(request) => Ok(Self::CreateEdge(request.try_into()?)),
            ProtoRequest::UpdateEdge(request) => Ok(Self::UpdateEdge(request.try_into()?)),
            ProtoRequest::DeleteEdge(request) => Ok(Self::DeleteEdge(request.into())),
            ProtoRequest::FindEdges(request) => Ok(Self::FindEdges(request.try_into()?)),
            ProtoRequest::Query(request) => Ok(Self::Query(request.try_into()?)),
            ProtoRequest::Explain(request) => Ok(Self::Explain(request.try_into()?)),
            ProtoRequest::Profile(request) => Ok(Self::Profile(request.try_into()?)),
            ProtoRequest::ApplyBatch(request) => Ok(Self::ApplyBatch(request.try_into()?)),
            ProtoRequest::IndexList(request) => Ok(Self::IndexList(request.into())),
            ProtoRequest::Summary(request) => Ok(Self::Summary(request.into())),
        }
    }
}

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
            ServiceRequest::SchemaList(_) => Self::Schema(grm_rs::SchemaRequest::List),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotHandle {
    pub id: String,
    pub etag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityFormat {
    Json,
    Binary,
}

impl From<DurabilityFormat> for grm_rs::DurabilityFormat {
    fn from(format: DurabilityFormat) -> Self {
        match format {
            DurabilityFormat::Json => Self::Json,
            DurabilityFormat::Binary => Self::Binary,
        }
    }
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

impl From<SnapshotHandle> for proto::SnapshotHandle {
    fn from(snapshot: SnapshotHandle) -> Self {
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

fn proto_runtime_response(
    response: grm_rs::RuntimeResponse,
    durable_ops: &[grm_rs::DurableOperation],
) -> grm_rs::Result<proto::RuntimeResponse> {
    use proto::runtime_response::Response as ProtoResponse;

    let response = match response {
        grm_rs::RuntimeResponse::Schema(grm_rs::SchemaResponse::DefineNode(model)) => {
            ProtoResponse::DefineNode(proto::DefineNodeResponse {
                model: Some(proto_node_model(model)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Schema(grm_rs::SchemaResponse::DefineEdge(model)) => {
            ProtoResponse::DefineEdge(proto::DefineEdgeResponse {
                model: Some(proto_edge_model(model)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Schema(grm_rs::SchemaResponse::List(schema)) => {
            ProtoResponse::SchemaList(proto::SchemaListResponse {
                node_models: schema
                    .node_models
                    .into_iter()
                    .map(proto_node_model)
                    .collect::<grm_rs::Result<Vec<_>>>()?,
                edge_models: schema
                    .edge_models
                    .into_iter()
                    .map(proto_edge_model)
                    .collect::<grm_rs::Result<Vec<_>>>()?,
                backend_id_type: proto_id_type(schema.backend_id_type)?,
            })
        }
        grm_rs::RuntimeResponse::Node(grm_rs::NodeResponse::Create(node)) => {
            ProtoResponse::CreateNode(proto::NodeCreateResponse {
                node: Some(proto_stored_node(node)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Node(grm_rs::NodeResponse::Update(node)) => {
            ProtoResponse::UpdateNode(proto::NodeUpdateResponse {
                node: Some(proto_stored_node(node)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Node(grm_rs::NodeResponse::Delete(deleted)) => {
            ProtoResponse::DeleteNode(proto::NodeDeleteResponse {
                deleted: Some(proto_delete_result(deleted)),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Node(grm_rs::NodeResponse::Find(found)) => {
            ProtoResponse::FindNodes(proto::NodeFindResponse {
                model: found.model,
                nodes: found
                    .nodes
                    .into_iter()
                    .map(proto_stored_node)
                    .collect::<grm_rs::Result<Vec<_>>>()?,
            })
        }
        grm_rs::RuntimeResponse::Edge(grm_rs::EdgeResponse::Create(edge)) => {
            ProtoResponse::CreateEdge(proto::EdgeCreateResponse {
                edge: Some(proto_stored_edge(edge)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Edge(grm_rs::EdgeResponse::Update(edge)) => {
            ProtoResponse::UpdateEdge(proto::EdgeUpdateResponse {
                edge: Some(proto_stored_edge(edge)?),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Edge(grm_rs::EdgeResponse::Delete(deleted)) => {
            ProtoResponse::DeleteEdge(proto::EdgeDeleteResponse {
                deleted: Some(proto_delete_result(deleted)),
                durability: Some(proto_durable_mutation_outcome(durable_ops)?),
            })
        }
        grm_rs::RuntimeResponse::Edge(grm_rs::EdgeResponse::Find(found)) => {
            ProtoResponse::FindEdges(proto::EdgeFindResponse {
                model: found.model,
                edges: found
                    .edges
                    .into_iter()
                    .map(proto_stored_edge)
                    .collect::<grm_rs::Result<Vec<_>>>()?,
            })
        }
        grm_rs::RuntimeResponse::Batch(batch) => {
            ProtoResponse::ApplyBatch(proto_batch_response(batch, durable_ops)?)
        }
    };

    Ok(proto::RuntimeResponse {
        response: Some(response),
    })
}

fn proto_node_model(model: grm_rs::RuntimeNodeModel) -> grm_rs::Result<proto::NodeModel> {
    Ok(proto::NodeModel {
        name: model.name,
        label: model.label,
        id_field_name: model.id_field_name,
        id_type: proto_id_type(model.id_type)?,
        fields: model
            .fields
            .into_iter()
            .map(proto_field_spec)
            .collect::<Vec<_>>(),
    })
}

fn proto_edge_model(model: grm_rs::RuntimeRelModel) -> grm_rs::Result<proto::EdgeModel> {
    Ok(proto::EdgeModel {
        name: model.name,
        rel_type: model.rel_type,
        from_model: model.from_model,
        to_model: model.to_model,
        id_field_name: model.id_field_name,
        id_type: proto_id_type(model.id_type)?,
        fields: model
            .fields
            .into_iter()
            .map(proto_field_spec)
            .collect::<Vec<_>>(),
    })
}

fn proto_field_spec(field: grm_rs::RuntimeField) -> proto::FieldSpec {
    proto::FieldSpec {
        name: field.name,
        value_type: proto_field_value_type_from_runtime(field.value_type),
        required: field.required,
    }
}

fn proto_stored_node(node: grm_rs::StoredNode) -> grm_rs::Result<proto::StoredNode> {
    Ok(proto::StoredNode {
        id: node.id,
        labels: node.labels,
        props: Some(proto_property_map(node.props)?),
    })
}

fn proto_stored_edge(edge: grm_rs::StoredRel) -> grm_rs::Result<proto::StoredEdge> {
    Ok(proto::StoredEdge {
        id: edge.id,
        rel_type: edge.rel_type,
        from: edge.from,
        to: edge.to,
        props: Some(proto_property_map(edge.props)?),
    })
}

fn proto_property_map(
    props: std::collections::BTreeMap<String, Value>,
) -> grm_rs::Result<proto::PropertyMap> {
    Ok(proto::PropertyMap {
        properties: props
            .into_iter()
            .map(|(name, value)| {
                Ok(proto::Property {
                    name,
                    value: Some(proto_property_value(value)?),
                })
            })
            .collect::<grm_rs::Result<Vec<_>>>()?,
    })
}

fn proto_property_value(value: Value) -> grm_rs::Result<proto::PropertyValue> {
    use proto::property_value::Kind;

    let kind = match value {
        Value::String(value) => Kind::StringValue(value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Kind::IntValue(value)
            } else if let Some(value) = value.as_f64() {
                Kind::FloatValue(value)
            } else {
                return Err(grm_rs::GrmError::Constraint(
                    "numeric property value cannot be represented in service proto".into(),
                ));
            }
        }
        Value::Bool(value) => Kind::BoolValue(value),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            return Err(grm_rs::GrmError::Constraint(
                "property value cannot be represented in service proto".into(),
            ));
        }
    };

    Ok(proto::PropertyValue { kind: Some(kind) })
}

fn proto_delete_result(deleted: grm_rs::RuntimeDelete) -> proto::DeleteResult {
    proto::DeleteResult {
        model: deleted.model,
        id: deleted.id,
    }
}

fn proto_durable_mutation_outcome(
    durable_ops: &[grm_rs::DurableOperation],
) -> grm_rs::Result<proto::DurableMutationOutcome> {
    Ok(proto::DurableMutationOutcome {
        durable_ops: durable_ops
            .iter()
            .cloned()
            .map(proto_durable_operation)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        durable_op_count: durable_ops.len().try_into().map_err(|_| {
            grm_rs::GrmError::Constraint("durable operation count is too large".into())
        })?,
        has_durable_mutation: !durable_ops.is_empty(),
    })
}

fn proto_durable_operation(
    op: grm_rs::DurableOperation,
) -> grm_rs::Result<proto::DurableOperation> {
    use proto::durable_operation::Operation;

    let operation = match op {
        grm_rs::DurableOperation::RegisterNodeModel { model } => {
            Operation::RegisterNodeModel(proto_node_model(model)?)
        }
        grm_rs::DurableOperation::RegisterRelModel { model } => {
            Operation::RegisterEdgeModel(proto_edge_model(model)?)
        }
        grm_rs::DurableOperation::UpsertNode { node } => {
            Operation::UpsertNode(proto_stored_node(node)?)
        }
        grm_rs::DurableOperation::DeleteNode { id } => Operation::DeleteNodeId(id),
        grm_rs::DurableOperation::UpsertRel { rel } => {
            Operation::UpsertEdge(proto_stored_edge(rel)?)
        }
        grm_rs::DurableOperation::DeleteRel { id } => Operation::DeleteEdgeId(id),
        grm_rs::DurableOperation::Batch { ops } => Operation::Batch(proto::DurableOperationBatch {
            ops: ops
                .into_iter()
                .map(proto_durable_operation)
                .collect::<grm_rs::Result<Vec<_>>>()?,
        }),
    };

    Ok(proto::DurableOperation {
        operation: Some(operation),
    })
}

fn proto_batch_response(
    batch: grm_rs::RuntimeBatchResponse,
    durable_ops: &[grm_rs::DurableOperation],
) -> grm_rs::Result<proto::BatchResponse> {
    Ok(proto::BatchResponse {
        applied: json_bool(&batch.value, "applied"),
        atomic: json_bool(&batch.value, "atomic"),
        operation_count: json_u32(&batch.value, "operation_count")?,
        counts: proto_batch_counts(&batch.value)?,
        errors: json_array(&batch.value, "errors")
            .iter()
            .map(proto_batch_error)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        ids: json_array(&batch.value, "ids")
            .iter()
            .map(proto_batch_applied_id)
            .collect::<grm_rs::Result<Vec<_>>>()?,
        durability: Some(proto_durable_mutation_outcome(durable_ops)?),
    })
}

fn proto_batch_counts(value: &Value) -> grm_rs::Result<Vec<proto::BatchCount>> {
    let Some(counts) = value.get("counts") else {
        return Ok(Vec::new());
    };
    let Some(counts) = counts.as_object() else {
        return Err(grm_rs::GrmError::Constraint(
            "batch counts must be an object keyed by operation and model".into(),
        ));
    };

    let mut proto_counts = Vec::new();
    for (op, models) in counts {
        let Some(models) = models.as_object() else {
            return Err(grm_rs::GrmError::Constraint(format!(
                "batch counts for operation '{op}' must be an object keyed by model"
            )));
        };
        for (model, count) in models {
            let count = count.as_u64().ok_or_else(|| {
                grm_rs::GrmError::Constraint(format!(
                    "batch count for {op}/{model} must be an unsigned integer"
                ))
            })?;
            proto_counts.push(proto::BatchCount {
                op: op.clone(),
                model: model.clone(),
                count: count.try_into().map_err(|_| {
                    grm_rs::GrmError::Constraint(format!(
                        "batch count for {op}/{model} is too large"
                    ))
                })?,
            });
        }
    }
    Ok(proto_counts)
}

fn proto_batch_error(value: &Value) -> grm_rs::Result<proto::BatchError> {
    Ok(proto::BatchError {
        index: json_u32(value, "index")?,
        message: json_string(value, "message"),
        recovery_hint: value
            .get("recovery_hint")
            .or_else(|| value.get("recovery"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn proto_batch_applied_id(value: &Value) -> grm_rs::Result<proto::BatchAppliedId> {
    Ok(proto::BatchAppliedId {
        op: json_string(value, "op"),
        model: json_string(value, "model"),
        id: value.get("id").and_then(Value::as_i64).unwrap_or_default(),
        local_ref: value
            .get("ref")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn json_array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn json_bool(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .and_then(Value::as_bool)
        .unwrap_or_default()
}

fn json_string(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn json_u32(value: &Value, field: &str) -> grm_rs::Result<u32> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .unwrap_or_default()
        .try_into()
        .map_err(|_| grm_rs::GrmError::Constraint(format!("{field} is too large")))
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

fn proto_workspace_create_mode(value: i32) -> grm_rs::Result<WorkspaceCreateMode> {
    match proto::WorkspaceCreateMode::try_from(value)
        .map_err(|_| unknown_proto_enum("WorkspaceCreateMode", value))?
    {
        proto::WorkspaceCreateMode::InMemory => Ok(WorkspaceCreateMode::InMemory),
        proto::WorkspaceCreateMode::LocalAutocommit => Ok(WorkspaceCreateMode::LocalAutocommit),
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

fn proto_field_value_type_from_runtime(value_type: grm_rs::RuntimeValueType) -> i32 {
    match value_type {
        grm_rs::RuntimeValueType::String => proto::FieldValueType::String as i32,
        grm_rs::RuntimeValueType::Int => proto::FieldValueType::Int as i32,
        grm_rs::RuntimeValueType::Float => proto::FieldValueType::Float as i32,
        grm_rs::RuntimeValueType::Bool => proto::FieldValueType::Bool as i32,
    }
}

fn proto_id_type(id_type: grm_rs::BackendIdType) -> grm_rs::Result<i32> {
    match id_type {
        grm_rs::BackendIdType::Int64 => Ok(proto::IdType::Int64 as i32),
        grm_rs::BackendIdType::Uuid => Err(grm_rs::GrmError::NotSupported(
            "service proto id type mapping for UUID ids",
        )),
    }
}
