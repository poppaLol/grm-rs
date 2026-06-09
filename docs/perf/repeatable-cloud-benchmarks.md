# Repeatable Cloud And VPS Benchmarks

WorkSlice 221 establishes a provider-neutral runner and provenance envelope for
repeating GRM Criterion benchmarks on a known VPS or cloud machine. The purpose
is to preserve trustworthy evidence and a clean performance re-entry point. It
does not establish hosted durability or prove that GRM outperforms another
database.

## What The Machine Must Provide

Choose a stable, dedicated Linux VM or bare-metal VPS with:

- a published CPU or instance shape that can be provisioned again
- fixed vCPU and memory allocation
- local SSD or clearly identified network-backed storage
- enough disk for a fresh Rust build and isolated Criterion output
- Rust, Cargo, Git, Python 3.10+, a C/C++ build toolchain, and `pkg-config`
- no shared GRM project-memory database or mounted project-memory volume
- no unrelated production workload running during measurement

Record the provider, region, availability zone when applicable, and exact
instance type. Avoid burstable instances for comparison runs unless CPU-credit
state is also controlled and reported. Prefer the same image, kernel, storage
class, power/performance policy, and instance shape across runs.

Cloud metadata is supplied explicitly to the runner. It does not call provider
metadata endpoints, require cloud credentials, or infer billing/product details
from the host.

## Safety Boundary

The runner refuses to start without `--confirm-disposable`. This flag means:

- the machine or benchmark target is isolated for the run
- no configured database contains shared GRM project memory/SOML
- benchmark cleanup may affect only runner-owned temporary files and workspace
  roots

Do not pass the flag for an ambiguous Neo4j, Postgres, Mongo, GRM service, Docker
volume, workspace root, or machine. A backup is not permission to overwrite
shared memory.

The current checked-in profiles use embedded state or temporary GRM workspace
roots. Future comparator profiles must preserve the same explicit isolation
contract.

## First VPS Run

From a clean checkout at the commit being measured:

```bash
python3 scripts/cloud_benchmark.py local-grpc-mtls \
  --provider hetzner \
  --region nbg1 \
  --availability-zone nbg1-dc3 \
  --instance-type cpx31 \
  --target-description "disposable GRM benchmark VM" \
  --storage-description "160 GB provider SSD volume" \
  --confirm-disposable
```

Use the real provider values. The names above demonstrate the required shape;
they are not a recommended provider or instance.

Available profiles:

- `embedded-quick`
- `local-grpc-insecure`
- `local-grpc-mtls`
- `persistence`

Additional Criterion arguments can follow `--`:

```bash
python3 scripts/cloud_benchmark.py local-grpc-mtls \
  --provider example \
  --region example-1 \
  --instance-type dedicated-4c-16g \
  --target-description "disposable benchmark VM" \
  --storage-description "200 GB local NVMe" \
  --confirm-disposable \
  -- --sample-size 20 --measurement-time 10
```

The runner gives each execution an isolated `CARGO_TARGET_DIR`. This keeps
Criterion artifacts and build output under one run directory and prevents
historical local Criterion state from being mistaken for the current VPS run.
Compilation time is not part of Criterion's operation measurements.

The runner rejects a dirty Git checkout by default. `--allow-dirty` exists for
local metadata validation and exploratory runs, but those results should not be
used as public or regression evidence.

## Provenance Envelope

Each run is written under:

```text
target/benchmark-runs/<timestamp>-<profile>-<commit>/
```

The directory contains:

- `provenance.json`: machine-readable run envelope
- `benchmark.log`: combined Cargo and Criterion output
- `cargo-target/criterion/`: Criterion estimates and reports

The envelope records:

- GRM commit, branch, describe value, and dirty-worktree state
- exact benchmark command and profile
- benchmark line, TLS mode, persistence format, and deterministic dataset shape
- provider, region, availability zone, instance type, and target description
- explicit provider storage class/size
- OS, kernel, architecture, CPU topology/model/frequency data, governors, RAM,
  swap, disk capacity, and detected virtualization
- Rust, Cargo, and Python versions
- selected non-secret benchmark/build environment variables
- disposable-target confirmation, timestamps, exit status, and artifact paths

Private keys, cloud credentials, arbitrary environment variables, and service
data are not captured.

Validate metadata collection without compiling or running a benchmark:

```bash
python3 scripts/cloud_benchmark.py local-grpc-mtls \
  --provider local \
  --region local \
  --instance-type validation \
  --target-description "disposable metadata validation" \
  --storage-description "local validation disk" \
  --confirm-disposable \
  --allow-dirty \
  --collect-only
```

## Interpreting Runs

Compare runs only when their provenance is materially compatible. CPU model,
instance shape, storage class, kernel, Rust toolchain, commit, benchmark command,
dataset, TLS mode, and persistence format can all change results.

The secured `local-grpc-mtls` profile is the credible GRM service baseline.
`local-grpc-insecure` remains local transport evidence, while `embedded-quick`
is an engine/local-runtime line. Do not merge these into one headline.

This platform creates repeatable evidence. Public comparator claims still need
fair indexes, equivalent operation intent, isolated comparator databases,
versioned comparator environments, repeated runs, and review of the resulting
data.
