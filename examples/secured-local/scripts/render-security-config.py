#!/usr/bin/env python3
"""Render secured-local service security.json from editable access templates."""

from __future__ import annotations

import argparse
import json
import re
import sys
import tempfile
from pathlib import Path

VALID_ACTIONS = {
    "workspace.create", "workspace.open", "workspace.close", "workspace.inspect",
    "workspace.save", "workspace.load", "workspace.export", "workspace.import",
    "schema.define", "schema.inspect", "node.create", "node.read", "node.update",
    "node.delete", "edge.create", "edge.read", "edge.update", "edge.delete",
    "query", "traverse", "explain", "profile", "batch.apply", "index.inspect",
}

VALID_RESOURCE_KINDS = {
    "service", "workspace", "node_model", "any_node_model", "edge_model",
    "any_edge_model", "operation_family", "index_catalog", "workspace_artifact",
    "reserved_admin",
}

VALID_SCOPE_KINDS = {"service", "workspace", "deployment_local_all_workspaces"}
FINGERPRINT_RE = re.compile(r"^[0-9a-fA-F]{64}$")
BOOTSTRAP_REQUIRED_PERMISSIONS = {
    ("service", "workspace.create", "service"),
    ("deployment_local_all_workspaces", "schema.define", "any_node_model"),
    ("deployment_local_all_workspaces", "schema.inspect", "workspace"),
    ("deployment_local_all_workspaces", "node.create", "any_node_model"),
    ("deployment_local_all_workspaces", "node.read", "any_node_model"),
}


def load_json(path: Path) -> object:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def find_template(access_levels: dict[str, object], access_level: str) -> dict[str, object]:
    templates = access_levels.get("templates")
    if not isinstance(templates, list):
        raise ValueError("access-levels.json must contain a templates array")
    for item in templates:
        if isinstance(item, dict) and item.get("id") == access_level:
            return item
    known = ", ".join(str(item.get("id")) for item in templates if isinstance(item, dict))
    raise ValueError(f"unknown access level '{access_level}' (known: {known})")


def validate_scope(scope: object) -> None:
    if not isinstance(scope, dict):
        raise ValueError("assignment scope must be an object")
    kind = scope.get("kind")
    if kind not in VALID_SCOPE_KINDS:
        raise ValueError(f"unknown scope kind '{kind}'")
    if kind == "workspace" and not scope.get("workspace"):
        raise ValueError("workspace scope requires a workspace value")


def validate_permission(permission: object) -> None:
    if not isinstance(permission, dict):
        raise ValueError("permission must be an object")
    action = permission.get("action")
    if action not in VALID_ACTIONS:
        raise ValueError(f"unknown permission action '{action}'")
    resource = permission.get("resource")
    if not isinstance(resource, dict):
        raise ValueError(f"permission '{action}' resource must be an object")
    kind = resource.get("kind")
    if kind not in VALID_RESOURCE_KINDS:
        raise ValueError(f"unknown resource kind '{kind}'")
    if kind in {"node_model", "edge_model"} and not resource.get("model"):
        raise ValueError(f"resource kind '{kind}' requires a model value")


def expand_assignments(
    access_template: dict[str, object], issuer: str, subject: str
) -> list[dict[str, object]]:
    principal = {"issuer": issuer, "subject": subject}
    expanded: list[dict[str, object]] = []
    raw_assignments = access_template.get("assignments", [])
    if not isinstance(raw_assignments, list):
        raise ValueError("access template assignments must be an array")
    for assignment in raw_assignments:
        if not isinstance(assignment, dict):
            raise ValueError("access template assignment must be an object")
        scope = assignment.get("scope")
        validate_scope(scope)
        permissions = assignment.get("permissions", [])
        if not isinstance(permissions, list):
            raise ValueError("assignment permissions must be an array")
        for permission in permissions:
            validate_permission(permission)
        expanded.append({"principal": principal, "scope": scope, "permissions": permissions})
    return expanded


