#!/bin/sh
set -eu

OUT_DIR="${OUT_DIR:-.grm/secured-local}"
FORCE="${FORCE:-0}"
PRINCIPAL="${PRINCIPAL:-admin-1}"
PREFIX="${PREFIX:-admin-1}"
mkdir -p "$OUT_DIR"

cert="$OUT_DIR/$PREFIX.crt"
key="$OUT_DIR/$PREFIX.key"
csr="$OUT_DIR/$PREFIX.csr"
ext="$OUT_DIR/$PREFIX.ext"

if [ -e "$cert" ] || [ -e "$key" ]; then
  if [ "$FORCE" != "1" ]; then
    echo "Reusing existing $PREFIX client certificate material in $OUT_DIR"
    exit 0
  fi
  rm -f "$cert" "$key" "$csr" "$ext"
fi

cat > "$ext" <<'EOF'
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature,keyEncipherment
extendedKeyUsage=clientAuth
EOF

openssl req -newkey rsa:2048 -nodes \
  -keyout "$key" \
  -out "$csr" \
  -subj "/CN=$PRINCIPAL/O=GRM secured local only" >/dev/null 2>&1

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
echo "Generated $PREFIX client certificate at $cert"
