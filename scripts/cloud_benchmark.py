#!/usr/bin/env python3
"""Run a GRM benchmark with a repeatable machine provenance envelope."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import re
import signal
import shutil
import socket
import subprocess
import sys
from pathlib import Path
from typing import Any


PROFILES: dict[str, dict[str, Any]] = {
    "embedded-quick": {
        "command": ["scripts/benchmarks.sh", "quick", "--noplot"],
        "benchmark_line": "GRM embedded in-memory",
        "tls_mode": "none",
        "persistence_format": "in-memory",
        "dataset_shape": "deterministic 1,000-row property lookup",
        "comparator_environment": "SQLite local where registered by the selected group",
    },
    "local-grpc-insecure": {
        "command": [
            "scripts/benchmarks.sh",
            "local-grpc-insecure",
            "--",
            "--noplot",
        ],
        "benchmark_line": "GRM local gRPC insecure",
        "tls_mode": "insecure",
        "persistence_format": "binary workspace",
        "dataset_shape": "deterministic 250-row and 1,000-row workspace fixtures",
        "comparator_environment": "none",
    },
    "local-grpc-mtls": {
        "command": [
            "scripts/benchmarks.sh",
            "local-grpc-mtls",
            "--",
            "--noplot",
        ],
        "benchmark_line": "GRM local gRPC mutual TLS",
        "tls_mode": "mutual TLS",
        "persistence_format": "binary workspace",
        "dataset_shape": "deterministic 250-row and 1,000-row workspace fixtures",
        "comparator_environment": "none",
    },
    "persistence": {
        "command": ["scripts/benchmarks.sh", "persistence", "--", "--noplot"],
        "benchmark_line": "GRM embedded persistence",
        "tls_mode": "none",
        "persistence_format": "JSON and binary workspace formats",
        "dataset_shape": "deterministic 1,000-row persistence fixture",
        "comparator_environment": "none",
    },
}

SAFE_ENVIRONMENT_KEYS = (
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
    "CARGO_PROFILE_BENCH_DEBUG",
    "RUSTFLAGS",
    "RUST_BACKTRACE",
    "GRM_BENCH_STRESS",
)

FUNCTION_PREFIXES = (
    "grm_local_grpc_mtls_",
    "grm_local_grpc_insecure_",
    "grm_embedded_in_memory_",
    "grm_",
)

DATASET_LABELS = {
    "250": "250 rows",
    "1k": "1,000 rows",
    "10k": "10,000 rows",
    "100k": "100,000 rows",
}

OPERATION_LABELS = {
    "create_node_populated_workspace": "Node create",
    "update_node_populated_workspace": "Node update",
    "create_edge_populated_workspace": "Edge create",
    "update_edge_populated_workspace": "Edge update",
    "node_find_name_eq": "Node property lookup",
    "edge_find_from": "Edge endpoint lookup",
    "traversal_selective": "Selective traversal",
    "explain_node_find": "Explain node.find",
    "explain_edge_find": "Explain edge.find",
    "profile_node_find": "Profile node.find",
    "profile_edge_find": "Profile edge.find",
}


def utc_now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z")


def run_capture(command: list[str], cwd: Path) -> str | None:
    try:
        result = subprocess.run(
            command,
            cwd=cwd,
            check=False,
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def git_value(repo_root: Path, *args: str) -> str | None:
    return run_capture(["git", *args], repo_root)


def read_os_release() -> dict[str, str]:
    path = Path("/etc/os-release")
    if not path.exists():
        return {}
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        values[key] = value.strip().strip('"')
    return values


def parse_lscpu(repo_root: Path) -> dict[str, str]:
    output = run_capture(["lscpu", "--json"], repo_root)
    if output is None:
        return {}
    try:
        rows = json.loads(output)["lscpu"]
    except (KeyError, TypeError, json.JSONDecodeError):
        return {}
    return {
        str(row["field"]).rstrip(":"): str(row["data"])
        for row in rows
        if "field" in row and "data" in row
    }


def memory_value_bytes(name: str) -> int | None:
    path = Path("/proc/meminfo")
    if not path.exists():
        return None
    pattern = re.compile(rf"^{re.escape(name)}:\s+(\d+)\s+kB$")
    for line in path.read_text(encoding="utf-8").splitlines():
        match = pattern.match(line)
        if match:
            return int(match.group(1)) * 1024
    return None


def cpu_governors() -> list[str]:
    governors = set()
    for path in Path("/sys/devices/system/cpu").glob(
        "cpu[0-9]*/cpufreq/scaling_governor"
    ):
        try:
            governors.add(path.read_text(encoding="utf-8").strip())
        except OSError:
            continue
    return sorted(governors)


def machine_metadata(repo_root: Path, output_root: Path) -> dict[str, Any]:
    lscpu = parse_lscpu(repo_root)
    disk = shutil.disk_usage(output_root)
    os_release = read_os_release()
    virtualization = run_capture(["systemd-detect-virt"], repo_root)
    return {
        "hostname": socket.gethostname(),
        "architecture": platform.machine(),
        "os": {
            "name": os_release.get("PRETTY_NAME", platform.system()),
            "id": os_release.get("ID"),
            "version_id": os_release.get("VERSION_ID"),
            "kernel": platform.release(),
        },
        "cpu": {
            "model": lscpu.get("Model name") or platform.processor() or None,
            "architecture": lscpu.get("Architecture") or platform.machine(),
            "logical_cpus": os.cpu_count(),
            "sockets": lscpu.get("Socket(s)"),
            "cores_per_socket": lscpu.get("Core(s) per socket"),
            "threads_per_core": lscpu.get("Thread(s) per core"),
            "max_mhz": lscpu.get("CPU max MHz"),
            "min_mhz": lscpu.get("CPU min MHz"),
            "governors": cpu_governors(),
        },
        "memory": {
            "total_bytes": memory_value_bytes("MemTotal"),
            "swap_total_bytes": memory_value_bytes("SwapTotal"),
        },
        "disk": {
            "path": str(output_root),
            "total_bytes": disk.total,
            "free_bytes_at_start": disk.free,
        },
        "virtualization": None
        if virtualization in (None, "none")
        else virtualization,
    }


def toolchain_metadata(repo_root: Path) -> dict[str, Any]:
    return {
        "rustc_verbose": run_capture(["rustc", "--version", "--verbose"], repo_root),
        "cargo_version": run_capture(["cargo", "--version"], repo_root),
        "python_version": platform.python_version(),
    }


def source_metadata(repo_root: Path) -> dict[str, Any]:
    status = git_value(repo_root, "status", "--porcelain")
    return {
        "commit": git_value(repo_root, "rev-parse", "HEAD"),
        "branch": git_value(repo_root, "branch", "--show-current"),
        "dirty": None if status is None else bool(status),
        "describe": git_value(repo_root, "describe", "--always", "--dirty", "--tags"),
    }


def locked_package_versions(repo_root: Path) -> dict[str, str]:
    lockfile = repo_root / "Cargo.lock"
    if not lockfile.exists():
        return {}
    wanted = {"criterion", "libsqlite3-sys", "rusqlite", "rustls", "tonic"}
    versions: dict[str, str] = {}
    current_name: str | None = None
    for line in lockfile.read_text(encoding="utf-8").splitlines():
        if line.startswith("name = "):
            current_name = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("version = ") and current_name in wanted:
            versions[current_name] = line.split("=", 1)[1].strip().strip('"')
            current_name = None
    return versions


def safe_environment() -> dict[str, str]:
    return {
        key: os.environ[key]
        for key in SAFE_ENVIRONMENT_KEYS
        if key in os.environ
    }


def slug(value: str) -> str:
    return re.sub(r"[^a-zA-Z0-9_.-]+", "-", value).strip("-") or "run"


def write_json(path: Path, value: dict[str, Any]) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def display_path(path: Path, repo_root: Path) -> str:
    try:
        return str(path.relative_to(repo_root))
    except ValueError:
        return str(path)


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"could not read Criterion JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ValueError(f"Criterion JSON must be an object: {path}")
    return value


def dataset_label(group_id: str) -> str:
    suffix = group_id.rsplit("_", 1)[-1]
    return DATASET_LABELS.get(suffix, suffix if suffix != group_id else group_id)


def operation_label(function_id: str) -> str:
    operation = function_id
    for prefix in FUNCTION_PREFIXES:
        if operation.startswith(prefix):
            operation = operation[len(prefix) :]
            break
    return OPERATION_LABELS.get(
        operation,
        operation.replace("_", " ").strip().capitalize(),
    )


def criterion_results(criterion_root: Path) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for benchmark_path in sorted(criterion_root.glob("**/new/benchmark.json")):
        estimates_path = benchmark_path.with_name("estimates.json")
        if not estimates_path.exists():
            continue
        benchmark = load_json(benchmark_path)
        estimates = load_json(estimates_path)
        if "median" not in estimates:
            continue
        estimate = estimates["median"]
        confidence = estimate.get("confidence_interval", {})
        try:
            point = float(estimate["point_estimate"])
            lower = float(confidence["lower_bound"])
            upper = float(confidence["upper_bound"])
            group_id = str(benchmark["group_id"])
            function_id = str(benchmark["function_id"])
        except (KeyError, TypeError, ValueError) as error:
            raise ValueError(
                f"incomplete Criterion estimate in {estimates_path}: {error}"
            ) from error
        results.append(
            {
                "benchmark_id": str(
                    benchmark.get("full_id", f"{group_id}/{function_id}")
                ),
                "group_id": group_id,
                "function_id": function_id,
                "dataset": dataset_label(group_id),
                "operation": operation_label(function_id),
                "estimate_kind": "median",
                "median_ns": point,
                "confidence_interval_ns": {
                    "confidence_level": confidence.get("confidence_level"),
                    "lower_bound": lower,
                    "upper_bound": upper,
                },
            }
        )
    return results


def format_duration(nanoseconds: float) -> str:
    if nanoseconds < 1_000:
        return f"{nanoseconds:.2f} ns"
    if nanoseconds < 1_000_000:
        return f"{nanoseconds / 1_000:.2f} us"
    if nanoseconds < 1_000_000_000:
        return f"{nanoseconds / 1_000_000:.2f} ms"
    return f"{nanoseconds / 1_000_000_000:.2f} s"


def public_machine_summary(envelope: dict[str, Any]) -> str:
    cpu = envelope["machine"]["cpu"]
    memory = envelope["machine"]["memory"]["total_bytes"]
    parts = [
        f"{cpu.get('logical_cpus') or 'unknown'} logical CPUs",
        cpu.get("model") or "unknown CPU model",
    ]
    if isinstance(memory, int):
        parts.append(f"{memory / (1024**3):.1f} GiB RAM")
    return ", ".join(parts)


def measurement_exclusions(tls_mode: str) -> list[str]:
    exclusions = ["fixture and benchmark setup"]
    if tls_mode in ("insecure", "mutual TLS"):
        exclusions.extend(["service startup", "connection establishment"])
    if tls_mode == "mutual TLS":
        exclusions.extend(["certificate generation", "TLS handshake"])
    return exclusions


def public_claim_boundary(tls_mode: str, evidence_status: str) -> str:
    if evidence_status == "exploratory_preview":
        baseline = "Exploratory local preview only"
    elif tls_mode == "mutual TLS":
        baseline = "Reproducible GRM secured service baseline only"
    else:
        baseline = "Reproducible benchmark-profile evidence only"
    return (
        f"{baseline}; no database superiority, hosted durability, multi-writer, "
        "authorization, or production certificate-lifecycle claim."
    )


def markdown_report(report: dict[str, Any]) -> str:
    provenance = report["provenance"]
    results = report["results"]
    datasets = sorted(
        {result["dataset"] for result in results},
        key=lambda value: (
            list(DATASET_LABELS.values()).index(value)
            if value in DATASET_LABELS.values()
            else len(DATASET_LABELS),
            value,
        ),
    )
    operations = sorted({result["operation"] for result in results})
    by_operation_dataset = {
        (result["operation"], result["dataset"]): result for result in results
    }

    lines = [
        "# GRM Performance Benchmark",
        "",
        (
            "Exploratory local preview generated from retained Criterion artifacts."
            if report["evidence_status"] == "exploratory_preview"
            else
            "Representative operation latency for GRM's local mutual-TLS "
            "workspace service on a reproducible VPS configuration."
            if provenance["tls_mode"] == "mutual TLS"
            else (
                "Representative Criterion latency from the "
                f"{provenance['benchmark_line']} benchmark profile."
            )
        ),
        "",
        "| Operation | " + " | ".join(datasets) + " |",
        "| --- | " + " | ".join("---:" for _ in datasets) + " |",
    ]
    for operation in operations:
        values = []
        for dataset in datasets:
            result = by_operation_dataset.get((operation, dataset))
            values.append(
                format_duration(result["median_ns"]) if result is not None else "-"
            )
        lines.append(f"| {operation} | " + " | ".join(values) + " |")

    if provenance["tls_mode"] in ("insecure", "mutual TLS"):
        measurement_note = (
            "Values are Criterion median estimates over established connections; "
            + ", ".join(report["measurement"]["excluded_from_operation_duration"])
            + " are outside the measured operation duration."
        )
    else:
        measurement_note = (
            "Values are Criterion median estimates; "
            + ", ".join(report["measurement"]["excluded_from_operation_duration"])
            + " is outside the measured operation duration."
        )

    lines.extend(
        [
            "",
            measurement_note,
            "",
            "## Run Provenance",
            "",
            "| Field | Value |",
            "| --- | --- |",
            f"| GRM commit | `{provenance['commit']}` |",
            f"| Date | {provenance['completed_at']} |",
            f"| Benchmark line | {provenance['benchmark_line']} |",
            f"| TLS mode | {provenance['tls_mode']} |",
            f"| Persistence | {provenance['persistence_format']} |",
            (
                f"| Provider | {provenance['provider']} / "
                f"{provenance['region']} / {provenance['instance_type']} |"
            ),
            f"| Machine | {provenance['machine']} |",
            f"| Storage | {provenance['storage']} |",
            f"| OS / kernel | {provenance['os']} |",
            f"| Rust | {provenance['rust']} |",
            "",
            "## Claim Boundary",
            "",
            report["claim_boundary"],
            "",
        ]
    )
    return "\n".join(lines)


def generate_reports(
    envelope: dict[str, Any],
    criterion_root: Path,
    run_dir: Path,
) -> tuple[Path, Path]:
    results = criterion_results(criterion_root)
    if not results:
        raise ValueError(f"no completed Criterion estimates found under {criterion_root}")

    source = envelope["source"]
    platform_data = envelope["platform"]
    os_data = envelope["machine"]["os"]
    rustc = envelope["toolchain"].get("rustc_verbose") or "unknown"
    rust_version = rustc.splitlines()[0]
    evidence_status = (
        "exploratory_preview"
        if source.get("dirty")
        or str(platform_data["provider"]).lower().startswith("local")
        else "publication_candidate"
    )
    report = {
        "schema_version": 1,
        "report_type": "grm_performance_baseline",
        "evidence_status": evidence_status,
        "work_slice": envelope["work_slice"],
        "provenance": {
            "run_id": envelope["run"]["id"],
            "commit": source["commit"],
            "branch": source["branch"],
            "completed_at": envelope["run"]["completed_at"],
            "benchmark_line": envelope["benchmark"]["benchmark_line"],
            "tls_mode": envelope["benchmark"]["tls_mode"],
            "persistence_format": envelope["benchmark"]["persistence_format"],
            "dataset_shape": envelope["benchmark"]["dataset_shape"],
            "provider": platform_data["provider"],
            "region": platform_data["region"],
            "availability_zone": platform_data["availability_zone"],
            "instance_type": platform_data["instance_type"],
            "machine": public_machine_summary(envelope),
            "storage": platform_data["storage_description"],
            "os": f"{os_data['name']} / {os_data['kernel']}",
            "rust": rust_version,
            "criterion": envelope["locked_package_versions"].get("criterion"),
        },
        "measurement": {
            "estimate": "Criterion median",
            "unit": "nanoseconds",
            "confidence_intervals_included": True,
            "excluded_from_operation_duration": measurement_exclusions(
                envelope["benchmark"]["tls_mode"]
            ),
        },
        "claim_boundary": public_claim_boundary(
            envelope["benchmark"]["tls_mode"],
            evidence_status,
        ),
        "results": results,
    }
    json_path = run_dir / "report.json"
    markdown_path = run_dir / "report.md"
    write_json(json_path, report)
    markdown_path.write_text(markdown_report(report), encoding="utf-8")
    return json_path, markdown_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Run an existing GRM Criterion profile and record machine, source, "
            "toolchain, benchmark, and target-isolation provenance."
        )
    )
    parser.add_argument("profile", choices=sorted(PROFILES))
    parser.add_argument("--provider", required=True, help="Cloud/VPS provider or local")
    parser.add_argument("--region", required=True, help="Provider region or local")
    parser.add_argument(
        "--availability-zone",
        default=None,
        help="Provider availability zone when applicable",
    )
    parser.add_argument(
        "--instance-type",
        required=True,
        help="Provider machine/instance type or a stable local machine label",
    )
    parser.add_argument(
        "--target-description",
        required=True,
        help="Human-readable identity of the isolated benchmark target",
    )
    parser.add_argument(
        "--storage-description",
        required=True,
        help="Provider storage class and size, or the equivalent local disk description",
    )
    parser.add_argument(
        "--confirm-disposable",
        action="store_true",
        help="Confirm this target is isolated and contains no shared project memory",
    )
    parser.add_argument(
        "--output-root",
        type=Path,
        default=Path("target/benchmark-runs"),
        help="Run directory parent (default: target/benchmark-runs)",
    )
    parser.add_argument(
        "--run-label",
        default=None,
        help="Optional stable label included in the run directory name",
    )
    parser.add_argument(
        "--collect-only",
        action="store_true",
        help="Write and validate provenance without running Cargo or Criterion",
    )
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="Allow an uncommitted checkout for exploratory runs only",
    )
    return parser


def parse_cli(argv: list[str]) -> tuple[argparse.Namespace, list[str]]:
    if "--" in argv:
        delimiter = argv.index("--")
        runner_args = argv[:delimiter]
        benchmark_args = argv[delimiter + 1 :]
    else:
        runner_args = argv
        benchmark_args = []
    return build_parser().parse_args(runner_args), benchmark_args


def terminate_process_group(process: subprocess.Popen[str]) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            return
        process.wait()


def main() -> int:
    args, benchmark_args = parse_cli(sys.argv[1:])
    if not args.confirm_disposable:
        print(
            "error: --confirm-disposable is required; do not benchmark against "
            "shared project memory or an ambiguous database target",
            file=sys.stderr,
        )
        return 2

    repo_root = Path(__file__).resolve().parent.parent
    source = source_metadata(repo_root)
    if not args.collect_only and (
        source["commit"] is None or source["dirty"] is None
    ):
        print(
            "error: unable to verify Git commit and worktree status; real "
            "benchmark runs require trustworthy source provenance",
            file=sys.stderr,
        )
        return 2
    if source["dirty"] and not args.allow_dirty:
        print(
            "error: benchmark checkout is dirty; commit/stash changes or use "
            "--allow-dirty for a non-public exploratory run",
            file=sys.stderr,
        )
        return 2

    output_root = (
        args.output_root
        if args.output_root.is_absolute()
        else repo_root / args.output_root
    )
    output_root.mkdir(parents=True, exist_ok=True)

    profile = PROFILES[args.profile]
    commit = source["commit"][:12] if source["commit"] is not None else "unknown"
    timestamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    labels = [timestamp, slug(args.profile), slug(args.run_label or commit)]
    run_dir = output_root / "-".join(labels)
    suffix = 1
    while run_dir.exists():
        run_dir = output_root / f"{'-'.join(labels)}-{suffix}"
        suffix += 1
    run_dir.mkdir()

    command = [*profile["command"], *benchmark_args]
    cargo_target_dir = run_dir / "cargo-target"
    log_path = run_dir / "benchmark.log"
    provenance_path = run_dir / "provenance.json"
    started_at = utc_now()

    envelope: dict[str, Any] = {
        "schema_version": 1,
        "work_slice": 221,
        "run": {
            "id": run_dir.name,
            "status": "collect_only" if args.collect_only else "running",
            "started_at": started_at,
            "completed_at": None,
            "exit_code": None,
            "command": command,
            "working_directory": str(repo_root),
            "log": display_path(log_path, repo_root),
            "criterion_root": display_path(
                cargo_target_dir / "criterion", repo_root
            ),
        },
        "source": source,
        "platform": {
            "provider": args.provider,
            "region": args.region,
            "availability_zone": args.availability_zone,
            "instance_type": args.instance_type,
            "target_description": args.target_description,
            "storage_description": args.storage_description,
        },
        "machine": machine_metadata(repo_root, output_root),
        "toolchain": toolchain_metadata(repo_root),
        "locked_package_versions": locked_package_versions(repo_root),
        "benchmark": {
            "profile": args.profile,
            "benchmark_line": profile["benchmark_line"],
            "tls_mode": profile["tls_mode"],
            "persistence_format": profile["persistence_format"],
            "dataset_shape": profile["dataset_shape"],
            "comparator_environment": profile["comparator_environment"],
            "extra_arguments": benchmark_args,
        },
        "safety": {
            "disposable_target_confirmed": True,
            "shared_project_memory_present": False,
            "dirty_checkout_allowed": args.allow_dirty,
            "cleanup_scope": "runner-owned temporary files and workspace roots only",
        },
        "environment": safe_environment(),
    }
    write_json(provenance_path, envelope)

    if args.collect_only:
        envelope["run"]["completed_at"] = utc_now()
        envelope["run"]["exit_code"] = 0
        write_json(provenance_path, envelope)
        print(provenance_path)
        return 0

    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(cargo_target_dir)
    exit_code = 127
    with log_path.open("w", encoding="utf-8") as log:
        try:
            process = subprocess.Popen(
                command,
                cwd=repo_root,
                env=environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
                start_new_session=True,
            )
        except OSError as error:
            message = f"failed to start benchmark: {error}\n"
            sys.stderr.write(message)
            log.write(message)
        else:
            try:
                assert process.stdout is not None
                for line in process.stdout:
                    sys.stdout.write(line)
                    log.write(line)
                exit_code = process.wait()
            except KeyboardInterrupt:
                terminate_process_group(process)
                exit_code = 130

    envelope["run"]["status"] = "completed" if exit_code == 0 else "failed"
    envelope["run"]["completed_at"] = utc_now()
    envelope["run"]["exit_code"] = exit_code
    envelope["machine"]["disk"]["free_bytes_at_end"] = shutil.disk_usage(
        output_root
    ).free
    report_paths: tuple[Path, Path] | None = None
    if exit_code == 0:
        try:
            report_paths = generate_reports(
                envelope,
                cargo_target_dir / "criterion",
                run_dir,
            )
            envelope["run"]["report_json"] = display_path(
                report_paths[0], repo_root
            )
            envelope["run"]["report_markdown"] = display_path(
                report_paths[1], repo_root
            )
        except (OSError, ValueError) as error:
            exit_code = 1
            envelope["run"]["status"] = "report_failed"
            envelope["run"]["exit_code"] = exit_code
            envelope["run"]["report_error"] = str(error)
    write_json(provenance_path, envelope)
    print(f"provenance: {provenance_path}")
    print(f"benchmark log: {log_path}")
    if report_paths is not None:
        print(f"web report JSON: {report_paths[0]}")
        print(f"web report Markdown: {report_paths[1]}")
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
