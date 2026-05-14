import json
from pathlib import Path
from tempfile import TemporaryDirectory

from grm_rs import GrmError, Session


def main() -> None:
    with TemporaryDirectory() as tmpdir:
        autocommit_path = Path(tmpdir) / "session.json"
        session = Session(autocommit=True, autocommit_path=str(autocommit_path))

        assert session.autocommit is True
        assert session.autocommit_format == "json"
        assert session.autocommit_path == str(autocommit_path)
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
        session.link_create(
            "Knows",
            "User",
            "User",
            "knowsId",
            [
                {"name": "since", "type": "int", "required": True},
            ],
        )

        assert autocommit_path.exists()
        assert session.model_show("User")["id_field"] == "userId"
        assert len(session.model_list()) == 2
        assert session.link_show("Authored")["from_model"] == "User"
        assert len(session.link_list()) == 2

        user = session.node_create("User", {"name": "Alice", "age": 42})
        post = session.node_create("Post", {"title": "Hello"})
        edge = session.edge_create("Authored", user["id"], post["id"], {"year": 2024})
        bob = session.node_create("User", {"name": "Bob", "age": 37})
        carol = session.node_create("User", {"name": "Carol", "age": 36})
        knows_bob = session.edge_create("Knows", user["id"], bob["id"], {"since": 2020})
        knows_carol = session.edge_create("Knows", bob["id"], carol["id"], {"since": 2021})

        export_path = Path(tmpdir) / "interchange.json"
        session.export_json(str(export_path))
        exported = json.loads(export_path.read_text())
        assert exported["format"] == "grm.interchange"
        assert exported["version"] == 1
        assert len(exported["schema"]["nodes"]) == 2
        assert len(exported["data"]["nodes"]) == 4
        assert len(exported["data"]["edges"]) == 3

        exported_dict = session.export_dict()
        assert exported_dict["format"] == "grm.interchange"
        assert exported_dict["data"] == exported["data"]

        import_autocommit_path = Path(tmpdir) / "imported-session.json"
        imported = Session(
            autocommit=True,
            autocommit_path=str(import_autocommit_path),
        )
        imported.import_json(str(export_path))
        assert import_autocommit_path.exists()
        assert len(imported.model_list()) == 2
        assert imported.model_show("User")["id_field"] == "userId"
        assert imported.link_show("Authored")["from_model"] == "User"
        assert imported.node_find("User", {"name": "Alice"})[0]["props"]["age"] == 42
        assert imported.edge_find("Authored", {"from": user["id"]})[0]["props"]["year"] == 2024

        autocommit_reloaded = Session()
        autocommit_reloaded.load_json(str(import_autocommit_path))
        assert len(autocommit_reloaded.node_find("User", {"name": "Alice"})) == 1

        batch_autocommit_path = Path(tmpdir) / "batch-session.json"
        batch_session = Session(
            autocommit=True,
            autocommit_path=str(batch_autocommit_path),
        )
        batch_result = batch_session.batch(
            [
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "BatchUser",
                        "id_field": "userId",
                        "fields": [
                            {"name": "name", "type": "string", "required": True}
                        ],
                    },
                },
                {
                    "op": "schema_define_node",
                    "args": {
                        "name": "BatchPost",
                        "id_field": "postId",
                        "fields": [
                            {"name": "title", "type": "string", "required": True}
                        ],
                    },
                },
                {
                    "op": "schema_define_edge",
                    "args": {
                        "name": "BatchAuthored",
                        "from_model": "BatchUser",
                        "to_model": "BatchPost",
                        "id_field": "authoredId",
                        "fields": [
                            {"name": "year", "type": "int", "required": True}
                        ],
                    },
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchUser",
                        "props": {"name": "Alice"},
                        "ref": "alice",
                    },
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchPost",
                        "props": {"title": "Hello"},
                        "ref": "post",
                    },
                },
                {
                    "op": "edge_create",
                    "args": {
                        "model": "BatchAuthored",
                        "from": "alice",
                        "to": "post",
                        "props": {"year": 2026},
                    },
                },
            ],
            response="detailed",
        )
        assert batch_result["applied"] is True
        assert batch_result["counts"]["node_create"]["BatchUser"] == 1
        assert batch_result["counts"]["edge_create"]["BatchAuthored"] == 1
        assert len(batch_result["ids"]) == 3
        assert {"alice", "post"}.issubset(
            {item.get("ref") for item in batch_result["ids"]}
        )
        assert len(batch_session.edge_find("BatchAuthored", {"year": 2026})) == 1

        batch_reloaded = Session()
        batch_reloaded.load_json(str(batch_autocommit_path))
        assert len(batch_reloaded.node_find("BatchUser", {"name": "Alice"})) == 1

        failed_atomic = batch_session.batch(
            [
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchPost",
                        "props": {"title": "Atomic rollback"},
                    },
                },
                {"op": "node_create", "args": {"model": "BatchPost", "props": {}}},
            ]
        )
        assert failed_atomic["applied"] is False
        assert failed_atomic["errors"][0]["index"] == 1
        assert batch_session.node_find("BatchPost", {"title": "Atomic rollback"}) == []

        partial = batch_session.batch(
            [
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchPost",
                        "props": {"title": "Partial success"},
                    },
                },
                {"op": "node_create", "args": {"model": "BatchPost", "props": {}}},
            ],
            atomic=False,
        )
        assert partial["applied"] is False
        assert partial["counts"]["node_create"]["BatchPost"] == 1
        assert len(batch_session.node_find("BatchPost", {"title": "Partial success"})) == 1

        duplicate_ref = batch_session.batch(
            [
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchUser",
                        "props": {"name": "Duplicate one"},
                        "ref": "duplicate",
                    },
                },
                {
                    "op": "node_create",
                    "args": {
                        "model": "BatchUser",
                        "props": {"name": "Duplicate two"},
                        "ref": "duplicate",
                    },
                },
            ]
        )
        assert duplicate_ref["applied"] is False
        assert "duplicate batch ref" in duplicate_ref["errors"][0]["message"]
        assert batch_session.node_find("BatchUser", {"name": "Duplicate one"}) == []

        delete_target = batch_session.node_create("BatchPost", {"title": "Delete me"})
        rejected_delete = batch_session.batch(
            [
                {
                    "op": "node_delete",
                    "args": {"model": "BatchPost", "id": delete_target["id"]},
                }
            ]
        )
        assert rejected_delete["applied"] is False
        assert "requires allow_deletes=true" in rejected_delete["errors"][0]["message"]
        assert len(batch_session.node_find("BatchPost", {"id": delete_target["id"]})) == 1
        allowed_delete = batch_session.batch(
            [
                {
                    "op": "node_delete",
                    "args": {"model": "BatchPost", "id": delete_target["id"]},
                }
            ],
            allow_deletes=True,
        )
        assert allowed_delete["applied"] is True
        assert batch_session.node_find("BatchPost", {"id": delete_target["id"]}) == []

        non_empty = Session()
        non_empty.model_create(
            "User",
            "userId",
            [
                {"name": "name", "type": "string", "required": True},
            ],
        )
        try:
            non_empty.import_json(str(export_path))
            raise AssertionError("import_json should require an empty session")
        except GrmError as exc:
            assert "empty session" in str(exc)

        users = session.node_find("User", {"name": "Alice"})
        assert len(users) == 1
        assert users[0]["props"]["age"] == 42

        authored_posts = session.node_find(
            "User",
            {"name": "Alice"},
            via=[
                {"dir": "out", "link": "Authored", "model": "Post"},
            ],
        )
        assert len(authored_posts) == 1
        assert authored_posts[0]["id"] == post["id"]

        authored_edges = session.node_find(
            "User",
            {"name": "Alice"},
            via=[
                {"dir": "out", "link": "Authored", "model": "Post"},
            ],
            end_filters={"title": "Hello"},
            edge_filters={"year": 2024},
            return_="edge",
        )
        assert len(authored_edges) == 1
        assert authored_edges[0]["id"] == edge["id"]

        friends_of_friends = session.node_find(
            "User",
            {"name": "Alice"},
            via=[
                {"dir": "out", "link": "Knows", "model": "User"},
                {"dir": "out", "link": "Knows", "model": "User"},
            ],
        )
        assert len(friends_of_friends) == 1
        assert friends_of_friends[0]["id"] == carol["id"]

        explain = session.explain_node_find(
            "User",
            {"name": "Alice"},
            via=[
                {"dir": "out", "link": "Authored", "model": "Post"},
            ],
        )
        assert explain["command"] == "node.find"
        assert explain["target"] == "User"
        assert any("ExpandOut" in step for step in explain["plan"]["steps"])
        assert "Return Node" in explain["plan"]["text"]

        profile = session.profile_node_find("User", {"name": "Alice"})
        assert profile["command"] == "node.find"
        assert profile["target"] == "User"
        assert profile["result_rows"] == 1
        assert isinstance(profile["elapsed"]["micros"], int)
        assert isinstance(profile["elapsed"]["display"], str)
        assert profile["per_step_metrics"] is None

        edge_profile = session.profile_edge_find("Authored", {"from": user["id"]})
        assert edge_profile["command"] == "edge.find"
        assert edge_profile["result_rows"] == 1
        assert any("RelationshipEndpointSeek" in step for step in edge_profile["plan"]["steps"])

        try:
            session.explain_node_find("User", {"format": "jsonl"})
            raise AssertionError("explain_node_find should reject format terms")
        except GrmError as exc:
            assert "format= is not supported with session.explain or session.profile" in str(exc)

        try:
            session.node_find("User", via=[{"dir": "out", "model": "Post"}])
            raise AssertionError("node_find should reject incomplete traversal dicts")
        except TypeError as exc:
            assert "via entries require key 'link'" in str(exc)

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
        session.edge_delete("Knows", knows_carol["id"])
        session.edge_delete("Knows", knows_bob["id"])
        assert session.edge_find("Authored") == []

        session.node_delete("Post", post["id"])
        session.node_delete("User", carol["id"])
        session.node_delete("User", bob["id"])
        session.node_delete("User", user["id"])
        assert session.node_find("User") == []

        reloaded = Session()
        reloaded.load_json(str(autocommit_path))
        assert len(reloaded.model_list()) == 2
        assert reloaded.node_find("User") == []


if __name__ == "__main__":
    main()
