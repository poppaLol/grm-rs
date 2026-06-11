import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MYPY_CONFIG = ROOT / "grm-python" / "pyproject.toml"
POSITIVE = ROOT / "tests" / "python_typing_positive.py"
NEGATIVE = ROOT / "tests" / "python_typing_negative.py"


def run_mypy(path: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "mypy",
            "--config-file",
            str(MYPY_CONFIG),
            str(path),
        ],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )


def main() -> None:
    positive = run_mypy(POSITIVE)
    if positive.returncode != 0:
        raise RuntimeError(f"positive typing fixture failed:\n{positive.stdout}{positive.stderr}")

    negative = run_mypy(NEGATIVE)
    output = negative.stdout + negative.stderr
    if negative.returncode == 0:
        raise RuntimeError("negative typing fixture unexpectedly passed")
    for expected in (
        'incompatible type "str"',
        '"None"',
        '"list[str]"',
        '"dict[str, bool]"',
        'Unexpected keyword argument "atomic"',
        "WorkspaceGraphSession",
        "profile_node_find",
    ):
        if expected not in output:
            raise RuntimeError(
                f"negative typing fixture did not reject {expected!r}:\n{output}"
            )


if __name__ == "__main__":
    main()
