#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: benchmark/run-in-docker.sh SUBJECT_REPO [benchmark args...]" >&2
  exit 2
fi

subject_repo=$1
shift

image=${BCORE_BENCH_IMAGE:-bcore-mutation-bench}
cpus=${BCORE_BENCH_CPUS:-10}
results_dir=${BCORE_BENCH_RESULTS:-"$(pwd)/benchmark-results"}

subject_repo=$(cd "$subject_repo" && pwd -P)
subject_parent=$(dirname "$subject_repo")
subject_name=$(basename "$subject_repo")
results_dir=$(mkdir -p "$results_dir" && cd "$results_dir" && pwd -P)

docker build -f benchmark/Dockerfile -t "$image" .

docker run --rm \
  --cpus="$cpus" \
  -v "$subject_parent:/bench" \
  -v "$results_dir:/results" \
  -w "/bench/$subject_name" \
  "$image" \
  --output-dir /results \
  "$@"

