#!/usr/bin/env python3
"""Write or verify non-secret secured-local bootstrap inputs."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--issuer", required=True)
    parser.add_argument("--principal", required=True)
    parser.add_argument("--access-level", required=True)
    parser.add_argument("--policy-version", required=True)
    parser.add_argument("--service-port", default="50051")
    parser.add_argument("--gateway-port", default="3001")
    parser.add_argument("--workspace", default="flight-deck-demo")
    parser.add_argument("--force", action="store_true")
    return parser.parse_args()


def expected_document(args: argparse.Namespace) -> dict[str, str]:
    return {
        "version": "secured-local-bootstrap-inputs-v1",
        "issuer": args.issuer,
        "principal": args.principal,
        "access_level": args.access_level,
        "policy_version": args.policy_version,
        "service_port": args.service_port,
        "gateway_port": args.gateway_port,
        "workspace": args.workspace,
    }


def main() -> int:
    args = parse_args()
    expected = expected_document(args)
    if args.output.exists() and not args.force:
        existing = json.loads(args.output.read_text(encoding="utf-8"))
        if existing != expected:
            existing_identity = f"{existing.get('issuer')}/{existing.get('principal')}"
            requested_identity = f"{args.issuer}/{args.principal}"
            raise SystemExit(
                "existing secured-local bootstrap uses "
                f"{existing_identity} with access level {existing.get('access_level')}; "
                "requested "
                f"{requested_identity} with access level {args.access_level}. "
                "Rerun with --force to replace generated local material."
            )
        print(f"Reusing existing {args.output}")
        return 0
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(expected, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"Wrote bootstrap input metadata to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
