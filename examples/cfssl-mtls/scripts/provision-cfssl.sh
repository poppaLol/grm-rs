#!/bin/sh
set -eu

CERT_DIR="${CERT_DIR:-/certs}"
CONFIG_DIR="${CONFIG_DIR:-/config}"
mkdir -p "$CERT_DIR" "$CONFIG_DIR"

cd "$CERT_DIR"

cat > ca-csr.json <<'JSON'
{
  "CN": "GRM local demo CA",
  "key": { "algo": "rsa", "size": 2048 },
  "names": [
    { "O": "GRM local demo only" }
  ]
}
JSON

cat > ca-config.json <<'JSON'
{
  "signing": {
    "default": { "expiry": "24h" },
    "profiles": {
      "server": {
        "expiry": "24h",
        "usages": ["signing", "key encipherment", "server auth"]
      },
      "client": {
        "expiry": "24h",
        "usages": ["signing", "key encipherment", "client auth"]
      }
    }
  }
}
JSON

cat > server-csr.json <<'JSON'
{
  "CN": "localhost",
  "hosts": ["localhost", "grm-secured", "127.0.0.1"],
  "key": { "algo": "rsa", "size": 2048 },
  "names": [
    { "O": "GRM local demo only" }
  ]
}
JSON

cat > mapped-client-csr.json <<'JSON'
{
  "CN": "mapped-cli",
  "hosts": [""],
  "key": { "algo": "rsa", "size": 2048 },
  "names": [
    { "O": "GRM local demo only" }
  ]
}
JSON

cat > limited-client-csr.json <<'JSON'
{
  "CN": "limited-cli",
  "hosts": [""],
  "key": { "algo": "rsa", "size": 2048 },
  "names": [
    { "O": "GRM local demo only" }
  ]
}
JSON

cat > unmapped-client-csr.json <<'JSON'
{
  "CN": "unmapped-cli",
  "hosts": [""],
  "key": { "algo": "rsa", "size": 2048 },
  "names": [
    { "O": "GRM local demo only" }
  ]
}
JSON

cfssl gencert -initca ca-csr.json | cfssljson -bare ca
cfssl gencert -ca=ca.pem -ca-key=ca-key.pem -config=ca-config.json -profile=server server-csr.json | cfssljson -bare server
cfssl gencert -ca=ca.pem -ca-key=ca-key.pem -config=ca-config.json -profile=client mapped-client-csr.json | cfssljson -bare mapped-client
cfssl gencert -ca=ca.pem -ca-key=ca-key.pem -config=ca-config.json -profile=client limited-client-csr.json | cfssljson -bare limited-client
cfssl gencert -ca=ca.pem -ca-key=ca-key.pem -config=ca-config.json -profile=client unmapped-client-csr.json | cfssljson -bare unmapped-client

# Demo sidecar containers run as the image's non-root user and need to read the
# generated local keys from the shared named volume. The docs call out that this
# is local-demo material only.
chmod 644 ./*-key.pem ca-key.pem
echo "Generated local demo CA, server certificate, and client certificates."
