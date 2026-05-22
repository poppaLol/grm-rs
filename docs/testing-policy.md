# Testing Policy

GRM tests should reinforce the same boundaries as the code.

## Default Placement

- Runtime core behavior belongs in `tests/runtime_*.rs`.
- CLI behavior belongs in CLI/session integration tests, such as `tests/cli_*`
  or `tests/runtime_session.rs`.
- MCP behavior belongs in `grm-mcp/tests/`.
- Python behavior belongs in Python smoke or integration tests.
- Backend contracts belong in shared backend integration tests.

## Inline Unit Tests

Inline `#[cfg(test)]` modules are allowed when they test a small private helper,
parser edge case, or internal invariant that is awkward to reach through a public
surface.

Good examples:

- closed transaction state inside `GraphClient`
- tiny parser helpers
- private normalization or validation helpers

Avoid inline tests for product-facing contracts. Help output, MCP tool schemas,
CLI startup behavior, JSON shapes, and public runtime behavior should be tested
through the surface that users call.

## Golden And Contract Tests

Use integration or golden tests for stable public shapes:

- MCP tool and resource JSON
- CLI output that should not drift accidentally
- interchange/export documents
- structured runtime request and response shapes

## Rule Of Thumb

If changing the test would require explaining a user-visible behavior, put it in
an integration test. If changing the test only explains a private implementation
detail, an inline unit test is fine.

Future graph rules may encode this policy by flagging new inline test modules in
adapter or public-surface files unless the file is explicitly allowlisted.
