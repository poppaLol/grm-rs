from grm_rs import AsyncNeo4jSession, GraphSession, Neo4jSession, WorkspaceGraphSession


def needs_workspace(session: WorkspaceGraphSession) -> None:
    session.model_list()


def rejects_string_workspace_ids(session: WorkspaceGraphSession) -> None:
    session.node_delete("User", "uuid-id")


def rejects_structured_graph_values(session: WorkspaceGraphSession) -> None:
    session.node_create("User", {"missing": None})
    session.node_create("User", {"tags": ["typed", "graph"]})
    session.node_find("User", {"metadata": {"active": True}})


def rejects_non_atomic_portable_batch(session: GraphSession) -> None:
    session.batch([], atomic=False)


neo4j = Neo4jSession(uri="bolt://localhost:7687", user="neo4j", password="password")
needs_workspace(neo4j)
neo4j.profile_node_find("User")


async def unsupported_async(session: AsyncNeo4jSession) -> None:
    await session.profile_node_find("User")
