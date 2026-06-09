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
    def write_criterion_result(
        self,
        criterion_root,
        group_id,
        function_id,
        point_estimate,
    ):
        result_dir = (
            Path(criterion_root) / group_id / function_id / "new"
        )
        result_dir.mkdir(parents=True)
        (result_dir / "benchmark.json").write_text(
            json.dumps(
                {
                    "group_id": group_id,
                    "function_id": function_id,
                    "full_id": f"{group_id}/{function_id}",
                }
            ),
            encoding="utf-8",
        )
        (result_dir / "estimates.json").write_text(
            json.dumps(
                {
                    "median": {
                        "point_estimate": point_estimate,
                        "confidence_interval": {
                            "confidence_level": 0.95,
                            "lower_bound": point_estimate * 0.9,
                            "upper_bound": point_estimate * 1.1,
                        },
                    }
                }
            ),
            encoding="utf-8",
        )

    def report_envelope(self):
        return {
            "work_slice": 221,
            "run": {
                "id": "test-run",
                "completed_at": "2026-06-09T12:00:00Z",
            },
            "source": {
                "commit": "abc123",
                "branch": "test",
                "dirty": False,
            },
            "platform": {
                "provider": "test-provider",
                "region": "test-region",
                "availability_zone": None,
                "instance_type": "test-shape",
                "storage_description": "test SSD",
            },
            "machine": {
                "cpu": {
                    "logical_cpus": 4,
                    "model": "Test CPU",
                },
                "memory": {
                    "total_bytes": 8 * 1024**3,
                },
                "os": {
                    "name": "Test Linux",
                    "kernel": "1.2.3",
                },
            },
            "toolchain": {
                "rustc_verbose": "rustc 1.88.0\nhost: test",
            },
            "locked_package_versions": {
                "criterion": "0.5.1",
            },
            "benchmark": {
                "benchmark_line": "GRM local gRPC mutual TLS",
                "tls_mode": "mutual TLS",
                "persistence_format": "binary workspace",
                "dataset_shape": "250 and 1,000 rows",
            },
        }

    def test_generate_reports_creates_web_friendly_table_and_json(self):
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = Path(temporary)
            criterion_root = run_dir / "cargo-target" / "criterion"
            self.write_criterion_result(
                criterion_root,
                "baseline_grpc_mtls_250",
                "grm_local_grpc_mtls_node_find_name_eq",
                125_000,
            )
            self.write_criterion_result(
                criterion_root,
                "baseline_grpc_mtls_1k",
                "grm_local_grpc_mtls_node_find_name_eq",
                150_000,
            )

            json_path, markdown_path = cloud_benchmark.generate_reports(
                self.report_envelope(),
                criterion_root,
                run_dir,
            )

            report = json.loads(json_path.read_text(encoding="utf-8"))
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertEqual(report["schema_version"], 1)
            self.assertEqual(report["evidence_status"], "publication_candidate")
            self.assertEqual(len(report["results"]), 2)
            self.assertEqual(report["results"][0]["estimate_kind"], "median")
            self.assertIn("| Operation | 250 rows | 1,000 rows |", markdown)
            self.assertIn(
                "| Node property lookup | 125.00 us | 150.00 us |",
                markdown,
            )
            self.assertIn("no database superiority", markdown)
            self.assertNotIn("hostname", json.dumps(report))

    def test_generate_reports_rejects_missing_criterion_results(self):
        with tempfile.TemporaryDirectory() as temporary:
            run_dir = Path(temporary)
            with self.assertRaisesRegex(
                ValueError,
                "no completed Criterion estimates",
            ):
                cloud_benchmark.generate_reports(
                    self.report_envelope(),
                    run_dir / "criterion",
                    run_dir,
                )

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
