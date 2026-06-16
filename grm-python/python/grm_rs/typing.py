"""Public type contracts for GRM Python sessions."""

from typing import (
    Any,
    Dict,
    List,
    Literal,
    Optional,
    Protocol,
    Sequence,
    Type,
    TypedDict,
    Union,
    overload,
    runtime_checkable,
)


GraphValue = Union[bool, int, float, str]
JsonScalar = Union[None, GraphValue]
JsonValue = Union[JsonScalar, List["JsonValue"], Dict[str, "JsonValue"]]
JsonObject = Dict[str, JsonValue]
GraphId = int
PropertyMap = Dict[str, GraphValue]
FilterMap = Dict[str, GraphValue]
FieldType = Literal["string", "int", "float", "bool"]
IdType = Literal["int", "uuid"]
TraversalDirection = Literal["out", "outgoing", "in", "incoming", "both"]
TraversalReturn = Literal["root", "end", "edge", "rel"]
BatchResponseMode = Literal["summary", "detailed"]
ServiceWorkspaceMode = Literal["create", "open"]
WorkspaceFormat = Literal["json", "binary", "bin"]


class FieldDefinition(TypedDict):
    name: str
    type: FieldType
    required: bool


class NodeModelDescription(TypedDict):
    name: str
    label: str
    id_field: str
    id_type: IdType
    fields: List[FieldDefinition]


class LinkModelDescription(TypedDict):
    name: str
    type: str
    from_model: str
    to_model: str
    id_field: str
    id_type: IdType
    fields: List[FieldDefinition]


class Node(TypedDict):
    id: GraphId
    labels: List[str]
    props: PropertyMap


Edge = TypedDict(
    "Edge",
    {
        "id": GraphId,
        "type": str,
        "from": GraphId,
        "to": GraphId,
        "props": PropertyMap,
    },
)


GraphEntity = Union[Node, Edge]


class TraversalStep(TypedDict):
    dir: TraversalDirection
    link: str
    model: str


class BatchDefineNodeArgs(TypedDict):
    name: str
    id_field: str
    fields: Sequence[FieldDefinition]


class BatchDefineEdgeArgs(TypedDict):
    name: str
    from_model: str
    to_model: str
    id_field: str
    fields: Sequence[FieldDefinition]


class _BatchNodeCreateRequired(TypedDict):
    model: str


class BatchNodeCreateArgs(_BatchNodeCreateRequired, total=False):
    props: PropertyMap
    ref: str


class _BatchNodeUpdateRequired(TypedDict):
    model: str
    id: int


class BatchNodeUpdateArgs(_BatchNodeUpdateRequired, total=False):
    props: PropertyMap


class BatchNodeDeleteArgs(TypedDict):
    model: str
    id: int


BatchEndpoint = Union[int, str]


BatchEdgeCreateArgsWithoutProps = TypedDict(
    "BatchEdgeCreateArgsWithoutProps",
    {
        "model": str,
        "from": BatchEndpoint,
        "to": BatchEndpoint,
    },
)
BatchEdgeCreateArgsWithProps = TypedDict(
    "BatchEdgeCreateArgsWithProps",
    {
        "model": str,
        "from": BatchEndpoint,
        "to": BatchEndpoint,
        "props": PropertyMap,
    },
)
BatchEdgeCreateArgs = Union[
    BatchEdgeCreateArgsWithoutProps, BatchEdgeCreateArgsWithProps
]


class _BatchEdgeUpdateRequired(TypedDict):
    model: str
    id: int


class BatchEdgeUpdateArgs(_BatchEdgeUpdateRequired, total=False):
    props: PropertyMap


class BatchEdgeDeleteArgs(TypedDict):
    model: str
    id: int


class BatchDefineNodeOperation(TypedDict):
    op: Literal["schema_define_node"]
    args: BatchDefineNodeArgs


class BatchDefineEdgeOperation(TypedDict):
    op: Literal["schema_define_edge"]
    args: BatchDefineEdgeArgs


class BatchNodeCreateOperation(TypedDict):
    op: Literal["node_create"]
    args: BatchNodeCreateArgs


