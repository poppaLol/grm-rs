import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


REPO_ROOT = Path(__file__).resolve().parents[2]
RUNNER = REPO_ROOT / "scripts" / "cloud_benchmark.py"

sys.path.insert(0, str(REPO_ROOT / "scripts"))
import cloud_benchmark  # noqa: E402


class CloudBenchmarkRunnerTests(unittest.TestCase):
    def test_source_metadata_preserves_unavailable_git_status(self):
        with mock.patch.object(
            cloud_benchmark,
            "git_value",
            side_effect=[None, "commit", "branch", "describe"],
        ):
            metadata = cloud_benchmark.source_metadata(REPO_ROOT)

        self.assertEqual(metadata["commit"], "commit")
        self.assertIsNone(metadata["dirty"])

    def test_real_run_requires_verified_git_provenance(self):
        result = subprocess.run(
            [
                sys.executable,
                str(RUNNER),
                "local-grpc-mtls",
                "--provider",
                "test",
                "--region",
                "test",
                "--instance-type",
                "test",
                "--target-description",
                "test target",
                "--storage-description",
                "test storage",
                "--confirm-disposable",
            ],
            cwd=REPO_ROOT,
            env={**os.environ, "PATH": ""},
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn(
            "real benchmark runs require trustworthy source provenance",
            result.stderr,
        )

    def test_terminate_process_group_escalates_after_timeout(self):
        process = mock.Mock()
        process.pid = 1234
        process.wait.side_effect = [subprocess.TimeoutExpired("test", 10), 0]

        with mock.patch.object(cloud_benchmark.os, "killpg") as killpg:
            cloud_benchmark.terminate_process_group(process)

        self.assertEqual(
            killpg.call_args_list,
            [
                mock.call(1234, signal.SIGTERM),
                mock.call(1234, signal.SIGKILL),
            ],
        )

    def test_requires_disposable_target_confirmation(self):
        result = subprocess.run(
            [
                sys.executable,
                str(RUNNER),
                "local-grpc-mtls",
                "--provider",
                "test",
                "--region",
                "test",
                "--instance-type",
                "test",
                "--target-description",
                "test target",
                "--storage-description",
                "test storage",
                "--collect-only",
            ],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("--confirm-disposable is required", result.stderr)

    def test_collect_only_writes_complete_provenance(self):
        with tempfile.TemporaryDirectory() as temporary:
            result = subprocess.run(
                [
                    sys.executable,
                    str(RUNNER),
                    "local-grpc-mtls",
                    "--provider",
                    "test-provider",
                    "--region",
                    "test-region",
                    "--instance-type",
                    "test-shape",
                    "--target-description",
                    "isolated test target",
                    "--storage-description",
                    "local test disk",
                    "--confirm-disposable",
                    "--collect-only",
                    "--allow-dirty",
                    "--output-root",
                    temporary,
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            provenance_path = Path(result.stdout.strip())
            envelope = json.loads(provenance_path.read_text(encoding="utf-8"))

            self.assertEqual(envelope["schema_version"], 1)
            self.assertEqual(envelope["work_slice"], 221)
            self.assertEqual(envelope["run"]["status"], "collect_only")
            self.assertEqual(envelope["run"]["exit_code"], 0)
            self.assertEqual(
                envelope["benchmark"]["benchmark_line"],
                "GRM local gRPC mutual TLS",
            )
            self.assertEqual(envelope["benchmark"]["tls_mode"], "mutual TLS")
            self.assertEqual(envelope["platform"]["provider"], "test-provider")
            self.assertEqual(
                envelope["platform"]["storage_description"],
                "local test disk",
            )
            self.assertIn("criterion", envelope["locked_package_versions"])
            self.assertTrue(
                envelope["safety"]["disposable_target_confirmed"]
            )
            self.assertFalse(
                envelope["safety"]["shared_project_memory_present"]
            )
            self.assertTrue(envelope["safety"]["dirty_checkout_allowed"])


if __name__ == "__main__":
    unittest.main()
