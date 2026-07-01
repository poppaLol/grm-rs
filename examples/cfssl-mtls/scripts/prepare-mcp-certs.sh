#!/bin/sh
set -eu

SOURCE_CERT_DIR="${SOURCE_CERT_DIR:-/source-certs}"
MCP_CERT_DIR="${MCP_CERT_DIR:-/mcp-certs}"
MCP_SMOKE_CERT_DIR="${MCP_SMOKE_CERT_DIR:-/mcp-smoke-certs}"

mkdir -p "$MCP_CERT_DIR" "$MCP_SMOKE_CERT_DIR"

cp "$SOURCE_CERT_DIR/ca.pem" "$MCP_CERT_DIR/ca.pem"
cp "$SOURCE_CERT_DIR/mapped-client.pem" "$MCP_CERT_DIR/mapped-client.pem"
cp "$SOURCE_CERT_DIR/mapped-client-key.pem" "$MCP_CERT_DIR/mapped-client-key.pem"

cp "$SOURCE_CERT_DIR/ca.pem" "$MCP_SMOKE_CERT_DIR/ca.pem"
cp "$SOURCE_CERT_DIR/mapped-client.pem" "$MCP_SMOKE_CERT_DIR/mapped-client.pem"
cp "$SOURCE_CERT_DIR/mapped-client-key.pem" "$MCP_SMOKE_CERT_DIR/mapped-client-key.pem"
cp "$SOURCE_CERT_DIR/limited-client.pem" "$MCP_SMOKE_CERT_DIR/limited-client.pem"
cp "$SOURCE_CERT_DIR/limited-client-key.pem" "$MCP_SMOKE_CERT_DIR/limited-client-key.pem"
cp "$SOURCE_CERT_DIR/unmapped-client.pem" "$MCP_SMOKE_CERT_DIR/unmapped-client.pem"
cp "$SOURCE_CERT_DIR/unmapped-client-key.pem" "$MCP_SMOKE_CERT_DIR/unmapped-client-key.pem"

chown -R grm:grm "$MCP_CERT_DIR" "$MCP_SMOKE_CERT_DIR"
chmod 0444 "$MCP_CERT_DIR"/*.pem "$MCP_SMOKE_CERT_DIR"/*.pem
chmod 0400 "$MCP_CERT_DIR"/*-key.pem "$MCP_SMOKE_CERT_DIR"/*-key.pem

echo "Prepared narrowed MCP certificate volumes."