class BatchNodeUpdateOperation(TypedDict):
    op: Literal["node_update"]
    args: BatchNodeUpdateArgs


class BatchNodeDeleteOperation(TypedDict):
    op: Literal["node_delete"]
    args: BatchNodeDeleteArgs


class BatchEdgeCreateOperation(TypedDict):
    op: Literal["edge_create"]
    args: BatchEdgeCreateArgs


class BatchEdgeUpdateOperation(TypedDict):
    op: Literal["edge_update"]
    args: BatchEdgeUpdateArgs


class BatchEdgeDeleteOperation(TypedDict):
    op: Literal["edge_delete"]
    args: BatchEdgeDeleteArgs


BatchOperation = Union[
    BatchDefineNodeOperation,
    BatchDefineEdgeOperation,
    BatchNodeCreateOperation,
    BatchNodeUpdateOperation,
    BatchNodeDeleteOperation,
    BatchEdgeCreateOperation,
    BatchEdgeUpdateOperation,
    BatchEdgeDeleteOperation,
]


class BatchError(TypedDict):
    index: int
    message: str
    recovery: str


class _BatchIdRequired(TypedDict):
    op: str
    model: str
    id: int


class BatchId(_BatchIdRequired, total=False):
    ref: str


class _BatchResultRequired(TypedDict):
    applied: bool
    atomic: bool
    operation_count: int
    counts: Dict[str, Dict[str, int]]
    errors: List[BatchError]


class BatchResult(_BatchResultRequired, total=False):
    ids: List[BatchId]


class _PlanDescriptionRequired(TypedDict):
    steps: List[str]
    text: str


class PlanDescription(_PlanDescriptionRequired, total=False):
    kind: str
    indexes: List[str]
    details: List[JsonObject]


class ExplainResult(TypedDict):
    command: str
    target: str
    plan: PlanDescription


class ElapsedTime(TypedDict):
    micros: int
    display: str


class ProfileStepMetric(TypedDict):
    step_index: int
    kind: str
    access_path: Optional[str]
    input_rows: Optional[int]
    output_rows: Optional[int]
    elapsed_micros: Optional[int]


class _ProfileResultRequired(TypedDict):
    command: str
    target: str
    plan: PlanDescription
    result_rows: int
    elapsed: ElapsedTime
    per_step_metrics: Optional[List[ProfileStepMetric]]


class ProfileResult(_ProfileResultRequired, total=False):
    phase_timings: JsonObject


class SchemaCapability(Protocol):
    def node_id_type(self) -> IdType: ...

    def rel_id_type(self) -> IdType: ...

    @overload
    def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...

    @overload
    def model_create(self, name: Type[Any], id_field: Optional[str] = None) -> None: ...

    @overload
    def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None: ...

    @overload
    def link_create(self, name: Type[Any]) -> None: ...


class SchemaInspectionCapability(Protocol):
    def model_list(self) -> List[NodeModelDescription]: ...

    def link_list(self) -> List[LinkModelDescription]: ...


class WorkspaceSchemaCapability(
    SchemaCapability, SchemaInspectionCapability, Protocol
):
    pass


class GraphCreateCapability(Protocol):
    @overload
    def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...

    @overload
    def node_create(self, model_name: object) -> Node: ...

    @overload
    def edge_create(
        self,
        model_name: str,
        from_id: GraphId,
        to_id: GraphId,
        values: Optional[PropertyMap] = None,
    ) -> Edge: ...

    @overload
    def edge_create(self, model_name: object) -> Edge: ...


class GraphCrudCapability(GraphCreateCapability, Protocol):
    def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node: ...

    def node_delete(self, model_name: str, node_id: GraphId) -> None: ...

    def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge: ...

    def edge_delete(self, model_name: str, edge_id: GraphId) -> None: ...

    def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]: ...


class GraphFindCapability(Protocol):
    def node_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[GraphEntity]: ...


class WorkspaceCrudCapability(GraphCrudCapability, Protocol):
    pass


