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
- query results should render through an explicit output format layer
- the current human-readable output should remain the default for now

## Proposed Grammar

High-level command shape:

```text
node.find <ModelName> [<node-term> ...]
edge.find <LinkName> [<edge-term> ...]
```

Node query terms:

```text
<node-term> := <predicate>
             | via=<traversal-step>
             | end.<predicate>
             | edge.<predicate>
             | rel.<predicate>
             | return=root|end|edge
             | limit=<int>
             | offset=<int>
             | order=<order-clause>
             | format=<output-format>
```

Edge query terms:

```text
<edge-term> := <predicate>
             | from=<id>
             | to=<id>
             | limit=<int>
             | offset=<int>
             | order=<order-clause>
             | format=<output-format>
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

Traversal:

```text
<traversal-step> := <direction>:<link-name|*>:<end-model>
<direction>      := out | in | both
<link-name|*>    := <LinkName> | *
```

Traversal semantics:

- one or more `via=` terms may be chained to describe a multi-hop traversal
- root predicates still apply to the starting model named by `node.find`
- `end.<predicate>` applies to the final node reached by the traversal chain
- `edge.<predicate>` and `rel.<predicate>` apply to the final traversed relationship
- `return=end` returns the final node and is the default for traversal queries
- `return=root` returns the starting node after traversal filtering has been applied
- `return=edge` returns the final traversed relationship
- `return=...`, `end.*`, and `edge.*`/`rel.*` are only valid when at least one `via=` term is present
- `*` means "match any link type", but only when the runtime can resolve that hop unambiguously

Ordering:

```text
<order-clause> := <order-item>[,<order-item> ...]
<order-item>   := <field>:asc|desc
```

Output format:

```text
<output-format> := default | jsonl | table | graph
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
node.find User active=true order=age:desc,name:asc limit=10
edge.find Authored from=1 order=year:desc,to:asc
```

### Edge endpoint filtering

```text
edge.find Authored from=1
edge.find Authored to=2 year>=2024
```

### Mixed query examples

```text
node.find User name!="Alice Jones" active=true order=name:asc
node.find User name!="Alice Jones" active=true order=age:desc,name:asc
edge.find Authored from=1 year>=2024 order=year:desc,to:asc limit=10
```

### Traversal queries

```text
node.find User name="Alice Jones" via=out:Authored:Post
node.find User name="Alice Jones" via=out:Accessed:Post end.title="Draft Notes"
node.find User name="Alice Jones" via=out:Accessed:Post edge.accessedOn=2026-04-20 return=edge
node.find User name="Alice Jones" via=out:*:Post
node.find User name="Alice Jones" via=out:Authored:Post via=in:CommentedOn:Comment
node.find User name="Alice Jones" via=out:Accessed:Post end.title~"Draft" return=root
```

### Traversal result intent

```text
node.find User name="Alice Jones" via=out:Authored:Post
# returns matching Post end nodes

node.find User name="Alice Jones" via=out:Authored:Post return=root
# returns matching User root nodes

node.find User name="Alice Jones" via=out:Authored:Post return=edge
# returns matching Authored relationships from the final hop
```

### Output format selection

```text
node.find User age>=21
node.find User age>=21 format=default
node.find User age>=21 format=jsonl
node.find User age>=21 order=age:desc format=table
edge.find Authored from=1 format=jsonl
```

## Output Design

Default behavior:

- the current human-readable node/edge output remains the default for `find` queries
- `format=default` is explicit but optional
- `format=jsonl` and `format=table` are available now
- `format=` remains available so the CLI can grow toward later `graph` output without changing query syntax
- `graph` should remain reserved for graph-shaped or traversal-shaped results
- coloured output should be layered onto the default and table renderers without changing query semantics

Renderer model:

- query execution should return a structured result value first
- rendering should happen as a separate step based on `format=...`
- that split should let default, `jsonl`, `table`, and later `graph` share the same execution path

## Output Mockups

### Default node output

```text
2 nodes matched model 'User'.
Node User userId=2 {name="Bob", age=43, active=true}
Node User userId=5 {name="Carol", age=41, active=false}
```

### Default edge output

```text
1 edge matched link 'Authored'.
Edge Authored authoredId=3 from=1 to=2 {year=2024}
```

### `jsonl` node output

```text
{"kind":"node","model":"User","id":2,"labels":["User"],"props":{"name":"Bob","age":43,"active":true}}
{"kind":"node","model":"User","id":5,"labels":["User"],"props":{"name":"Carol","age":41,"active":false}}
```

### `jsonl` edge output

```text
{"kind":"edge","model":"Authored","id":3,"from":1,"to":2,"type":"Authored","props":{"year":2024}}
```

### `table` node output

```text
+--------+-------------+-----+--------+
| userId | name        | age | active |
+--------+-------------+-----+--------+
| 2      | Bob         | 43  | true   |
| 5      | Carol       | 41  | false  |
+--------+-------------+-----+--------+
```

### Future `graph` output

```text
(User#1 {name="Alice"})
  |
  +--[Authored#3 {year=2024}]--> (Post#2 {title="Hello"})
```

### Next presentation work

- graph output for graph-shaped and traversal-shaped results
- coloured output for interactive terminals
- clear non-colour behavior when output is piped or redirected

## Reserved Query Terms

These should remain reserved inside `find` commands:

- `limit`
- `offset`
- `order`
- `format`
- `via`
- `return`
- `from`
- `to`

`from` and `to` are special only for edge queries.
`end.` / `edge.` / `rel.` are special prefixes only for traversal-capable node queries.

## Parser Expectations

The parser should:

- preserve quoted values as a single token
- support escaped quotes inside quoted strings
- distinguish parser errors from query validation errors
- reject malformed order clauses clearly
- reject malformed multi-order clauses clearly
- reject unknown output formats clearly
- reject unknown fields clearly

Examples of invalid input:

```text
node.find User user name="Alice"
node.find User age>>
node.find User order=age
node.find User order=age:desc,name
node.find User format=xml
node.find User name="Alice
```

## Test Expectations

Implementation work should include acceptance-style tests in `tests/runtime_session.rs` covering:

- equality and inequality
- numeric comparisons
- `contains` matching via `~`
- quoted values with spaces
- ordering
- multi-field ordering
- limit and offset
- output format selection with the current human-readable output as the default
- edge `from` / `to` with additional predicates
- invalid syntax and invalid field errors

The current test scaffolding for those planned cases lives in `tests/query_language_mockups.rs`.
