# Migrating schema-declared Neo4j data to GRM

The `migrate_neo4j_to_grpc` example copies nodes and relationships covered by a
GRM runtime schema-session JSON file into a service-managed workspace. It reads
Neo4j without changing it. This is a schema-guided example, not an automatic
converter for arbitrary Neo4j databases: indexes, constraints, undeclared labels
and relationship types are not copied. Nodes matching multiple declared models,
undeclared properties, incompatible values and invalid endpoints fail preflight.
Custom labels/types differing from model names are unsupported.

## Backup

Pause other writers for the entire backup and migration window. For Docker
deployments, this optional helper archives `/data` and `/workspaces` with each
container stopped temporarily. It requires existing mounts, running/unpaused
containers, and `tar`/`gzip` in their images. It restarts a stopped container on
normal completion or a trapped error; machine failure or SIGKILL can still leave
it stopped. Shared volumes must have no other active writers.

```bash
export NEO4J_CONTAINER=source-neo4j
export GRM_CONTAINER=target-grm
export GRM_SCHEMA_TEMPLATE=/private/migration/schema.json
export GRM_BACKUP_ROOT=/private/migration/backups
GRM_BACKUP_ALLOW_STOP=1 bash scripts/backup-migration-containers.sh
```

Backup contents, schema files, reports and ID maps are private operational data.
Keep them outside the repository. The helper uses private permissions, not
encryption. Docker inspection metadata is omitted unless
`GRM_BACKUP_INCLUDE_INSPECT=1` is set; that metadata may contain credentials.
Checksums and archive validation do not prove recoverability: test restoration
in isolated containers with matching images and no production mounts.

## Preflight And Execute

Supply `NEO4J_URI`, `NEO4J_USER` and `NEO4J_PASSWORD` through your local secret
configuration. The example uses the backend's default Neo4j database; setting
`NEO4J_DATABASE` does not select a database. Prepare a GRM schema-session JSON
file describing the source models and fields.

To prepare a schema file, start an empty embedded session with
`env -u GRM_BACKEND cargo run --bin grm -- session`. Define models matching
your source labels, types and properties, then save the session as JSON. For a
fictional source with `User`, `Post` and `AUTHORED` records:

```text
model.define User userId name:string:required
model.define Post postId title:string:required
link.define AUTHORED User Post authoredId year:int:required
session.save --json /private/migration/schema.json
session.exit
```

The saved file supplies schema metadata; any graph records in it are not imported.
Start a separate local target service if needed, using a new storage directory:

```bash
cargo run -p grm-service-api --bin grm-local-workspace-server -- \
  127.0.0.1:50051 /private/migration/target-workspaces
```

With the source environment configured, run the importer from another terminal:

```bash
umask 077
cargo run -p grm-service-api --features neo4j --example migrate_neo4j_to_grpc -- \
  --schema /private/migration/schema.json \
  --endpoint http://127.0.0.1:50051 --workspace imported-demo \
  --dry-run --report /private/migration/preflight.json
```

Dry-run does not connect to or mutate the target service. Review its counts and
blocking issues against the intended source scope. It reads all declared source
models into memory; keep writers paused because these reads are not one source
snapshot. For a subset, pass `--selection` with JSON containing `node_ids` and
`edge_ids` arrays of source Neo4j internal IDs. Every selected relationship must
have both endpoints selected. Source IDs are remapped, not preserved as GRM IDs.

Use a new, unused workspace name for execution:

```bash
cargo run -p grm-service-api --features neo4j --example migrate_neo4j_to_grpc -- \
  --schema /private/migration/schema.json \
  --endpoint http://127.0.0.1:50051 --workspace imported-demo \
  --execute --report /private/migration/execution.json \
  --id-map /private/migration/id-map.json
```

The plaintext loopback endpoint is for local development. Secured deployments
must configure the existing GRM client TLS options and service permissions;
see [Service Security Design](security/security-design.md). Binary durability is the default;
`--format json` is available. `--mode open` is refused during execution unless
`--allow-open-target` is supplied; opening an existing workspace can append
duplicates and is unsuitable for ordinary migration.

Execution uses individual autocommit operations, not an atomic import. A failure
can leave a partially populated workspace and no final ID map. There is no
automatic rollback or resumable retry. Keep the source intact and use another
fresh workspace for a retry. Report target counts describe successful create
operations, not an independent read-back verification.

Before changing clients, independently check schema, counts, properties and
relationships using the ID map, then reopen the target after a service restart
and repeat verification. Maintain one active writer for the documented local
durability profile. Retain the source and verified backups for rollback; after
new target writes, rollback requires reconciling those writes.
