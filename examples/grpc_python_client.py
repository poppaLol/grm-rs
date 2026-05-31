#!/usr/bin/env python3
"""Small Python-driven smoke client for the Docker gRPC demo service.

The repository does not yet ship generated Python protobuf bindings. This
example shells out to grpcurl with the checked-in proto files so the request
shape stays honest while a first-class Python gRPC client is still future work.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


WORKSPACE_ID = os.environ.get("WORKSPACE_ID", f"python-grpc-demo-{int(time.time())}")
REPO_ROOT = Path(__file__).resolve().parents[1]
PROTO_ROOT = os.environ.get("PROTO_ROOT", str(REPO_ROOT / "grm-service-api" / "proto"))
PROTO_FILE = os.environ.get("PROTO_FILE", "grm/service/v1/service.proto")
METHOD_PREFIX = "grm.service.v1.GrmService"
GRPCURL_IMAGE = os.environ.get("GRPCURL_IMAGE", "fullstorydev/grpcurl:latest")
GRPCURL_DOCKER_NETWORK = os.environ.get("GRPCURL_DOCKER_NETWORK", "grm-rs_default")
GRPCURL_MODE = os.environ.get("GRPCURL_MODE", "auto")


def resolve_grpcurl_mode() -> str:
    if GRPCURL_MODE != "auto":
        return GRPCURL_MODE
    native = shutil.which("grpcurl")
    if native is None:
        return "docker"
    probe = subprocess.run([native, "--version"], text=True, capture_output=True)
    return "native" if probe.returncode == 0 else "docker"


def grpcurl(mode: str, endpoint: str, method: str, payload: dict) -> dict:
    grpcurl_args = [
        "-plaintext",
        "-import-path",
        "/protos" if mode == "docker" else PROTO_ROOT,
        "-proto",
        PROTO_FILE,
        "-d",
        json.dumps(payload),
        endpoint,
        f"{METHOD_PREFIX}/{method}",
    ]
    if mode == "docker":
        command = [
            "docker",
            "run",
            "--rm",
            "--network",
            GRPCURL_DOCKER_NETWORK,
            "-v",
            f"{PROTO_ROOT}:/protos:ro",
            GRPCURL_IMAGE,
            *grpcurl_args,
        ]
    else:
        command = ["grpcurl", *grpcurl_args]
    try:
        completed = subprocess.run(command, check=True, text=True, capture_output=True)
    except subprocess.CalledProcessError as exc:
        if exc.stdout:
            print(exc.stdout, file=sys.stderr)
        if exc.stderr:
            print(exc.stderr, file=sys.stderr)
        raise
    return json.loads(completed.stdout or "{}")


def main() -> int:
    mode = resolve_grpcurl_mode()
    if mode not in {"native", "docker"}:
        print("GRPCURL_MODE must be auto, native, or docker.", file=sys.stderr)
        return 1
    if mode == "docker" and shutil.which("docker") is None:
        print("docker is required for GRPCURL_MODE=docker.", file=sys.stderr)
        return 1
    endpoint = os.environ.get(
        "GRPC_ENDPOINT", "grm-grpc:50051" if mode == "docker" else "localhost:50051"
    )
    if mode == "native" and shutil.which("grpcurl") is None:
        print("grpcurl is required for GRPCURL_MODE=native.", file=sys.stderr)
        return 1

    print("GRM gRPC Python smoke")
    print(f"endpoint:  {endpoint}")
    print(f"workspace: {WORKSPACE_ID}")
    print(f"grpcurl:   {mode}")

    created = grpcurl(
        mode,
        endpoint,
        "CreateWorkspace",
        {
            "mode": "WORKSPACE_CREATE_MODE_LOCAL_AUTOCOMMIT",
            "workspace": {"id": WORKSPACE_ID},
            "format": "DURABILITY_FORMAT_JSON",
        },
    )
    handle = created["handle"]["id"]
    print(f"created workspace handle: {handle}")

    grpcurl(
        mode,
        endpoint,
        "ExecuteWorkspace",
        {
            "handle": {"id": handle},
            "request": {
                "defineNode": {
                    "name": "User",
                    "idField": "userId",
                    "fields": [
                        {
                            "name": "name",
                            "valueType": "FIELD_VALUE_TYPE_STRING",
                            "required": True,
                        }
                    ],
                }
            },
        },
    )
    print("defined User model")

    node = grpcurl(
        mode,
        endpoint,
        "ExecuteWorkspace",
        {
            "handle": {"id": handle},
            "request": {
                "createNode": {
                    "model": "User",
                    "props": {
                        "properties": [
                            {"name": "name", "value": {"stringValue": "Ada"}}
                        ]
                    },
                }
            },
        },
    )
    node_id = node["response"]["createNode"]["node"]["id"]
    print(f"created User node: {node_id}")

    found = grpcurl(
        mode,
        endpoint,
        "ExecuteWorkspace",
        {
            "handle": {"id": handle},
            "request": {"findNodes": {"model": "User", "id": int(node_id)}},
        },
    )
    print(json.dumps(found, indent=2))

    grpcurl(mode, endpoint, "CloseWorkspace", {"handle": {"id": handle}})
    print("closed workspace")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
