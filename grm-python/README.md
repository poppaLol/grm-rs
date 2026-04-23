# grm-rs Python bindings

This crate packages a first-pass Python API for the `grm-rs` runtime session surface.

## Build

```bash
cd grm-python
maturin develop
```

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
