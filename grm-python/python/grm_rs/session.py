"""Pythonic typed-object adapters over the native GRM extension classes."""

from typing import Any, List, Literal, Optional, Sequence, Type, Union, cast, overload

from ._grm_rs import Neo4jSession as _NativeNeo4jSession
from ._grm_rs import ServiceSession as _NativeServiceSession
from ._grm_rs import Session as _NativeSession
from .models import edge_create_args, link_model_args, node_create_args, node_model_args
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


class _SyncGraphAdapter:
    _native: Any

    def __getattr__(self, name: str) -> Any:
        return getattr(self._native, name)

    def node_id_type(self) -> IdType:
        return cast(IdType, self._native.node_id_type())

    def rel_id_type(self) -> IdType:
        return cast(IdType, self._native.rel_id_type())

    def capabilities(self) -> List[str]:
        return cast(List[str], self._native.capabilities())

    @overload
    def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None: ...

    @overload
    def model_create(self, name: Type[Any], id_field: Optional[str] = None) -> None: ...

    def model_create(
        self,
        name: Union[str, Type[Any]],
        id_field: Optional[str] = None,
        fields: Optional[Sequence[FieldDefinition]] = None,
    ) -> None:
        if isinstance(name, str):
            if id_field is None or fields is None:
                raise TypeError(
                    "model_create(name, id_field, fields) requires all explicit arguments"
                )
            self._native.model_create(name, id_field, fields)
            return None
        if fields is not None:
            raise TypeError(
                "model_create(PythonClass, ...) does not accept an explicit fields argument"
            )
        model_name, resolved_id_field, derived_fields = node_model_args(name, id_field)
        self._native.model_create(model_name, resolved_id_field, derived_fields)
        return None

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

    def link_create(
        self,
        name: Union[str, Type[Any]],
        from_model: Optional[str] = None,
        to_model: Optional[str] = None,
        id_field: Optional[str] = None,
        fields: Optional[Sequence[FieldDefinition]] = None,
    ) -> None:
        if isinstance(name, str):
            if (
                from_model is None
                or to_model is None
                or id_field is None
                or fields is None
            ):
                raise TypeError(
                    "link_create(name, from_model, to_model, id_field, fields) "
                    "requires all explicit arguments"
                )
            self._native.link_create(name, from_model, to_model, id_field, fields)
            return None
        if any(value is not None for value in (from_model, to_model, id_field, fields)):
            raise TypeError(
                "link_create(PythonClass) does not accept explicit link arguments"
            )
        link_name, resolved_from, resolved_to, resolved_id, derived_fields = (
            link_model_args(name)
        )
        self._native.link_create(
            link_name,
            resolved_from,
            resolved_to,
            resolved_id,
            derived_fields,
        )
        return None

    @overload
    def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node: ...

    @overload
    def node_create(self, model_name: object) -> Node: ...

    def node_create(
        self, model_name: Union[str, object], values: Optional[PropertyMap] = None
    ) -> Node:
        if isinstance(model_name, str):
            return cast(Node, self._native.node_create(model_name, values))
        if values is not None:
            raise TypeError(
                "node_create(instance) does not accept an explicit values argument"
            )
        resolved_model, props = node_create_args(model_name)
        return cast(Node, self._native.node_create(resolved_model, props))

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

    def edge_create(
        self,
        model_name: Union[str, object],
        from_id: Optional[GraphId] = None,
        to_id: Optional[GraphId] = None,
        values: Optional[PropertyMap] = None,
    ) -> Edge:
        if isinstance(model_name, str):
            if from_id is None or to_id is None:
                raise TypeError(
                    "edge_create(model_name, from_id, to_id, values=None) "
                    "requires endpoint ids"
                )
            return cast(
                Edge,
                self._native.edge_create(model_name, from_id, to_id, values),
            )
        if from_id is not None or to_id is not None or values is not None:
            raise TypeError(
                "edge_create(instance) does not accept explicit endpoint or value arguments"
            )
        resolved_model, resolved_from, resolved_to, props = edge_create_args(model_name)
        return cast(
            Edge,
            self._native.edge_create(resolved_model, resolved_from, resolved_to, props),
        )

    def model_show(self, name: str) -> Optional[NodeModelDescription]:
        for model in self.model_list():
            if model["name"] == name:
                return model
        return None

    def model_list(self) -> List[NodeModelDescription]:
        return cast(List[NodeModelDescription], self._native.model_list())

    def link_show(self, name: str) -> Optional[LinkModelDescription]:
        for link in self.link_list():
            if link["name"] == name:
                return link
        return None

    def link_list(self) -> List[LinkModelDescription]:
        return cast(List[LinkModelDescription], self._native.link_list())

    def node_update(
        self, model_name: str, node_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Node:
        return cast(Node, self._native.node_update(model_name, node_id, values))

    def node_delete(self, model_name: str, node_id: GraphId) -> None:
        self._native.node_delete(model_name, node_id)
        return None

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
    ) -> List[GraphEntity]:
        return cast(
            List[GraphEntity],
            self._native.node_find(
                model_name,
                filters,
                via=via,
                end_filters=end_filters,
                edge_filters=edge_filters,
                return_=return_,
                order=order,
                limit=limit,
                offset=offset,
            ),
        )

    def edge_update(
        self, model_name: str, edge_id: GraphId, values: Optional[PropertyMap] = None
    ) -> Edge:
        return cast(Edge, self._native.edge_update(model_name, edge_id, values))

    def edge_delete(self, model_name: str, edge_id: GraphId) -> None:
        self._native.edge_delete(model_name, edge_id)
        return None

    def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]:
        return cast(List[Edge], self._native.edge_find(model_name, filters))

    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult:
        return cast(
            BatchResult,
            self._native.batch(ops, response=response, allow_deletes=allow_deletes),
        )


