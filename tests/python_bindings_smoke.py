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

        assert autocommit_path.exists()
        assert session.model_show("User")["id_field"] == "userId"
        assert len(session.model_list()) == 2
        assert session.link_show("Authored")["from_model"] == "User"
        assert len(session.link_list()) == 1

        user = session.node_create("User", {"name": "Alice", "age": 42})
        post = session.node_create("Post", {"title": "Hello"})
        edge = session.edge_create("Authored", user["id"], post["id"], {"year": 2024})

        export_path = Path(tmpdir) / "interchange.json"
        session.export_json(str(export_path))
        exported = json.loads(export_path.read_text())
        assert exported["format"] == "grm.interchange"
        assert exported["version"] == 1
        assert len(exported["schema"]["nodes"]) == 2
        assert len(exported["data"]["nodes"]) == 2
        assert len(exported["data"]["edges"]) == 1

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

        reloaded = Session()
        reloaded.load_json(str(autocommit_path))
        assert len(reloaded.model_list()) == 2
        assert reloaded.node_find("User") == []


if __name__ == "__main__":
    main()
