# Python Package Distribution

The Python package is published as an early public release for evaluation,
tutorials, and early application development. Early releases may change API or
backend capability details between versions.

## Package Names

- Distribution name: `grm-rs`
- Import name: `grm_rs`
- Python release candidate: `0.3.0` (not published by merging the release PR)
- Previously published Python version: `0.2.0`

Pip uses `==` for versions:

```bash
python -m pip install grm-rs==0.2.0
```

To install the latest available release without naming its version:

```bash
python -m pip install grm-rs
```

## Build A Local Wheel

From the repository root:

```bash
python -m pip install maturin
maturin build --manifest-path grm-python/Cargo.toml --release --out dist
```

Then install the wheel into a virtualenv:

```bash
python -m pip install ./dist/grm_rs-0.3.0-*.whl
```

## Alternative Distribution

During release development, packages can also be distributed as wheel files or
GitHub release assets:

1. Install a locally built wheel.
2. Download a wheel from a GitHub release.
3. Publish first to TestPyPI when validating release automation.

## GitHub Releases

Use the manual `Python Wheels` GitHub Actions workflow to build wheels. It can
either upload build artifacts only, or create/update a draft release such as:

```text
grm-python-v0.3.0
```

Users can install a downloaded wheel file:

```bash
python -m pip install ./grm_rs-0.3.0-*.whl
```

Or install directly from a release asset URL:

```bash
python -m pip install "https://github.com/<owner>/<repo>/releases/download/grm-python-v0.3.0/<wheel-file>.whl"
```

## PyPI Release Checks

Before publishing each release:

- build and verify wheels on each supported platform
- build and verify the source distribution
- confirm the Apache 2.0 license text is included
- install the candidate into a clean environment and run the Python smoke tests
- publish to TestPyPI first when changing release automation

## Trusted Publishing

The `Python Wheels` GitHub Actions workflow can publish verified artifacts to
PyPI without a stored API token. Run it manually from `main` with
`publish_pypi` enabled.

Configure the PyPI trusted publisher with:

- owner: `poppaLol`
- repository: `grm-rs`
- workflow: `python-wheels.yml`
- environment: `pypi`

The workflow requests GitHub's OIDC identity only in the publish job and uses
the `pypa/gh-action-pypi-publish` action to upload the distributions.

## Release 0.3.0 Candidate

This candidate includes typed primitive properties, including `datetime`.
Declare the field type as `datetime` and provide a tagged value with an RFC3339
timestamp containing an explicit timezone:

```python
{"$grm_type": "datetime", "value": "2026-10-09T14:30:00Z"}
```

Native Python `datetime.datetime` objects are not converted automatically.
The wheel smoke test verifies datetime readback after reopening a persisted
embedded workspace; this does not assert identical support for every backend.

Merge the candidate PR first, then run `Python Wheels` from `main` with publishing
disabled to inspect the artifacts. Each platform installs its candidate wheel
and runs `tests/python_bindings_smoke.py` before artifacts can be published.
When ready, run from `main` with `publish_pypi=true` and, optionally,
`publish_release=true` using `release_tag=grm-python-v0.3.0`.
The GitHub release is a draft; PyPI publishing is a separate explicit action.