class Session(_SyncGraphAdapter):
    def __init__(
        self,
        *,
        autocommit: bool = False,
        autocommit_path: Optional[str] = None,
        autocommit_format: WorkspaceFormat = "json",
    ) -> None:
        self._native = _NativeSession(
            autocommit=autocommit,
            autocommit_path=autocommit_path,
            autocommit_format=autocommit_format,
        )

    @property
    def autocommit(self) -> bool:
        return cast(bool, self._native.autocommit)

    @property
    def autocommit_path(self) -> Optional[str]:
        return cast(Optional[str], self._native.autocommit_path)

    @property
    def autocommit_format(self) -> Optional[WorkspaceFormat]:
        return cast(Optional[WorkspaceFormat], self._native.autocommit_format)

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
    ) -> ExplainResult:
        return cast(
            ExplainResult,
            self._native.explain_node_find(
                model_name,
                filters,
                via=via,
                end_filters=end_filters,
                edge_filters=edge_filters,
                return_=return_,
                order=order,
                limit=limit,
                offset=offset,
            ),
        )

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
    ) -> ProfileResult:
        return cast(
            ProfileResult,
            self._native.profile_node_find(
                model_name,
                filters,
                via=via,
                end_filters=end_filters,
                edge_filters=edge_filters,
                return_=return_,
                order=order,
                limit=limit,
                offset=offset,
            ),
        )

    def explain_edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> ExplainResult:
        return cast(ExplainResult, self._native.explain_edge_find(model_name, filters))

    def profile_edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> ProfileResult:
        return cast(ProfileResult, self._native.profile_edge_find(model_name, filters))

    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        atomic: bool = True,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult:
        return cast(
            BatchResult,
            self._native.batch(
                ops,
                atomic=atomic,
                response=response,
                allow_deletes=allow_deletes,
            ),
        )


class ServiceSession(_SyncGraphAdapter):
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
    ) -> None:
        self._native = _NativeServiceSession(
            endpoint=endpoint,
            workspace_ref=workspace_ref,
            mode=mode,
            workspace_format=workspace_format,
            tls_ca_cert=tls_ca_cert,
            tls_domain_name=tls_domain_name,
            tls_client_cert=tls_client_cert,
            tls_client_key=tls_client_key,
        )

    def endpoint(self) -> str:
        return cast(str, self._native.endpoint())

    def workspace_ref(self) -> str:
        return cast(str, self._native.workspace_ref())

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
    ) -> ExplainResult:
        return cast(
            ExplainResult,
            self._native.explain_node_find(
                model_name,
                filters,
                via=via,
                end_filters=end_filters,
                edge_filters=edge_filters,
                return_=return_,
                order=order,
                limit=limit,
                offset=offset,
            ),
        )

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
    ) -> ProfileResult:
        return cast(
            ProfileResult,
            self._native.profile_node_find(
                model_name,
                filters,
                via=via,
                end_filters=end_filters,
                edge_filters=edge_filters,
                return_=return_,
                order=order,
                limit=limit,
                offset=offset,
            ),
        )

    def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        atomic: bool = True,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult:
        return cast(
            BatchResult,
            self._native.batch(
                ops,
                atomic=atomic,
                response=response,
                allow_deletes=allow_deletes,
            ),
        )


class Neo4jSession(_SyncGraphAdapter):
    def __init__(self, *, uri: str, user: str, password: str) -> None:
        self._native = _NativeNeo4jSession(uri=uri, user=user, password=password)

    def node_id_type(self) -> Literal["int"]:
        return cast(Literal["int"], self._native.node_id_type())

    def rel_id_type(self) -> Literal["int"]:
        return cast(Literal["int"], self._native.rel_id_type())

    def execute_query(
        self, query_text: str, params: Optional[JsonObject] = None
    ) -> int:
        return cast(int, self._native.execute_query(query_text, params))
