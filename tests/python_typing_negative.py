from grm_rs import AsyncNeo4jSession, GraphSession, Neo4jSession, WorkspaceGraphSession


def needs_workspace(session: WorkspaceGraphSession) -> None:
    session.model_list()


def rejects_string_workspace_ids(session: WorkspaceGraphSession) -> None:
    session.node_delete("User", "uuid-id")  # pyright: ignore[reportArgumentType]


def rejects_structured_graph_values(session: WorkspaceGraphSession) -> None:
    session.node_create("User", {"missing": None})  # pyright: ignore[reportArgumentType]
    session.node_create(
        "User",
        {"tags": ["typed", "graph"]},  # pyright: ignore[reportArgumentType]
    )
    session.node_find(
        "User",
        {"metadata": {"active": True}},  # pyright: ignore[reportArgumentType]
    )


def rejects_non_atomic_portable_batch(session: GraphSession) -> None:
    session.batch([], atomic=False)  # pyright: ignore[reportCallIssue]


neo4j = Neo4jSession(uri="bolt://localhost:7687", user="neo4j", password="password")
needs_workspace(neo4j)  # pyright: ignore[reportArgumentType]
neo4j.profile_node_find("User")  # pyright: ignore[reportAttributeAccessIssue]


async def unsupported_async(session: AsyncNeo4jSession) -> None:
    await session.profile_node_find(  # pyright: ignore[reportAttributeAccessIssue]
        "User"
    )
