import json
from importlib import resources
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, cast

import grm_rs
from grm_rs import GrmError, Session, WorkspaceGraphSession
from grm_rs.session import ServiceSession


class _ListOnlyNative:
    def model_list(self) -> list[dict[str, object]]:
        return [
            {
                "name": "ListOnly",
                "label": "ListOnly",
                "id_field": "listOnlyId",
                "id_type": "int",
                "fields": [],
            }
        ]

    def link_list(self) -> list[dict[str, object]]:
        return [
            {
                "name": "LIST_ONLY",
                "type": "LIST_ONLY",
                "from_model": "ListOnly",
                "to_model": "ListOnly",
                "id_field": "listOnlyEdgeId",
                "id_type": "int",
                "fields": [],
            }
        ]


class _FakePydanticField:
    def __init__(self, annotation: object, required: bool = True) -> None:
        self.annotation = annotation
        self._required = required

    def is_required(self) -> bool:
        return self._required


class StatementLine:
    __grm_id_field__ = "statementLineId"
    model_fields = {
        "date": _FakePydanticField(str),
        "amount": _FakePydanticField(float),
        "cleared": _FakePydanticField(bool, required=False),
    }

    def __init__(self, date: str, amount: float, cleared: bool = False) -> None:
        self.date = date
        self.amount = amount
        self.cleared = cleared
        self.dump_mode: object = None

    def model_dump(self, *, mode: str = "python") -> dict[str, object]:
        self.dump_mode = mode
        return {
            "date": self.date,
            "amount": self.amount,
            "cleared": self.cleared,
        }


class StatementTag:
    model_fields = {
        "name": _FakePydanticField(str),
    }


class UnsupportedStatementLine:
    __grm_id_field__ = "unsupportedLineId"
    model_fields = {
        "tags": _FakePydanticField(list[str]),
    }


class Authored:
    __grm_link_name__ = "TYPED_AUTHORED"
    __grm_from_model__ = "TypedUser"
    __grm_to_model__ = "TypedPost"
    __grm_id_field__ = "typedAuthoredId"
    __grm_from_id_field__ = "user_id"
    __grm_to_id_field__ = "post_id"
    year: int

    def __init__(self, user_id: int, post_id: int, year: int) -> None:
        self.user_id = user_id
        self.post_id = post_id
        self.year = year


