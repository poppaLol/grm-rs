#!/usr/bin/env bash

# Usage: scripts/index_inspection.sh
# Override setup script with: PLAYGROUND_SCRIPT=path/to/file.grm scripts/index_inspection.sh
set -euo pipefail

PLAYGROUND_SCRIPT="${PLAYGROUND_SCRIPT:-query_playground.grm}"
COMMAND_FILE="$(mktemp)"
trap 'rm -f "$COMMAND_FILE"' EXIT

cat >"$COMMAND_FILE" <<'GRM_COMMANDS'
session.indexes
session.indexes --verbose
session.describe --verbose

session.explain --verbose node.find User name="Alice Jones"
session.profile --verbose node.find User name="Alice Jones"

session.explain --verbose node.find User age>30
session.profile --verbose node.find User age>30

session.explain --verbose node.find User name="Alice Jones" via=out:Authored:Post
session.profile --verbose node.find User name="Alice Jones" via=out:Authored:Post

session.explain --verbose node.find User name="Bob Smith" via=in:Knows:User
session.profile --verbose node.find User name="Bob Smith" via=in:Knows:User

session.explain --verbose node.find User name="Bob Smith" via=both:Knows:User
session.profile --verbose node.find User name="Bob Smith" via=both:Knows:User

session.explain --verbose edge.find Authored from=1
session.profile --verbose edge.find Authored from=1

session.explain --verbose edge.find Authored to=4
session.profile --verbose edge.find Authored to=4

session.explain --verbose edge.find Authored from=1 to=4
session.profile --verbose edge.find Authored from=1 to=4

session.explain --verbose edge.find Authored authoredOn=2026-04-10
session.profile --verbose edge.find Authored authoredOn=2026-04-10

session.exit
GRM_COMMANDS

echo "== GRM index inspection command batch =="
sed 's/^/  /' "$COMMAND_FILE"
echo
echo "== Running playground setup: $PLAYGROUND_SCRIPT =="

cargo run --bin grm -- session --script "$PLAYGROUND_SCRIPT" <"$COMMAND_FILE"
