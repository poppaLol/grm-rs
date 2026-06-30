#!/bin/sh
set -eu

CERT_DIR="${CERT_DIR:-/certs}"
CONFIG_DIR="${CONFIG_DIR:-/config}"
mkdir -p "$CONFIG_DIR"

MAPPED_FINGERPRINT="$(grm-cert-fingerprint "$CERT_DIR/mapped-client.pem")"
LIMITED_FINGERPRINT="$(grm-cert-fingerprint "$CERT_DIR/limited-client.pem")"
printf '%s\n' "$MAPPED_FINGERPRINT" > "$CERT_DIR/mapped-client.sha256"
printf '%s\n' "$LIMITED_FINGERPRINT" > "$CERT_DIR/limited-client.sha256"

cat > "$CONFIG_DIR/security.json" <<JSON
{
  "certificate_mappings": [
    {
      "fingerprint_sha256": "$MAPPED_FINGERPRINT",
      "principal": { "issuer": "local-demo", "subject": "cli/full" }
    },
    {
      "fingerprint_sha256": "$LIMITED_FINGERPRINT",
      "principal": { "issuer": "local-demo", "subject": "cli/limited" }
    }
  ],
  "permission_table": {
    "version": "local-demo-policy-v1",
    "assignments": [
      {
        "principal": { "issuer": "local-demo", "subject": "cli/full" },
        "scope": { "kind": "service" },
        "permissions": [
          { "action": "workspace.create", "resource": { "kind": "service" } }
        ]
      },
      {
        "principal": { "issuer": "local-demo", "subject": "cli/full" },
        "scope": { "kind": "deployment_local_all_workspaces" },
        "permissions": [
          { "action": "schema.define", "resource": { "kind": "any_node_model" } },
          { "action": "schema.define", "resource": { "kind": "any_edge_model" } },
          { "action": "schema.inspect", "resource": { "kind": "workspace" } },
          { "action": "workspace.open", "resource": { "kind": "workspace" } },
          { "action": "workspace.close", "resource": { "kind": "workspace" } },
          { "action": "node.create", "resource": { "kind": "any_node_model" } },
          { "action": "node.read", "resource": { "kind": "any_node_model" } },
          { "action": "edge.create", "resource": { "kind": "any_edge_model" } },
          { "action": "edge.read", "resource": { "kind": "any_edge_model" } }
        ]
      },
      {
        "principal": { "issuer": "local-demo", "subject": "cli/limited" },
        "scope": { "kind": "service" },
        "permissions": [
          { "action": "workspace.create", "resource": { "kind": "service" } }
        ]
      }
    ]
  }
}
JSON

echo "Computed mapped client fingerprints and rendered security.json."