def main() -> None:
    package_files = resources.files(grm_rs)
    assert package_files.joinpath("py.typed").is_file()
    assert package_files.joinpath("_grm_rs.pyi").is_file()

    with TemporaryDirectory() as tmpdir:
        autocommit_path = Path(tmpdir) / "session.json"
        session = Session(autocommit=True, autocommit_path=str(autocommit_path))
        assert isinstance(session, WorkspaceGraphSession)

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
        user_model = session.model_show("User")
        assert user_model is not None
        assert user_model["id_field"] == "userId"
        assert len(session.model_list()) == 2
        authored_link = session.link_show("Authored")
        assert authored_link is not None
        assert authored_link["from_model"] == "User"
        assert len(session.link_list()) == 2

        user = session.node_create("User", {"name": "Alice", "age": 42})
        post = session.node_create("Post", {"title": "Hello"})
        edge = session.edge_create("Authored", user["id"], post["id"], {"year": 2024})
        bob = session.node_create("User", {"name": "Bob", "age": 37})
        carol = session.node_create("User", {"name": "Carol", "age": 36})
        knows_bob = session.edge_create("Knows", user["id"], bob["id"], {"since": 2020})
        knows_carol = session.edge_create("Knows", bob["id"], carol["id"], {"since": 2021})
        ordered_users = session.node_find(
            "User", {"age>": 35, "order": "age:asc", "limit": 1}
        )
        assert [node["props"]["name"] for node in ordered_users] == ["Carol"]
        assert session.edge_find("Knows", {"from": user["id"]})[0]["id"] == knows_bob["id"]

        same_path_reopened = Session(
            autocommit=True,
            autocommit_path=str(autocommit_path),
        )
        assert len(same_path_reopened.node_find("User", {"name": "Alice"})) == 1

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
        imported_user_model = imported.model_show("User")
        assert imported_user_model is not None
        assert imported_user_model["id_field"] == "userId"
        imported_authored_link = imported.link_show("Authored")
        assert imported_authored_link is not None
        assert imported_authored_link["from_model"] == "User"
        assert imported.node_find("User", {"name": "Alice"})[0]["props"]["age"] == 42
        assert imported.edge_find("Authored", {"from": user["id"]})[0]["props"]["year"] == 2024

        typed_session = Session()
        typed_session.model_create(StatementLine, id_field="explicitLineId")
        statement_model = typed_session.model_show("StatementLine")
        assert statement_model is not None
        assert statement_model["id_field"] == "explicitLineId"
        assert statement_model["fields"] == [
            {"name": "date", "type": "string", "required": True},
            {"name": "amount", "type": "float", "required": True},
            {"name": "cleared", "type": "bool", "required": False},
        ]
        line = StatementLine("2026-06-16", 12.5, True)
        typed_line = typed_session.node_create(line)
        assert typed_line["props"] == line.model_dump()
        assert line.dump_mode == "python"

        list_only_service = ServiceSession.__new__(ServiceSession)
        cast(Any, list_only_service)._native = _ListOnlyNative()
        assert list_only_service.model_show("ListOnly") is not None
        assert list_only_service.model_show("Missing") is None
        assert list_only_service.link_show("LIST_ONLY") is not None
        assert list_only_service.link_show("MISSING") is None

        typed_session.model_create(StatementTag, id_field="tagId")
        explicit_tag_model = typed_session.model_show("StatementTag")
        assert explicit_tag_model is not None
        assert explicit_tag_model["id_field"] == "tagId"

        inferred_id_session = Session()
        inferred_id_session.model_create(StatementLine)
        inferred_statement_model = inferred_id_session.model_show("StatementLine")
        assert inferred_statement_model is not None
        assert inferred_statement_model["id_field"] == "statementLineId"

        try:
            inferred_id_session.model_create(StatementTag)
            raise AssertionError("typed model creation should require id field metadata")
        except TypeError as exc:
            assert "__grm_id_field__" in str(exc)

        try:
            typed_session.model_create(UnsupportedStatementLine)
            raise AssertionError("typed model creation should reject unsupported field types")
        except TypeError as exc:
            assert "UnsupportedStatementLine.tags" in str(exc)
            assert "unsupported field type" in str(exc)

        typed_session.model_create(
            "TypedUser",
            "typedUserId",
            [{"name": "name", "type": "string", "required": True}],
        )
        typed_session.model_create(
            "TypedPost",
            "typedPostId",
            [{"name": "title", "type": "string", "required": True}],
        )
        typed_session.link_create(Authored)
        typed_user = typed_session.node_create("TypedUser", {"name": "Ada"})
        typed_post = typed_session.node_create("TypedPost", {"title": "Typed hello"})
        authored = Authored(typed_user["id"], typed_post["id"], 2026)
        typed_edge = typed_session.edge_create(authored)
        assert typed_edge["type"] == "TYPED_AUTHORED"
        assert typed_edge["from"] == typed_user["id"]
        assert typed_edge["to"] == typed_post["id"]
        assert typed_edge["props"] == {"year": 2026}

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
        assert "ids" in batch_result
        batch_ids = batch_result["ids"]
        assert len(batch_ids) == 3
        assert {"alice", "post"}.issubset(
            {item.get("ref") for item in batch_ids}
        )
        assert len(batch_session.edge_find("BatchAuthored", {"year": 2026})) == 1
        assert '"Batch"' in Path(f"{batch_autocommit_path}.log").read_text()

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
        assert "details" in explain["plan"]
        plan_details = explain["plan"]["details"]
        assert any(
            step["kind"] == "ExpandOut"
            and step["access_path"] == "outgoing_adjacency"
            and step["index"] == "system.edge.outgoing_adjacency"
            for step in plan_details
        )
        assert "Return Node" in explain["plan"]["text"]

        indexes = session.indexes()
        index_entries = indexes.get("indexes")
        assert isinstance(index_entries, list)
        assert any(
            isinstance(index, dict) and index.get("name") == "system.node.property"
            for index in index_entries
        )

        profile = session.profile_node_find("User", {"name": "Alice"})
        assert profile["command"] == "node.find"
        assert profile["target"] == "User"
        assert profile["result_rows"] == 1
        assert isinstance(profile["elapsed"]["micros"], int)
        assert isinstance(profile["elapsed"]["display"], str)
        per_step_metrics = profile["per_step_metrics"]
        assert per_step_metrics is not None
        assert len(per_step_metrics) >= 2

        edge_profile = session.profile_edge_find("Authored", {"from": user["id"]})
        assert edge_profile["command"] == "edge.find"
        assert edge_profile["result_rows"] == 1
        assert any("RelationshipEndpointSeek" in step for step in edge_profile["plan"]["steps"])

        try:
            session.explain_node_find("User", {"format": "jsonl"})
            raise AssertionError("explain_node_find should reject unknown fields")
        except GrmError as exc:
            assert "unknown field 'format' for model 'User'" in str(exc)

        session.model_create(
            "Document",
            "documentId",
            [
                {"name": "format", "type": "string", "required": True},
            ],
        )
        session.node_create("Document", {"format": "jsonl"})
        assert len(session.node_find("Document", {"format": "jsonl"}, limit=1)) == 1

        try:
            incomplete_via = cast(Any, [{"dir": "out", "model": "Post"}])
            session.node_find("User", via=incomplete_via)
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
        assert len(reloaded.model_list()) == 3
        assert reloaded.node_find("User") == []


if __name__ == "__main__":
    main()
