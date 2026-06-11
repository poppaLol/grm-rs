from typing import Sequence

from grm_rs import (
    AsyncNeo4jGraphSession,
    AsyncNeo4jSession,
    BatchOperation,
    FieldDefinition,
    Neo4jGraphSession,
    Neo4jSession,
    ServiceSession,
    Session,
    WorkspaceGraphSession,
)


FIELDS: Sequence[FieldDefinition] = [
    {"name": "name", "type": "string", "required": True},
]


def use_workspace(session: WorkspaceGraphSession) -> int:
    session.model_create("User", "userId", FIELDS)
    node = session.node_create("User", {"name": "Ada"})
    session.node_find(
        "User",
        {"name": "Ada"},
        via=[{"dir": "out", "link": "*", "model": "User"}],
        return_="end",
    )
    operations: Sequence[BatchOperation] = [
        {
            "op": "node_create",
            "args": {"model": "User", "props": {"name": "Grace"}},
        }
    ]
    result = session.batch(operations)
    session.explain_node_find("User", {"id": node["id"]})
    session.profile_node_find("User", {"id": node["id"]})
    return result["operation_count"]


embedded = Session()
service = ServiceSession(
    endpoint="http://127.0.0.1:50051",
    workspace_ref="typing-only",
)
use_workspace(embedded)
use_workspace(service)

neo4j = Neo4jSession(uri="bolt://localhost:7687", user="neo4j", password="password")
sync_neo4j: Neo4jGraphSession = neo4j
sync_neo4j.execute_query(
    "RETURN $payload",
    {"payload": {"items": [1, None, "typed"]}},
)


async def use_async_neo4j(session: AsyncNeo4jGraphSession) -> int:
    await session.model_create("User", "userId", FIELDS)
    return await session.execute_query(
        "RETURN $payload",
        {"payload": {"items": [1, None, "typed"]}},
    )


async_neo4j: AsyncNeo4jGraphSession = AsyncNeo4jSession(neo4j)
