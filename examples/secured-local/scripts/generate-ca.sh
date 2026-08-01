#!/bin/sh
set -eu

OUT_DIR="${OUT_DIR:-.grm/secured-local}"
FORCE="${FORCE:-0}"
mkdir -p "$OUT_DIR"

cert="$OUT_DIR/ca.crt"
key="$OUT_DIR/ca.key"

if [ -e "$cert" ] || [ -e "$key" ]; then
  if [ "$FORCE" != "1" ]; then
    echo "Reusing existing local CA material in $OUT_DIR"
    exit 0
  fi
  rm -f "$cert" "$key" "$OUT_DIR/ca.srl"
fi

openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout "$key" \
  -out "$cert" \
  -days "${GRM_SECURED_LOCAL_CERT_DAYS:-365}" \
  -subj "/CN=GRM Secured Local CA/O=GRM secured local only" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1

chmod 600 "$key"
chmod 644 "$cert"
echo "Generated local CA certificate at $cert"
