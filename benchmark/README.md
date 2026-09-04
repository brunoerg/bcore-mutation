# Parallel Benchmark Harness

This harness compares `bcore-mutation analyze --parallel 1` with one or more
parallel configurations inside Docker. It records wall-clock time, keeps logs,
and generates charts.

## Build and Run

From this repository:

```bash
bash benchmark/run-in-docker.sh /path/to/bitcoin \
  --db mutation.db \
  --run-id 123 \
  --repeats 3 \
  --timeout 900 \
  --case sequential:1:3 \
  --case parallel-2x4:2:4 \
  --case parallel-3x3:3:3 \
  --case parallel-4x2:4:2 \
  --setup-command "cmake -B build_corecheck -DBUILD_FOR_FUZZING=ON && cmake --build build_corecheck -j3" \
  --command "FUZZ=coin_grinder_is_optimal ./build_corecheck/bin/fuzz ../qa-assets/fuzz_corpora/coin_grinder_is_optimal"
```

The Docker runner mounts the subject repository's parent directory at `/bench`.
That matters for Bitcoin Core fuzz benchmarks because a sibling directory such
as `../qa-assets` remains visible inside the container.

By default the runner limits Docker to 10 CPUs. Override it with:

```bash
BCORE_BENCH_CPUS=8 bash benchmark/run-in-docker.sh /path/to/bitcoin ...
```

## Outputs

Results are written to `benchmark-results/` by default:

- `results.csv`: one row per case/repeat.
- `summary.md`: median timing and speedup table.
- `time-by-case.png`: median wall-clock seconds.
- `speedup-by-case.png`: speedup relative to the `sequential` case.
- `logs/*.stdout.log` and `logs/*.stderr.log`: raw analyze output.
- `db/*.db`: copied SQLite database used for each run.

Each run uses a copy of the input SQLite database, so the original DB is not
modified by the benchmark.

## Case Format

Cases use this format:

```text
LABEL:PARALLEL:JOBS
```

For a 10 CPU machine, useful starting points are:

```text
sequential:1:3
parallel-2x4:2:4
parallel-3x3:3:3
parallel-4x2:4:2
```

`PARALLEL * JOBS` is the approximate maximum compiler/test parallelism. For
example, `parallel-3x3` can run about 9 build jobs across 3 active mutants.

