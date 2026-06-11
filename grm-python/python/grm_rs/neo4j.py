import asyncio
from functools import partial
from typing import List, Literal, Optional, Sequence

from ._grm_rs import Neo4jSession
from .typing import (
    BatchOperation,
    BatchResponseMode,
    BatchResult,
    Edge,
    FieldDefinition,
    FilterMap,
    JsonObject,
    LinkModelDescription,
    Node,
    NodeModelDescription,
    PropertyMap,
)


class AsyncNeo4jSession:
    """Async convenience wrapper around the first Neo4j Python session surface."""

    def __init__(self, session: Neo4jSession) -> None:
        self._session = session

    @classmethod
    async def connect(
        cls, *, uri: str, user: str, password: str
    ) -> "AsyncNeo4jSession":
        session = await asyncio.to_thread(
            Neo4jSession,
            uri=uri,
            user=user,
            password=password,
        )
        return cls(session)

    async def model_create(
        self, name: str, id_field: str, fields: Sequence[FieldDefinition]
    ) -> None:
        return await asyncio.to_thread(
            self._session.model_create,
            name,
            id_field,
            fields,
        )

    async def execute_query(
        self, query_text: str, params: Optional[JsonObject] = None
    ) -> int:
        return await asyncio.to_thread(
            partial(self._session.execute_query, query_text, params),
        )

    async def model_list(self) -> List[NodeModelDescription]:
        return await asyncio.to_thread(self._session.model_list)

    async def link_list(self) -> List[LinkModelDescription]:
        return await asyncio.to_thread(self._session.link_list)

    async def link_create(
        self,
        name: str,
        from_model: str,
        to_model: str,
        id_field: str,
        fields: Sequence[FieldDefinition],
    ) -> None:
        return await asyncio.to_thread(
            self._session.link_create,
            name,
            from_model,
            to_model,
            id_field,
            fields,
        )

    async def node_create(
        self, model_name: str, values: Optional[PropertyMap] = None
    ) -> Node:
        return await asyncio.to_thread(
            partial(self._session.node_create, model_name, values),
        )

    async def node_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Node]:
        return await asyncio.to_thread(
            partial(self._session.node_find, model_name, filters),
        )

    async def edge_create(
        self,
        model_name: str,
        from_id: int,
        to_id: int,
        values: Optional[PropertyMap] = None,
    ) -> Edge:
        return await asyncio.to_thread(
            partial(self._session.edge_create, model_name, from_id, to_id, values),
        )

    async def node_update(
        self, model_name: str, node_id: int, values: Optional[PropertyMap] = None
    ) -> Node:
        return await asyncio.to_thread(
            partial(self._session.node_update, model_name, node_id, values),
        )

    async def node_delete(self, model_name: str, node_id: int) -> None:
        return await asyncio.to_thread(self._session.node_delete, model_name, node_id)

    async def edge_find(
        self, model_name: str, filters: Optional[FilterMap] = None
    ) -> List[Edge]:
        return await asyncio.to_thread(
            partial(self._session.edge_find, model_name, filters),
        )

    async def edge_update(
        self, model_name: str, edge_id: int, values: Optional[PropertyMap] = None
    ) -> Edge:
        return await asyncio.to_thread(
            partial(self._session.edge_update, model_name, edge_id, values),
        )

    async def edge_delete(self, model_name: str, edge_id: int) -> None:
        return await asyncio.to_thread(self._session.edge_delete, model_name, edge_id)

    async def batch(
        self,
        ops: Sequence[BatchOperation],
        *,
        response: BatchResponseMode = "summary",
        allow_deletes: bool = False,
    ) -> BatchResult:
        return await asyncio.to_thread(
            partial(
                self._session.batch,
                ops,
                atomic=True,
                response=response,
                allow_deletes=allow_deletes,
            )
        )

    def node_id_type(self) -> Literal["int"]:
        return self._session.node_id_type()

    def rel_id_type(self) -> Literal["int"]:
        return self._session.rel_id_type()

    def capabilities(self) -> List[str]:
        return self._session.capabilities()
