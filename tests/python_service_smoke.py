import socket
import subprocess
import time
from pathlib import Path
from tempfile import TemporaryDirectory

from grm_rs import ServiceSession


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_for_service(endpoint: str, workspace_ref: str) -> None:
    deadline = time.monotonic() + 15
    last_error = None
    while time.monotonic() < deadline:
        try:
            ServiceSession(
                endpoint=endpoint,
                workspace_ref=workspace_ref,
                mode="create",
            )
            return
        except Exception as exc:  # noqa: BLE001 - smoke test reports last startup failure.
            last_error = exc
            time.sleep(0.25)
    raise RuntimeError(f"service did not become ready: {last_error}")


def main() -> None:
    with TemporaryDirectory() as tmpdir:
        port = free_port()
        endpoint = f"http://127.0.0.1:{port}"
        root = Path(tmpdir) / "workspaces"
        root.mkdir()
        server = subprocess.Popen(
            [
                "cargo",
                "run",
                "-p",
                "grm-service-api",
                "--example",
                "local_workspace_server",
                "--",
                f"127.0.0.1:{port}",
                str(root),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            wait_for_service(endpoint, "python-service-ready")
            session = ServiceSession(
                endpoint=endpoint,
                workspace_ref="python-service-smoke",
                mode="create",
            )
            session.model_create(
                "User",
                "userId",
                [{"name": "name", "type": "string", "required": True}],
            )
            session.model_create(
                "Post",
                "postId",
                [{"name": "title", "type": "string", "required": True}],
            )
            session.link_create(
                "Authored",
                "User",
                "Post",
                "authoredId",
                [{"name": "year", "type": "int", "required": True}],
            )
            ada = session.node_create("User", {"name": "Ada"})
            post = session.node_create("Post", {"title": "Traversal"})
            session.edge_create("Authored", ada["id"], post["id"], {"year": 2026})
            assert session.node_find("User", {"id": ada["id"]})[0]["props"]["name"] == "Ada"
            traversed = session.node_find(
                "User",
                {"name": "Ada"},
                via=[{"dir": "out", "link": "Authored", "model": "Post"}],
                end_filters={"title": "Traversal"},
                edge_filters={"year": 2026},
                return_="end",
                order="title:asc",
                limit=1,
                offset=0,
            )
            assert len(traversed) == 1
            assert traversed[0]["id"] == post["id"]
            authored_edges = session.node_find(
                "User",
                {"name": "Ada"},
                via=[{"dir": "out", "link": "Authored", "model": "Post"}],
                return_="edge",
            )
            assert len(authored_edges) == 1
            assert authored_edges[0]["type"] == "Authored"
            assert authored_edges[0]["from"] == ada["id"]
            assert authored_edges[0]["to"] == post["id"]
            assert authored_edges[0]["props"]["year"] == 2026

            reopened = ServiceSession(
                endpoint=endpoint,
                workspace_ref="python-service-smoke",
                mode="open",
            )
            assert len(reopened.node_find("User", {"name": "Ada"})) == 1
            assert (root / "python-service-smoke.bin").exists()
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)


if __name__ == "__main__":
    main()
