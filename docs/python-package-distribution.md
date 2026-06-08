# Python Package Distribution

The Python package is currently intended for pre-release sharing, not public
PyPI publication.

## Package Names

- Distribution name: `grm-rs`
- Import name: `grm_rs`
- Current Python version: `0.1.0a6`

Pip uses `==` for versions:

```bash
python -m pip install grm-rs==0.1.0a6
```

That command only works after the package is published to an index. For private
pre-release use, install a wheel file or a GitHub Release asset instead.

## Build A Local Wheel

From the repository root:

```bash
python -m pip install maturin
maturin build --manifest-path grm-python/Cargo.toml --release --out dist
```

Then install the wheel into a virtualenv:

```bash
python -m pip install ./dist/grm_rs-0.1.0a6-*.whl
```

## Share Without Public PyPI

Good early options:

1. Share wheel files directly with a small group.
2. Attach wheel files to a GitHub prerelease.
3. Use a private package index later if repeated installs become painful.

For a private GitHub repository, release assets are only available to users with
repository access. For a public repository, GitHub Release assets are public, so
do not use public releases for private package sharing.

## GitHub Release Pre-Releases

Use the manual `Python Wheels` GitHub Actions workflow to build wheels. It can
either upload build artifacts only, or create/update a draft prerelease such as:

```text
grm-python-v0.1.0a6
```

Users can install a downloaded wheel file:

```bash
python -m pip install ./grm_rs-0.1.0a6-*.whl
```

Or, if they have access to the release asset URL:

```bash
python -m pip install "https://github.com/<owner>/<repo>/releases/download/grm-python-v0.1.0a6/<wheel-file>.whl"
```

## Public PyPI Later

Before publishing publicly, decide:

- whether the public package name should stay `grm-rs`
- whether `0.1.0a6` has enough install smoke coverage across platforms
- whether the README is ready for people outside the project
- whether the package should publish to TestPyPI first
