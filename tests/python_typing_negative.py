from grm_rs import AsyncNeo4jSession, Neo4jSession, WorkspaceGraphSession


def needs_workspace(session: WorkspaceGraphSession) -> None:
    session.model_list()


def rejects_string_workspace_ids(session: WorkspaceGraphSession) -> None:
    session.node_delete("User", "uuid-id")


def rejects_structured_graph_values(session: WorkspaceGraphSession) -> None:
    session.node_create("User", {"missing": None})
    session.node_create("User", {"tags": ["typed", "graph"]})
    session.node_find("User", {"metadata": {"active": True}})


neo4j = Neo4jSession(uri="bolt://localhost:7687", user="neo4j", password="password")
needs_workspace(neo4j)
neo4j.node_update("User", 1, {"name": "Ada"})


async def unsupported_async(session: AsyncNeo4jSession) -> None:
    await session.profile_node_find("User")
