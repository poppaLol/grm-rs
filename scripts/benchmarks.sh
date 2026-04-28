#!/usr/bin/env bash
set -euo pipefail

case "${1:-all}" in
  all)
    cargo bench "${@:2}"
    ;;
  grm-vs-sqlite)
    cargo bench --bench grm_vs_sqlite "${@:2}"
    ;;
  persistence)
    cargo bench --bench persistence "${@:2}"
    ;;
  quick)
    cargo bench --bench grm_vs_sqlite property_lookup_1k -- --sample-size 10 --warm-up-time 1 --measurement-time 2 "${@:2}"
    ;;
  scaled)
    cargo bench --bench grm_vs_sqlite 'property_lookup|one_hop' -- --sample-size 10 --warm-up-time 1 --measurement-time 2 "${@:2}"
    ;;
  stress)
    GRM_BENCH_STRESS=1 cargo bench --bench grm_vs_sqlite 'property_lookup|one_hop' -- --sample-size 10 --warm-up-time 1 --measurement-time 2 "${@:2}"
    ;;
  check)
    cargo bench --no-run
    ;;
  *)
    echo "usage: scripts/benchmarks.sh [all|grm-vs-sqlite|persistence|quick|scaled|stress|check] [cargo bench args...]" >&2
    exit 2
    ;;
esac
