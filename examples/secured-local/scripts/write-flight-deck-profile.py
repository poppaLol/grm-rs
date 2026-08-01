#!/usr/bin/env python3
"""Write safe non-secret secured-local profile metadata for flight-deck use."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--endpoint", default="https://127.0.0.1:50051")
    parser.add_argument("--principal", default="admin-1")
    parser.add_argument("--access-level", default="owner-bootstrap-admin")
    parser.add_argument("--security-config", required=True)
    parser.add_argument("--ca-cert", required=True)
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.output.exists() and not args.force:
        raise SystemExit(f"{args.output} exists; pass --force to overwrite")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    profile = {
        "version": "secured-local-flight-deck-profile-v1",
        "mode": "secured_local_mtls",
        "endpoint": args.endpoint,
        "principal": args.principal,
        "access_level_template": args.access_level,
        "security_config": args.security_config,
        "tls_ca_cert": args.ca_cert,
        "tls_domain_name": "localhost",
        "gateway_hint": "use a trusted local gateway or CLI environment for client certificate and key material",
        "private_key_handling": "not-present-browser-must-not-import-or-cache-private-keys",
    }
    args.output.write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Wrote flight-deck profile metadata to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
