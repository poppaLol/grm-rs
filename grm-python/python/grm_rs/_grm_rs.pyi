from typing import List, Literal, Optional, Sequence

from .typing import (
    BatchOperation,
    BatchResponseMode,
    BatchResult,
    Edge,
    ExplainResult,
    FieldDefinition,
    FilterMap,
    GraphEntity,
    GraphId,
    IdType,
    JsonObject,
    LinkModelDescription,
    Node,
    NodeModelDescription,
    ProfileResult,
    PropertyMap,
    ServiceWorkspaceMode,
    TraversalReturn,
    TraversalStep,
    WorkspaceFormat,
)


class GrmError(Exception): ...


class Session:
    def __init__(
        self,
        *,
        autocommit: bool = False,
        autocommit_path: Optional[str] = None,
        autocommit_format: WorkspaceFormat = "json",
    ) -> None: ...

    @property
    def autocommit(self) -> bool: ...

    @property
    def autocommit_path(self) -> Optional[str]: ...

    @property
    def autocommit_format(self) -> Optional[WorkspaceFormat]: ...

    def node_id_type(self) -> IdType: ...
    def rel_id_type(self) -> IdType: ...
    def capabilities(self) -> List[str]: ...
    def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...
    def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None: ...
    def model_show(self, name: str) -> Optional[NodeModelDescription]: ...
    def model_list(self) -> List[NodeModelDescription]: ...
    def link_show(self, name: str) -> Optional[LinkModelDescription]: ...
    def link_list(self) -> List[LinkModelDescription]: ...
    def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_delete(self, model_name: str, node_id: GraphId) -> None: ...
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
    def edge_create(
        self,
        model_name: str,
        from_id: GraphId,
        to_id: GraphId,
        values: Optional[PropertyMap] = None,
    ) -> Edge: ...
    def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge: ...
    def edge_delete(self, model_name: str, edge_id: GraphId) -> None: ...
    def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]: ...
    def explain_edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> ExplainResult: ...
    def profile_edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> ProfileResult: ...
    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        atomic: bool = True,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...
    def indexes(self) -> JsonObject: ...
    def save_json(self, path: str) -> None: ...
    def save_binary(self, path: str) -> None: ...
    def export_json(self, path: str) -> None: ...
    def export_dict(self) -> JsonObject: ...
    def load_json(self, path: str) -> None: ...
    def load_binary(self, path: str) -> None: ...
    def import_json(self, path: str) -> None: ...


class ServiceSession:
    def __init__(
        self,
        *,
        endpoint: str,
        workspace_ref: str,
        mode: ServiceWorkspaceMode = "open",
        workspace_format: WorkspaceFormat = "binary",
        tls_ca_cert: Optional[str] = None,
        tls_domain_name: Optional[str] = None,
        tls_client_cert: Optional[str] = None,
        tls_client_key: Optional[str] = None,
    ) -> None: ...
    def endpoint(self) -> str: ...
    def workspace_ref(self) -> str: ...
    def node_id_type(self) -> IdType: ...
    def rel_id_type(self) -> IdType: ...
    def capabilities(self) -> List[str]: ...
    def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...
    def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None: ...
    def model_list(self) -> List[NodeModelDescription]: ...
    def link_list(self) -> List[LinkModelDescription]: ...
    def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_delete(self, model_name: str, node_id: GraphId) -> None: ...
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
    def edge_create(
        self,
        model_name: str,
        from_id: GraphId,
        to_id: GraphId,
        values: Optional[PropertyMap] = None,
    ) -> Edge: ...
    def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge: ...
    def edge_delete(self, model_name: str, edge_id: GraphId) -> None: ...
    def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]: ...
    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        atomic: bool = True,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...


class Neo4jSession:
    def __init__(self, *, uri: str, user: str, password: str) -> None: ...
    def node_id_type(self) -> Literal["int"]: ...
    def rel_id_type(self) -> Literal["int"]: ...
    def capabilities(self) -> List[str]: ...
    def execute_query(
        self, query_text: str, params: Optional[JsonObject] = None
    ) -> int: ...
    def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...
    def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None: ...
    def model_list(self) -> List[NodeModelDescription]: ...
    def link_list(self) -> List[LinkModelDescription]: ...
    def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[GraphEntity]: ...
    def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node: ...
    def node_delete(self, model_name: str, node_id: GraphId) -> None: ...
    def edge_create(
        self,
        model_name: str,
        from_id: int,
        to_id: int,
        values: Optional[PropertyMap] = None,
    ) -> Edge: ...
    def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]: ...
    def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge: ...
    def edge_delete(self, model_name: str, edge_id: GraphId) -> None: ...
    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult: ...
