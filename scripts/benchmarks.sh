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
  profile-insert)
    filter="${2:-insert_10k/grm_repo_bulk_transactions}"
    profile_time="${PROFILE_TIME:-10}"
    output="${FLAMEGRAPH_OUTPUT:-target/criterion/${filter}/flamegraph.svg}"
    mkdir -p "$(dirname "$output")"
    CARGO_PROFILE_BENCH_DEBUG="${CARGO_PROFILE_BENCH_DEBUG:-true}" GRM_BENCH_STRESS=1 GRM_BENCH_PROFILE_GRM_INSERT_ONLY=1 cargo bench --bench grm_vs_sqlite --no-run
    bench_bin="$(find target/release/deps -maxdepth 1 -type f -executable -name 'grm_vs_sqlite-*' -printf '%T@ %p\n' | sort -nr | head -n 1 | cut -d' ' -f2-)"
    GRM_BENCH_STRESS=1 GRM_BENCH_PROFILE_GRM_INSERT_ONLY=1 flamegraph --output "$output" -- "$bench_bin" "$filter" --profile-time "$profile_time" "${@:3}"
    ;;
  check)
    cargo bench --no-run
    ;;
  *)
    echo "usage: scripts/benchmarks.sh [all|grm-vs-sqlite|persistence|quick|scaled|stress|profile-insert|check] [cargo bench args...]" >&2
    exit 2
    ;;
esac
