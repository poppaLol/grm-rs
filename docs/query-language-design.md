# Query Language Design

This document sketches the next expansion of the `grm session` query language.

The goal is to increase query power while preserving the current dotted command style.

## Design Goals

- keep `node.find` and `edge.find` as the primary entrypoints
- support quoted values with whitespace
- keep field names identifier-like and whitespace-free
- add comparison and string matching operators without jumping straight to a whole new DSL
- make the intended CLI shape concrete before implementation
- pair the design with acceptance-style tests in `tests/runtime_session.rs`

## Guiding Rules

- field names do not allow whitespace
- values may allow whitespace when quoted
- parser work should land before rich query syntax is treated as stable
- query controls such as ordering and paging should remain explicit and readable

## Proposed Grammar

High-level command shape:

```text
node.find <ModelName> [<node-term> ...]
edge.find <LinkName> [<edge-term> ...]
```

Node query terms:

```text
<node-term> := <predicate>
             | limit=<int>
             | offset=<int>
             | order=<field>:asc|desc
```

Edge query terms:

```text
<edge-term> := <predicate>
             | from=<id>
             | to=<id>
             | limit=<int>
             | offset=<int>
             | order=<field>:asc|desc
```

Predicates:

```text
<predicate> := <field><op><value>
```

Operators:

```text
=    exact match
!=   not equal
>    greater than
>=   greater than or equal
<    less than
<=   less than or equal
~    string contains
```

Values:

```text
<value> := bare-value
         | "double quoted value"
         | 'single quoted value'
```

## CLI Mockups

### Equality and inequality

```text
node.find User name=Alice
node.find User name!="Alice Jones"
node.find User active=true
```

### Numeric comparison

```text
node.find User age>40
node.find User age>=21
node.find User age<65
node.find User age<=18
```

### String matching

```text
node.find User bio~"graph databases"
node.find Post title~"hello world"
```

### Ordering and paging

```text
node.find User age>=21 order=age:desc limit=10
node.find User active=true order=name:asc offset=20 limit=10
edge.find Authored year>=2020 order=year:desc limit=5
```

### Edge endpoint filtering

```text
edge.find Authored from=1
edge.find Authored to=2 year>=2024
```

### Mixed query examples

```text
node.find User name!="Alice Jones" active=true order=name:asc
edge.find Authored from=1 year>=2024 order=year:desc limit=10
```

## Output Mockups

As query power increases, result formatting should carry a little more structure.

Example node output:

```text
2 nodes matched model 'User'.
Node User userId=2 {name="Bob", age=43, active=true}
Node User userId=5 {name="Carol", age=41, active=false}
```

Example edge output:

```text
1 edge matched link 'Authored'.
Edge Authored authoredId=3 from=1 to=2 {year=2024}
```

Example no-results output:

```text
No nodes matched model 'User'.
No edges matched link 'Authored'.
```

## Reserved Query Terms

These should remain reserved inside `find` commands:

- `limit`
- `offset`
- `order`
- `from`
- `to`

`from` and `to` are special only for edge queries.

## Parser Expectations

The parser should:

- preserve quoted values as a single token
- support escaped quotes inside quoted strings
- distinguish parser errors from query validation errors
- reject malformed order clauses clearly
- reject unknown fields clearly

Examples of invalid input:

```text
node.find User user name="Alice"
node.find User age>>
node.find User order=age
node.find User name="Alice
```

## Test Expectations

Implementation work should include acceptance-style tests in `tests/runtime_session.rs` covering:

- equality and inequality
- numeric comparisons
- `contains` matching via `~`
- quoted values with spaces
- ordering
- limit and offset
- edge `from` / `to` with additional predicates
- invalid syntax and invalid field errors

The current test scaffolding for those planned cases lives in `tests/query_language_mockups.rs`.
