#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <certificate-pem>" >&2
  exit 2
fi

cert="$1"
if command -v grm-cert-fingerprint >/dev/null 2>&1; then
  grm-cert-fingerprint "$cert"
else
  openssl x509 -in "$cert" -outform DER |
    openssl dgst -sha256 -binary |
    od -An -tx1 -v |
    tr -d ' \n'
  printf '\n'
fi