class WorkspaceTraversalCapability(Protocol):
    def node_find(
        self,
        model_name: str,
        filters: Optional[FilterMap] = None,
        *,
        via: Optional[Sequence[TraversalStep]] = None,
        end_filters: Optional[FilterMap] = None,
        edge_filters: Optional[FilterMap] = None,
        return_: Optional[TraversalReturn] = None,
        order: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
    ) -> List[GraphEntity]: ...


class AtomicBatchCapability(Protocol):
    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...


class WorkspaceBatchCapability(AtomicBatchCapability, Protocol):
    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        atomic: bool = True,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...


class CapabilityDiscovery(Protocol):
    def capabilities(self) -> List[str]: ...


class Neo4jNativeQueryCapability(Protocol):
    def execute_query(
        self, query_text: str, params: Optional[JsonObject] = None
    ) -> int: ...


@runtime_checkable
class GraphSession(
    CapabilityDiscovery,
    SchemaCapability,
    SchemaInspectionCapability,
    GraphCrudCapability,
    GraphFindCapability,
    AtomicBatchCapability,
    Protocol,
):
    """Portable synchronous graph-session core shared by workspace and Neo4j."""

    def node_id_type(self) -> IdType: ...

    def rel_id_type(self) -> IdType: ...


class WorkspaceExplainProfileCapability(Protocol):
    def explain_node_find(
        self,
        model_name: str,
        filters: Optional[FilterMap] = None,
        *,
        via: Optional[Sequence[TraversalStep]] = None,
        end_filters: Optional[FilterMap] = None,
        edge_filters: Optional[FilterMap] = None,
        return_: Optional[TraversalReturn] = None,
        order: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
    ) -> ExplainResult: ...

    def profile_node_find(
        self,
        model_name: str,
        filters: Optional[FilterMap] = None,
        *,
        via: Optional[Sequence[TraversalStep]] = None,
        end_filters: Optional[FilterMap] = None,
        edge_filters: Optional[FilterMap] = None,
        return_: Optional[TraversalReturn] = None,
        order: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
    ) -> ProfileResult: ...


@runtime_checkable
class WorkspaceGraphSession(
    GraphSession,
    WorkspaceBatchCapability,
    WorkspaceTraversalCapability,
    WorkspaceExplainProfileCapability,
    Protocol,
):
    """Shared synchronous workspace contract implemented by Session and ServiceSession."""

    def node_find(
        self,
        model_name: str,
        filters: Optional[FilterMap] = None,
        *,
        via: Optional[Sequence[TraversalStep]] = None,
        end_filters: Optional[FilterMap] = None,
        edge_filters: Optional[FilterMap] = None,
        return_: Optional[TraversalReturn] = None,
        order: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
    ) -> List[GraphEntity]: ...


@runtime_checkable
class Neo4jGraphSession(GraphSession, Neo4jNativeQueryCapability, Protocol):
    """Portable graph core plus explicitly Neo4j-native query access."""


@runtime_checkable
class AsyncNeo4jGraphSession(Protocol):
    def capabilities(self) -> List[str]: ...

    @overload
    async def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...

    @overload
    async def model_create(
        self, name: Type[Any], id_field: Optional[str] = None
    ) -> None: ...

    async def model_list(self) -> List[NodeModelDescription]: ...

    async def link_list(self) -> List[LinkModelDescription]: ...

    @overload
    async def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None: ...

    @overload
    async def link_create(self, name: Type[Any]) -> None: ...

    async def execute_query(
        self, query_text: str, params: Optional[JsonObject] = None
    ) -> int: ...

    @overload
    async def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...

    @overload
    async def node_create(self, model_name: object) -> Node: ...

    async def node_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Node]: ...

    async def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node: ...

    async def node_delete(self, model_name: str, node_id: GraphId) -> None: ...

    @overload
    async def edge_create(
        self,
        model_name: str,
        from_id: int,
        to_id: int,
        values: Optional[PropertyMap] = None,
    ) -> Edge: ...

    @overload
    async def edge_create(self, model_name: object) -> Edge: ...

    async def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]: ...

    async def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge: ...

    async def edge_delete(self, model_name: str, edge_id: GraphId) -> None: ...

    async def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...

    def node_id_type(self) -> Literal["int"]: ...

    def rel_id_type(self) -> Literal["int"]: ...
