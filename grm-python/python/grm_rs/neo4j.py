import asyncio
from functools import partial

from ._grm_rs import Neo4jSession


class AsyncNeo4jSession:
    """Async convenience wrapper around the first Neo4j Python session surface."""

    def __init__(self, session):
        self._session = session

    @classmethod
    async def connect(cls, *, uri, user, password):
        session = await asyncio.to_thread(
            Neo4jSession,
            uri=uri,
            user=user,
            password=password,
        )
        return cls(session)

    async def model_create(self, name, id_field, fields):
        return await asyncio.to_thread(
            self._session.model_create,
            name,
            id_field,
            fields,
        )

    async def execute_query(self, query_text, params=None):
        return await asyncio.to_thread(
            partial(self._session.execute_query, query_text, params),
        )

    async def link_create(self, name, from_model, to_model, id_field, fields):
        return await asyncio.to_thread(
            self._session.link_create,
            name,
            from_model,
            to_model,
            id_field,
            fields,
        )

    async def node_create(self, model_name, values=None):
        return await asyncio.to_thread(
            partial(self._session.node_create, model_name, values),
        )

    async def node_find(self, model_name, filters=None):
        return await asyncio.to_thread(
            partial(self._session.node_find, model_name, filters),
        )

    async def edge_create(self, model_name, from_id, to_id, values=None):
        return await asyncio.to_thread(
            partial(self._session.edge_create, model_name, from_id, to_id, values),
        )

    def node_id_type(self):
        return self._session.node_id_type()

    def rel_id_type(self):
        return self._session.rel_id_type()
