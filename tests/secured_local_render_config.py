#!/usr/bin/env python3
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "examples" / "secured-local" / "scripts" / "render-security-config.py"
INPUTS_SCRIPT = ROOT / "examples" / "secured-local" / "scripts" / "write-bootstrap-inputs.py"
ACCESS_LEVELS = ROOT / "examples" / "secured-local" / "templates" / "access-levels.json"
TEMPLATE = ROOT / "examples" / "secured-local" / "templates" / "security-config.template.json"
FINGERPRINT = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"


def run_render(*extra: str) -> tuple[subprocess.CompletedProcess[str], dict[str, object] | None]:
    with tempfile.TemporaryDirectory() as tmp:
        output = Path(tmp) / "security.json"
        command = [
            sys.executable,
            str(SCRIPT),
            "--access-levels",
            str(ACCESS_LEVELS),
            "--template",
            str(TEMPLATE),
            "--output",
            str(output),
            "--fingerprint",
            FINGERPRINT,
            *extra,
        ]
        completed = subprocess.run(command, text=True, capture_output=True, check=False)
        rendered = json.loads(output.read_text(encoding="utf-8")) if output.exists() else None
        return completed, rendered


def test_owner_template_expands_to_explicit_permissions() -> None:
    completed, rendered = run_render("--principal", "admin-1", "--require-bootstrap-admin")
    assert completed.returncode == 0, completed.stderr
    assert rendered is not None
    assert rendered["certificate_mappings"] == [
        {
            "fingerprint_sha256": FINGERPRINT,
            "principal": {"issuer": "local-admin", "subject": "admin-1"},
        }
    ]
    assignments = rendered["permission_table"]["assignments"]
    assert assignments
    assert all("permissions" in assignment for assignment in assignments)
    assert any(
        permission == {"action": "workspace.create", "resource": {"kind": "service"}}
        for assignment in assignments
        for permission in assignment["permissions"]
    )
    assert all("access_level" not in assignment for assignment in assignments)


def test_unknown_template_fails_without_writing() -> None:
    completed, rendered = run_render("--access-level", "made-up")
    assert completed.returncode != 0
    assert rendered is None
    assert "unknown access level" in completed.stderr


def test_invalid_fingerprint_is_rejected() -> None:
    completed, rendered = run_render("--fingerprint", "not-a-secret-or-fingerprint")
    assert completed.returncode != 0
    assert rendered is None
    assert "invalid certificate SHA-256 fingerprint" in completed.stderr


def test_non_bootstrap_template_is_rejected_for_admin_no_1() -> None:
    completed, rendered = run_render(
        "--access-level", "read-only-explorer", "--require-bootstrap-admin"
    )
    assert completed.returncode != 0
    assert rendered is None
    assert "not bootstrap-capable" in completed.stderr


def test_bootstrap_inputs_reject_identity_mismatch_on_reuse() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        output = Path(tmp) / "bootstrap-inputs.json"
        base_command = [
            sys.executable,
            str(INPUTS_SCRIPT),
            "--output",
            str(output),
            "--issuer",
            "local-admin",
            "--principal",
            "admin-1",
            "--access-level",
            "owner-bootstrap-admin",
            "--policy-version",
            "secured-local-policy-v1",
        ]
        first = subprocess.run(base_command, text=True, capture_output=True, check=False)
        assert first.returncode == 0, first.stderr
        mismatch = subprocess.run(
            [*base_command[:-6], "--principal", "alice", "--access-level", "owner-bootstrap-admin", "--policy-version", "secured-local-policy-v1"],
            text=True,
            capture_output=True,
            check=False,
        )
        assert mismatch.returncode != 0
        assert "existing secured-local bootstrap uses local-admin/admin-1" in mismatch.stderr
        assert "Rerun with --force" in mismatch.stderr


if __name__ == "__main__":
    test_owner_template_expands_to_explicit_permissions()
    test_unknown_template_fails_without_writing()
    test_invalid_fingerprint_is_rejected()
    test_non_bootstrap_template_is_rejected_for_admin_no_1()
    test_bootstrap_inputs_reject_identity_mismatch_on_reuse()
    print("secured-local render config tests passed")
