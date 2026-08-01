#!/bin/sh
set -eu

OUT_DIR="${OUT_DIR:-.grm/secured-local}"
FORCE="${FORCE:-0}"
mkdir -p "$OUT_DIR"

cert="$OUT_DIR/server.crt"
key="$OUT_DIR/server.key"
csr="$OUT_DIR/server.csr"
ext="$OUT_DIR/server.ext"

if [ -e "$cert" ] || [ -e "$key" ]; then
  if [ "$FORCE" != "1" ]; then
    echo "Reusing existing server certificate material in $OUT_DIR"
    exit 0
  fi
  rm -f "$cert" "$key" "$csr" "$ext"
fi

cat > "$ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:localhost,DNS:grm-secured,DNS:grm-secured-local,IP:127.0.0.1
EOF

openssl req -newkey rsa:2048 -nodes \
  -keyout "$key" \
  -out "$csr" \
  -subj "/CN=localhost/O=GRM secured local only" >/dev/null 2>&1

openssl x509 -req \
  -in "$csr" \
  -CA "$OUT_DIR/ca.crt" \
  -CAkey "$OUT_DIR/ca.key" \
  -CAcreateserial \
  -out "$cert" \
  -days "${GRM_SECURED_LOCAL_CERT_DAYS:-365}" \
  -sha256 \
  -extfile "$ext" >/dev/null 2>&1

chmod 600 "$key"
chmod 644 "$cert"
echo "Generated server certificate at $cert"
