# Python Quickstart

This guide shows a Python-first developer how to:

1. build and install the `grm-rs` Python extension locally
2. use the extension from Python
3. run the `grm-rs` CLI from compiled Rust code

## Prerequisites

- Rust toolchain installed
- Python 3.9+
- a virtualenv tool such as `venv`

## Install The Python Extension

From the repo root:

```bash
python -m venv .venv
source .venv/bin/activate
pip install maturin
cd grm-python
maturin develop
```

`maturin develop` compiles the Rust extension and installs it into the active virtualenv, so `import grm_rs` works immediately in that environment.

## Use The Extension From Python

Example:

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

session.model_create(
    "Post",
    "postId",
    [
        {"name": "title", "type": "string", "required": True},
    ],
)

session.link_create(
    "Authored",
    "User",
    "Post",
    "authoredId",
    [
        {"name": "year", "type": "int", "required": True},
    ],
)

user = session.node_create("User", {"name": "Alice", "age": 42})
post = session.node_create("Post", {"title": "Hello"})
edge = session.edge_create("Authored", user["id"], post["id"], {"year": 2024})

print(user)
print(edge)
print(session.node_find("User", {"name": "Alice"}))
print(session.edge_find("Authored"))
```

### Data Shape Notes

- Field definitions are Python dicts with `name`, `type`, and `required`
- Supported field types are `string`, `int`, `float`, and `bool`
- The Python method names mostly mirror the CLI commands with `_` instead of `.`, such as `model_create`, `node_find`, and `edge_update`
- Session persistence helpers use the shorter Python-style names `save_json`, `save_binary`, `load_json`, and `load_binary`
- Autocommit is off by default; enable it at construction time with `Session(autocommit=True, autocommit_path="test-dbs/session.json")`
- When autocommit is enabled, `session.autocommit` is `True` and every successful mutating operation persists the session file
- `node_create`, `node_find`, `edge_create`, and `edge_find` return plain Python dictionaries/lists
- Rust-side failures are raised as `grm_rs.GrmError`
- For local scratch session files, prefer keeping them under `test-dbs/`

## Run The CLI From Compiled Code

The Python extension does not wrap the CLI directly. The CLI is still the Rust binary named `grm`.

To build it:

```bash
cargo build --bin grm
```

Then run the compiled binary:

```bash
./target/debug/grm session
```

If you want an optimized build:

```bash
cargo build --release --bin grm
./target/release/grm session
```

You can also run a setup script through the compiled binary:

```bash
./target/debug/grm session --script examples/session_setup.grm
```

## Typical Developer Workflow

For a Python-focused contributor, the common loop is:

1. activate the virtualenv
2. run `maturin develop` after Rust changes that affect the extension
3. run Python code against `grm_rs.Session`
4. build or run `grm` separately when working with the interactive CLI

If you are editing both the Python bindings and the CLI/runtime code, it is normal to use both of these during development:

```bash
cd grm-python && maturin develop
cargo build --bin grm
```
