#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
test_dir="$(mktemp -d)"
trap 'rm -rf -- "$test_dir"' EXIT
mkdir "$test_dir/bin" "$test_dir/payload"
printf 'fictional data\n' > "$test_dir/payload/data"
printf '{}\n' > "$test_dir/schema.json"
cp "$root/scripts/tests/fixtures/migration-docker" "$test_dir/bin/docker"
chmod +x "$test_dir/bin/docker"
export PATH="$test_dir/bin:$PATH" MOCK_ROOT="$test_dir"
export NEO4J_CONTAINER=source GRM_CONTAINER=target
export GRM_SCHEMA_TEMPLATE="$test_dir/schema.json" GRM_BACKUP_ROOT="$test_dir/backups"
if bash "$root/scripts/backup-migration-containers.sh" > /dev/null 2>&1; then
  echo 'Backup unexpectedly allowed downtime without opt-in' >&2; exit 1
fi
[[ ! -e "$test_dir/docker.log" ]]
export GRM_BACKUP_ALLOW_STOP=1
bash "$root/scripts/backup-migration-containers.sh" > /dev/null
run_dirs=("$test_dir/backups"/migration-*)
[[ -f "${run_dirs[0]}/COMPLETE" ]]
[[ "$(stat -c %a "${run_dirs[0]}")" == 700 ]]
[[ "$(stat -c %a "${run_dirs[0]}/neo4j-data.tar.gz")" == 600 ]]
[[ ! -e "${run_dirs[0]}/metadata/neo4j-data.inspect.json" ]]
grep -qx 'start source-id' "$test_dir/docker.log"
grep -qx 'start target-id' "$test_dir/docker.log"
export MOCK_FAIL_ARCHIVE=1
if bash "$root/scripts/backup-migration-containers.sh" > /dev/null 2>&1; then
  echo 'Backup unexpectedly accepted failed archive' >&2; exit 1
fi
[[ "$(tail -n 1 "$test_dir/docker.log")" == 'start source-id' ]]
echo 'Backup helper tests passed (mock Docker only)'
