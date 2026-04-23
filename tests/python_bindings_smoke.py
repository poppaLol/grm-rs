from grm_rs import Session


def main() -> None:
    session = Session()

    assert session.node_id_type() == "int"
    assert session.rel_id_type() == "int"

    session.model_create(
        "User",
        "userId",
        [
            {"name": "name", "type": "string", "required": True},
            {"name": "age", "type": "int", "required": False},
        ],
    )
    session.model_create(
        "Post",
        "postId",
        [
            {"name": "title", "type": "string", "required": True},
        ],
    )
    session.link_create(
        "Authored",
        "User",
        "Post",
        "authoredId",
        [
            {"name": "year", "type": "int", "required": True},
        ],
    )

    assert session.model_show("User")["id_field"] == "userId"
    assert len(session.model_list()) == 2
    assert session.link_show("Authored")["from_model"] == "User"
    assert len(session.link_list()) == 1

    user = session.node_create("User", {"name": "Alice", "age": 42})
    post = session.node_create("Post", {"title": "Hello"})
    edge = session.edge_create("Authored", user["id"], post["id"], {"year": 2024})

    users = session.node_find("User", {"name": "Alice"})
    assert len(users) == 1
    assert users[0]["props"]["age"] == 42

    session.node_update("User", user["id"], {"age": 43})
    updated_users = session.node_find("User", {"age": 43})
    assert len(updated_users) == 1
    assert updated_users[0]["id"] == user["id"]

    edges = session.edge_find("Authored", {"from": user["id"]})
    assert len(edges) == 1
    assert edges[0]["id"] == edge["id"]

    session.edge_update("Authored", edge["id"], {"year": 2025})
    updated_edges = session.edge_find("Authored", {"year": 2025})
    assert len(updated_edges) == 1
    assert updated_edges[0]["id"] == edge["id"]

    session.edge_delete("Authored", edge["id"])
    assert session.edge_find("Authored") == []

    session.node_delete("Post", post["id"])
    session.node_delete("User", user["id"])
    assert session.node_find("User") == []


if __name__ == "__main__":
    main()