def permission_key(assignment: dict[str, object], permission: dict[str, object]) -> tuple[str, str, str]:
    scope = assignment["scope"]
    resource = permission["resource"]
    if not isinstance(scope, dict) or not isinstance(resource, dict):
        raise ValueError("assignment scope and permission resource must be objects")
    return (str(scope["kind"]), str(permission["action"]), str(resource["kind"]))


def validate_bootstrap_capable(
    access_template: dict[str, object], assignments: list[dict[str, object]]
) -> None:
    if access_template.get("bootstrap_capable") is not True:
        template_id = access_template.get("id", "<unknown>")
        raise ValueError(
            f"access level '{template_id}' is not bootstrap-capable for Admin no.1"
        )
    granted = {
        permission_key(assignment, permission)
        for assignment in assignments
        for permission in assignment["permissions"]  # type: ignore[index]
    }
    missing = sorted(BOOTSTRAP_REQUIRED_PERMISSIONS - granted)
    if missing:
        formatted = ", ".join(
            f"{scope}:{action}:{resource}" for scope, action, resource in missing
        )
        raise ValueError(f"bootstrap access template is missing required permissions: {formatted}")


def render_template(
    template_path: Path,
    fingerprint: str,
    issuer: str,
    subject: str,
    policy_version: str,
    assignments: list[dict[str, object]],
) -> dict[str, object]:
    def json_string_fragment(value: str) -> str:
        encoded = json.dumps(value)
        return encoded[1:-1]

    raw = template_path.read_text(encoding="utf-8")
    rendered = (
        raw.replace("{{ADMIN_CERT_FINGERPRINT_SHA256}}", fingerprint.lower())
        .replace("{{PRINCIPAL_ISSUER}}", json_string_fragment(issuer))
        .replace("{{PRINCIPAL_SUBJECT}}", json_string_fragment(subject))
        .replace("{{POLICY_VERSION}}", json_string_fragment(policy_version))
        .replace(
            "{{PERMISSION_ASSIGNMENTS_JSON}}",
            json.dumps(assignments, indent=6, sort_keys=True),
        )
    )
    parsed = json.loads(rendered)
    if not isinstance(parsed, dict):
        raise ValueError("rendered security config must be an object")
    return parsed


def atomic_write_json(path: Path, document: dict[str, object], *, force: bool) -> None:
    if path.exists() and not force:
        raise FileExistsError(f"{path} exists; pass --force to overwrite")
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", dir=path.parent, delete=False) as handle:
        json.dump(document, handle, indent=2, sort_keys=True)
        handle.write("\n")
        tmp_name = handle.name
    Path(tmp_name).replace(path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Render GRM secured-local security.json.")
    parser.add_argument("--access-levels", required=True, type=Path)
    parser.add_argument("--template", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--fingerprint", required=True)
    parser.add_argument("--principal", default="admin-1")
    parser.add_argument("--issuer", default="local-admin")
    parser.add_argument("--access-level", default="owner-bootstrap-admin")
    parser.add_argument("--policy-version", default="secured-local-policy-v1")
    parser.add_argument(
        "--require-bootstrap-admin",
        action="store_true",
        help="Reject templates that cannot perform the Admin no.1 bootstrap smoke flow.",
    )
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not FINGERPRINT_RE.match(args.fingerprint):
        print("invalid certificate SHA-256 fingerprint", file=sys.stderr)
        return 2
    access_levels_raw = load_json(args.access_levels)
    if not isinstance(access_levels_raw, dict):
        print("access-levels.json must be an object", file=sys.stderr)
        return 2
    try:
        access_template = find_template(access_levels_raw, args.access_level)
        assignments = expand_assignments(access_template, args.issuer, args.principal)
        if args.require_bootstrap_admin:
            validate_bootstrap_capable(access_template, assignments)
        document = render_template(
            args.template,
            args.fingerprint,
            args.issuer,
            args.principal,
            args.policy_version,
            assignments,
        )
        atomic_write_json(args.output, document, force=args.force)
    except Exception as error:
        print(f"failed to render secured-local security config: {error}", file=sys.stderr)
        return 1
    print(f"Rendered {args.output}")
    print(f"Principal {args.issuer}/{args.principal}")
    print(f"Access template {args.access_level} expanded to explicit permissions.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
