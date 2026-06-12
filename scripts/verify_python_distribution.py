import argparse
import tarfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_LICENSE = (ROOT / "LICENSE").read_bytes()


def verify_package_license() -> None:
    package_license = ROOT / "grm-python" / "python" / "grm_rs" / "LICENSE"
    if package_license.read_bytes() != EXPECTED_LICENSE:
        raise RuntimeError("grm-python/LICENSE must match the repository LICENSE")


def verify_wheels(dist: Path) -> None:
    wheels = sorted(dist.glob("*.whl"))
    if not wheels:
        raise RuntimeError(f"no wheels found in {dist}")

    for wheel in wheels:
        with zipfile.ZipFile(wheel) as archive:
            license_paths = [
                name
                for name in archive.namelist()
                if name == "grm_rs/LICENSE"
            ]
            if len(license_paths) != 1:
                raise RuntimeError(f"{wheel.name} must contain grm_rs/LICENSE")
            if archive.read(license_paths[0]) != EXPECTED_LICENSE:
                raise RuntimeError(f"{wheel.name} contains unexpected license text")


def verify_sdists(dist: Path) -> None:
    sdists = sorted(dist.glob("*.tar.gz"))
    if not sdists:
        raise RuntimeError(f"no source distributions found in {dist}")

    for sdist in sdists:
        with tarfile.open(sdist, "r:gz") as archive:
            license_members = [
                member
                for member in archive.getmembers()
                if member.isfile()
                and len(Path(member.name).parts) == 2
                and Path(member.name).name == "LICENSE"
            ]
            if len(license_members) != 1:
                raise RuntimeError(f"{sdist.name} must contain one LICENSE file")
            license_file = archive.extractfile(license_members[0])
            if license_file is None or license_file.read() != EXPECTED_LICENSE:
                raise RuntimeError(f"{sdist.name} contains unexpected license text")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dist", type=Path, default=Path("dist"))
    parser.add_argument("--artifact", choices=("wheel", "sdist"), required=True)
    args = parser.parse_args()

    verify_package_license()
    if args.artifact == "wheel":
        verify_wheels(args.dist)
    else:
        verify_sdists(args.dist)


if __name__ == "__main__":
    main()
