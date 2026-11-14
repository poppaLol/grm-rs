#!/usr/bin/env bash
set -euo pipefail
umask 077

if [[ "${1:-}" == --help ]]; then
  echo 'Usage: NEO4J_CONTAINER=... GRM_CONTAINER=... GRM_SCHEMA_TEMPLATE=... GRM_BACKUP_ROOT=... bash scripts/backup-migration-containers.sh'
  echo 'Stops each container temporarily for a cold archive; requires GRM_BACKUP_ALLOW_STOP=1.'
  echo 'Optional: GRM_BACKUP_INCLUDE_INSPECT=1 includes potentially secret Docker metadata.'
  exit 0
fi
[[ $# == 0 ]] || { echo 'Unexpected arguments; use --help' >&2; exit 1; }
: "${NEO4J_CONTAINER:?Set the source Neo4j container}"
: "${GRM_CONTAINER:?Set the GRM container}"
: "${GRM_SCHEMA_TEMPLATE:?Set the schema-session JSON path}"
: "${GRM_BACKUP_ROOT:?Set a private backup directory outside the repository}"
[[ "${GRM_BACKUP_ALLOW_STOP:-0}" == 1 ]] || { echo 'Set GRM_BACKUP_ALLOW_STOP=1 to permit temporary service downtime' >&2; exit 1; }
[[ "$NEO4J_CONTAINER" != "$GRM_CONTAINER" ]] || { echo 'Containers must be different' >&2; exit 1; }
[[ -f "$GRM_SCHEMA_TEMPLATE" ]] || { echo 'Schema file missing' >&2; exit 1; }
for command in docker gzip tar sha256sum mktemp; do command -v "$command" >/dev/null; done

restart_needed=''
cleanup() {
  local status=$?
  trap - EXIT
  if [[ -n "$restart_needed" ]]; then
    if ! docker start "$restart_needed" >/dev/null; then
      echo "Restart failed: run docker start $restart_needed" >&2
      status=1
    fi
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Resolve immutable container IDs and verify mounts before stopping anything.
neo4j_id="$(docker inspect --format '{{.Id}}' "$NEO4J_CONTAINER")"
grm_id="$(docker inspect --format '{{.Id}}' "$GRM_CONTAINER")"
[[ "$neo4j_id" != "$grm_id" ]] || { echo 'Container aliases resolve to the same container' >&2; exit 1; }
for container in "$neo4j_id" "$grm_id"; do
  state="$(docker inspect --format '{{.State.Running}} {{.State.Paused}}' "$container")"
  [[ "$state" == 'true false' ]] || { echo 'Both containers must be running and unpaused' >&2; exit 1; }
  docker exec "$container" sh -c 'command -v tar >/dev/null && command -v gzip >/dev/null'
done
for pair in "$neo4j_id:/data" "$grm_id:/workspaces"; do
  container="${pair%:*}"; directory="${pair##*:}"
  mount="$(docker inspect --format "{{range .Mounts}}{{if eq .Destination \"$directory\"}}{{.Type}}{{end}}{{end}}" "$container")"
  [[ "$mount" == volume || "$mount" == bind ]] || { echo "Missing storage mount at $directory" >&2; exit 1; }
done

mkdir -p -- "$GRM_BACKUP_ROOT"
run_dir="$(mktemp -d "$GRM_BACKUP_ROOT/migration-XXXXXXXX")"
mkdir -- "$run_dir/schema" "$run_dir/metadata"
for suffix in '' .bak .log; do
  if [[ -f "$GRM_SCHEMA_TEMPLATE$suffix" ]]; then
    cp -- "$GRM_SCHEMA_TEMPLATE$suffix" "$run_dir/schema/schema.json$suffix"
  fi
done
chmod 600 "$run_dir"/schema/*

backup_container() {
  local container="$1" directory="$2" name="$3" image
  image="$(docker inspect --format '{{.Image}}' "$container")"
  printf '%s\n' "$image" > "$run_dir/metadata/$name.image-id.txt"
  if [[ "${GRM_BACKUP_INCLUDE_INSPECT:-0}" == 1 ]]; then
    docker inspect "$container" > "$run_dir/metadata/$name.inspect.json"
    docker image inspect "$image" > "$run_dir/metadata/$name.image.json"
  fi
  restart_needed="$container"
  docker stop --time 60 "$container" >/dev/null
  docker run --rm --network none --user 0 --entrypoint /bin/sh \
    --volumes-from "$container:ro" "$image" \
    -c 'tar --numeric-owner -czf - -C "$1" .' sh "$directory" \
    > "$run_dir/$name.tar.gz.partial"
  docker start "$container" >/dev/null
  restart_needed=''
  gzip -t "$run_dir/$name.tar.gz.partial"
  tar -tzf "$run_dir/$name.tar.gz.partial" > "$run_dir/metadata/$name.archive-files.txt"
  mv -- "$run_dir/$name.tar.gz.partial" "$run_dir/$name.tar.gz"
}
backup_container "$neo4j_id" /data neo4j-data
backup_container "$grm_id" /workspaces grm-workspaces
(
  cd -- "$run_dir"
  sha256sum ./*.tar.gz schema/* metadata/* > SHA256SUMS.txt
  sha256sum -c SHA256SUMS.txt
)
touch "$run_dir/COMPLETE"
echo "Backup complete: $run_dir"
echo 'Archives contain private data and are not encrypted. Verify recovery in isolated containers before relying on them.'
