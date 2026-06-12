import asyncio
import os
import time
from typing import Literal

from grm_rs import AsyncNeo4jSession, FieldDefinition


def require_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        raise RuntimeError(f"set {name} to run the Python Neo4j smoke test")
    return value


def field(
    name: str,
    value_type: Literal["string", "int", "float", "bool"] = "string",
    required: bool = True,
) -> FieldDefinition:
    return FieldDefinition(name=name, type=value_type, required=required)


async def define_schema(session: AsyncNeo4jSession) -> None:
    await session.model_create(
        "User",
        "userId",
        [
            field("name"),
            field("age", "int", required=False),
            field("smoke_id"),
        ],
    )
    await session.model_create(
        "Post",
        "postId",
        [
            field("title"),
            field("text", required=False),
            field("published", "bool", required=False),
            field("smoke_id"),
        ],
    )
    await session.link_create(
        "Authored",
        "User",
        "Post",
        "authoredId",
        [
            field("authoredOn"),
            field("smoke_id"),
        ],
    )
    await session.link_create(
        "Accessed",
        "User",
        "Post",
        "accessedId",
        [
            field("accessedOn"),
            field("smoke_id"),
        ],
    )
    await session.link_create(
        "Knows",
        "User",
        "User",
        "knowsId",
        [
            field("smoke_id"),
        ],
    )


async def persist_query_playground(session: AsyncNeo4jSession, smoke_id: str):
    alice_jones = await session.node_create(
        "User", {"name": "Alice Jones", "age": 42, "smoke_id": smoke_id}
    )
    bob_smith = await session.node_create(
        "User", {"name": "Bob Smith", "age": 35, "smoke_id": smoke_id}
    )
    eve_turner = await session.node_create(
        "User", {"name": "Eve Turner", "age": 29, "smoke_id": smoke_id}
    )
    alice = await session.node_create(
        "User", {"name": "Alice", "age": 31, "smoke_id": smoke_id}
    )

    hello_world = await session.node_create(
        "Post",
        {
            "title": "Hello World",
            "text": "A short welcome post about graphs.",
            "published": True,
            "smoke_id": smoke_id,
        },
    )
    draft_notes = await session.node_create(
        "Post",
        {
            "title": "Draft Notes",
            "text": "A quick draft about traversal ideas.",
            "published": False,
            "smoke_id": smoke_id,
        },
    )
    traversal_tips = await session.node_create(
        "Post",
        {
            "title": "Traversal Tips",
            "text": "Short notes on paths hops and filtering.",
            "published": True,
            "smoke_id": smoke_id,
        },
    )

    authored_1 = await session.edge_create(
        "Authored",
        alice_jones["id"],
        hello_world["id"],
        {"authoredOn": "2026-04-10", "smoke_id": smoke_id},
    )
    await session.edge_create(
        "Authored",
        bob_smith["id"],
        draft_notes["id"],
        {"authoredOn": "2026-04-12", "smoke_id": smoke_id},
    )
    await session.edge_create(
        "Authored",
        alice_jones["id"],
        traversal_tips["id"],
        {"authoredOn": "2026-04-15", "smoke_id": smoke_id},
    )

    await session.edge_create(
        "Accessed",
        alice_jones["id"],
        draft_notes["id"],
        {"accessedOn": "2026-04-20", "smoke_id": smoke_id},
    )
    await session.edge_create(
        "Accessed",
        bob_smith["id"],
        hello_world["id"],
        {"accessedOn": "2026-04-21", "smoke_id": smoke_id},
    )
    await session.edge_create(
        "Accessed",
        eve_turner["id"],
        traversal_tips["id"],
        {"accessedOn": "2026-04-22", "smoke_id": smoke_id},
    )

    await session.edge_create(
        "Knows",
        alice["id"],
        bob_smith["id"],
        {"smoke_id": smoke_id},
    )
    await session.edge_create(
        "Knows",
        bob_smith["id"],
        eve_turner["id"],
        {"smoke_id": smoke_id},
    )

    return {
        "alice_jones": alice_jones,
        "bob_smith": bob_smith,
        "eve_turner": eve_turner,
        "alice": alice,
        "hello_world": hello_world,
        "draft_notes": draft_notes,
        "traversal_tips": traversal_tips,
        "first_authored": authored_1,
    }


async def main() -> None:
    uri = require_env("NEO4J_URI")
    user = require_env("NEO4J_USER")
    password = require_env("NEO4J_PASSWORD")
    smoke_id = f"grm-python-smoke-{time.time_ns()}"
    print(f"python neo4j smoke_id={smoke_id}")

    writer = await AsyncNeo4jSession.connect(uri=uri, user=user, password=password)
    await define_schema(writer)
    created = await persist_query_playground(writer, smoke_id)
    print(
        "created query_playground graph "
        f"alice_jones={created['alice_jones']['id']} "
        f"hello_world={created['hello_world']['id']} "
        f"first_authored={created['first_authored']['id']}"
    )

    reader = await AsyncNeo4jSession.connect(uri=uri, user=user, password=password)
    await define_schema(reader)
    users = await reader.node_find("User", {"smoke_id": smoke_id})
    posts = await reader.node_find("Post", {"smoke_id": smoke_id})
    assert len(users) == 4, users
    assert len(posts) == 3, posts
    assert sorted(user["props"]["name"] for user in users) == [
        "Alice",
        "Alice Jones",
        "Bob Smith",
        "Eve Turner",
    ]
    assert sorted(post["props"]["title"] for post in posts) == [
        "Draft Notes",
        "Hello World",
        "Traversal Tips",
    ]
    print("verified persisted query_playground nodes from a fresh Python Neo4j session")
    print("inspect in Neo4j Browser with:")
    print(
        f"MATCH p=(n {{smoke_id: '{smoke_id}'}})-[r]-(m {{smoke_id: '{smoke_id}'}}) RETURN p"
    )
    print("cleanup with:")
    print(f"MATCH (n {{smoke_id: '{smoke_id}'}}) DETACH DELETE n")


if __name__ == "__main__":
    asyncio.run(main())
