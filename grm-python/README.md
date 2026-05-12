# grm-rs Python bindings

This crate packages a first-pass Python API for the `grm-rs` runtime session surface.

The current Python package is a pre-release. It is meant for private wheel
sharing and GitHub Release testing before any public PyPI upload.

Install from a wheel file:

```bash
python -m pip install ./dist/grm_rs-0.1.0a2-*.whl
```

Or install from an authenticated GitHub Release asset URL:

```bash
python -m pip install "https://github.com/<owner>/<repo>/releases/download/grm-python-v0.1.0a2/<wheel-file>.whl"
```

The distribution package is named `grm-rs`; the import package is `grm_rs`.

## Build

```bash
cd grm-python
maturin develop
```

Build a shareable wheel from the repository root:

```bash
python -m pip install maturin
maturin build --manifest-path grm-python/Cargo.toml --release --out dist
```

See [`../docs/python-package-distribution.md`](../docs/python-package-distribution.md)
for private sharing and pre-release options.

## Example

```python
from grm_rs import Session

session = Session()
session.model_create(
    "User",
    "userId",
    [
        {"name": "name", "type": "string", "required": True},
        {"name": "age", "type": "int", "required": False},
    ],
)

alice = session.node_create("User", {"name": "Alice", "age": 42})
users = session.node_find("User", {"name": "Alice"})
```

Use `save_json` / `load_json` or `save_binary` / `load_binary` for local
workspace snapshots. Use `export_json`, `export_dict`, and `import_json` for
portable `grm.interchange` graph files; `import_json` currently imports into an
empty session only.
