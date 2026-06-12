# Python Package Distribution

The Python package is published as an alpha pre-release for public evaluation,
tutorials, and early application development. Alpha releases may change API or
backend capability details between versions.

## Package Names

- Distribution name: `grm-rs`
- Import name: `grm_rs`
- Current Python version: `0.1.0a7`

Pip uses `==` for versions:

```bash
python -m pip install grm-rs==0.1.0a7
```

To install the latest available pre-release without naming its version:

```bash
python -m pip install --pre grm-rs
```

## Build A Local Wheel

From the repository root:

```bash
python -m pip install maturin
maturin build --manifest-path grm-python/Cargo.toml --release --out dist
```

Then install the wheel into a virtualenv:

```bash
python -m pip install ./dist/grm_rs-0.1.0a7-*.whl
```

## Alternative Distribution

During release development, packages can also be distributed as wheel files or
GitHub prerelease assets:

1. Install a locally built wheel.
2. Download a wheel from a GitHub prerelease.
3. Publish first to TestPyPI when validating release automation.

## GitHub Release Pre-Releases

Use the manual `Python Wheels` GitHub Actions workflow to build wheels. It can
either upload build artifacts only, or create/update a draft prerelease such as:

```text
grm-python-v0.1.0a7
```

Users can install a downloaded wheel file:

```bash
python -m pip install ./grm_rs-0.1.0a7-*.whl
```

Or install directly from a release asset URL:

```bash
python -m pip install "https://github.com/<owner>/<repo>/releases/download/grm-python-v0.1.0a7/<wheel-file>.whl"
```

## PyPI Release Checks

Before publishing each alpha:

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
