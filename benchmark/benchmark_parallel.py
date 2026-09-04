#!/usr/bin/env python3
"""Benchmark sequential vs parallel mutant verification.

The script runs `bcore-mutation analyze` for one or more cases, records wall
clock time, and writes a CSV plus PNG charts. It copies the input SQLite
database before every run so each case starts from the same mutant state.
"""

from __future__ import annotations

import argparse
import csv
import os
import platform
import shutil
import sqlite3
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Case:
    label: str
    parallel: int
    jobs: int


@dataclass
class ResultRow:
    case: str
    repeat: int
    parallel: int
    jobs: int
    seconds: float
    exit_code: int
    status: str
    killed: int | None
    survived: int | None
    errors: int | None
    db_path: Path
    stdout_log: Path
    stderr_log: Path


def parse_case(value: str) -> Case:
    parts = value.split(":")
    if len(parts) != 3:
        raise argparse.ArgumentTypeError(
            "case must use LABEL:PARALLEL:JOBS, for example parallel-3x3:3:3"
        )

    label, parallel_raw, jobs_raw = parts
    if not label:
        raise argparse.ArgumentTypeError("case label cannot be empty")

    try:
        parallel = int(parallel_raw)
        jobs = int(jobs_raw)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("parallel and jobs must be integers") from exc

    if parallel < 1:
        raise argparse.ArgumentTypeError("parallel must be at least 1")
    if jobs < 0:
        raise argparse.ArgumentTypeError("jobs must be at least 0")

    return Case(label=label, parallel=parallel, jobs=jobs)


def parser() -> argparse.ArgumentParser:
    argument_parser = argparse.ArgumentParser(
        description="Benchmark bcore-mutation analyze sequential and parallel runs."
    )
    argument_parser.add_argument("--db", default="mutation.db", help="SQLite DB path")
    argument_parser.add_argument("--run-id", required=True, type=int, help="Mutation run ID")
    argument_parser.add_argument(
        "--command",
        required=True,
        help="Command passed to bcore-mutation analyze --command",
    )
    argument_parser.add_argument(
        "--setup-command",
        default=None,
        help="Optional command passed to bcore-mutation analyze --setup-command",
    )
    argument_parser.add_argument(
        "--timeout",
        type=int,
        default=300,
        help="Timeout in seconds per mutant",
    )
    argument_parser.add_argument(
        "--case",
        dest="cases",
        action="append",
        type=parse_case,
        required=True,
        help="Benchmark case as LABEL:PARALLEL:JOBS. Repeat for multiple cases.",
    )
    argument_parser.add_argument(
        "--repeats",
        type=int,
        default=3,
        help="Number of repetitions per case",
    )
    argument_parser.add_argument(
        "--output-dir",
        default="benchmark-results",
        help="Directory for copied DBs, logs, CSV, and charts",
    )
    argument_parser.add_argument(
        "--bcore-mutation-bin",
        default="bcore-mutation",
        help="Path to bcore-mutation binary inside the benchmark environment",
    )
    argument_parser.add_argument(
        "--project",
        default=None,
        help="Optional --project value passed to analyze",
    )
    argument_parser.add_argument(
        "--file-path",
        default=None,
        help="Optional --file-path value passed to analyze",
    )
    argument_parser.add_argument(
        "--extra-analyze-arg",
        action="append",
        default=[],
        help="Extra argument passed through to analyze. Repeat as needed.",
    )
    argument_parser.add_argument(
        "--keep-going",
        action="store_true",
        help="Continue remaining cases when one run fails",
    )
    argument_parser.add_argument(
        "--no-charts",
        action="store_true",
        help="Only write CSV and logs",
    )
    return argument_parser


def run_git(args: list[str]) -> str:
    completed = subprocess.run(
        ["git", *args],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if completed.returncode != 0:
        return "unknown"
    return completed.stdout.strip() or "unknown"


def count_mutants(db_path: Path, run_id: int) -> tuple[int | None, int | None, int | None]:
    try:
        with sqlite3.connect(db_path) as connection:
            killed = connection.execute(
                "SELECT COUNT(*) FROM mutants WHERE run_id = ? AND status = 'killed'",
                (run_id,),
            ).fetchone()[0]
            survived = connection.execute(
                "SELECT COUNT(*) FROM mutants WHERE run_id = ? AND status = 'survived'",
                (run_id,),
            ).fetchone()[0]
            errors = connection.execute(
                "SELECT COUNT(*) FROM mutants WHERE run_id = ? AND status = 'error'",
                (run_id,),
            ).fetchone()[0]
            return killed, survived, errors
    except sqlite3.Error:
        return None, None, None


def analyze_command(args: argparse.Namespace, case: Case, db_path: Path) -> list[str]:
    command = [
        args.bcore_mutation_bin,
        "analyze",
        "--sqlite",
        str(db_path),
        "--run-id",
        str(args.run_id),
        "--timeout",
        str(args.timeout),
        "--parallel",
        str(case.parallel),
        "--jobs",
        str(case.jobs),
        "--command",
        args.command,
    ]
    if args.project:
        command.extend(["--project", args.project])
    if args.file_path:
        command.extend(["--file-path", args.file_path])
    if args.setup_command:
        command.extend(["--setup-command", args.setup_command])
    command.extend(args.extra_analyze_arg)
    return command


def run_case(args: argparse.Namespace, case: Case, repeat: int, output_dir: Path) -> ResultRow:
    run_name = f"{repeat:02d}-{case.label}"
    db_path = output_dir / "db" / f"{run_name}.db"
    stdout_log = output_dir / "logs" / f"{run_name}.stdout.log"
    stderr_log = output_dir / "logs" / f"{run_name}.stderr.log"

    shutil.copy2(args.db, db_path)
    command = analyze_command(args, case, db_path)

    print(
        f"Running {case.label} repeat {repeat}: "
        f"--parallel {case.parallel} --jobs {case.jobs}",
        flush=True,
    )
    start = time.perf_counter()
    completed = subprocess.run(
        command,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    seconds = time.perf_counter() - start

    stdout_log.write_text(completed.stdout, encoding="utf-8")
    stderr_log.write_text(completed.stderr, encoding="utf-8")
    killed, survived, errors = count_mutants(db_path, args.run_id)
    status = "ok" if completed.returncode == 0 else "failed"

    print(
        f"Finished {case.label} repeat {repeat}: "
        f"{seconds:.2f}s status={status}",
        flush=True,
    )
    return ResultRow(
        case=case.label,
        repeat=repeat,
        parallel=case.parallel,
        jobs=case.jobs,
        seconds=seconds,
        exit_code=completed.returncode,
        status=status,
        killed=killed,
        survived=survived,
        errors=errors,
        db_path=db_path,
        stdout_log=stdout_log,
        stderr_log=stderr_log,
    )


def write_csv(rows: list[ResultRow], path: Path) -> None:
    with path.open("w", newline="", encoding="utf-8") as csv_file:
        writer = csv.DictWriter(
            csv_file,
            fieldnames=[
                "case",
                "repeat",
                "parallel",
                "jobs",
                "seconds",
                "exit_code",
                "status",
                "killed",
                "survived",
                "errors",
                "db_path",
                "stdout_log",
                "stderr_log",
            ],
        )
        writer.writeheader()
        for row in rows:
            writer.writerow(
                {
                    "case": row.case,
                    "repeat": row.repeat,
                    "parallel": row.parallel,
                    "jobs": row.jobs,
                    "seconds": f"{row.seconds:.6f}",
                    "exit_code": row.exit_code,
                    "status": row.status,
                    "killed": row.killed,
                    "survived": row.survived,
                    "errors": row.errors,
                    "db_path": row.db_path,
                    "stdout_log": row.stdout_log,
                    "stderr_log": row.stderr_log,
                }
            )


def grouped_ok_rows(rows: list[ResultRow]) -> dict[str, list[ResultRow]]:
    grouped: dict[str, list[ResultRow]] = {}
    for row in rows:
        if row.status == "ok":
            grouped.setdefault(row.case, []).append(row)
    return grouped


def write_summary(rows: list[ResultRow], path: Path) -> None:
    grouped = grouped_ok_rows(rows)
    baseline = grouped.get("sequential") or next(iter(grouped.values()), [])
    baseline_median = (
        statistics.median(row.seconds for row in baseline) if baseline else None
    )

    with path.open("w", encoding="utf-8") as summary:
        summary.write("# bcore-mutation parallel benchmark\n\n")
        summary.write(f"- Commit: `{run_git(['rev-parse', 'HEAD'])}`\n")
        summary.write(f"- Host: `{platform.platform()}`\n")
        summary.write(f"- CPUs visible: `{os.cpu_count()}`\n\n")
        summary.write("| Case | Parallel | Jobs | Runs | Median seconds | Speedup |\n")
        summary.write("|------|----------|------|------|----------------|---------|\n")
        for case, case_rows in grouped.items():
            median_seconds = statistics.median(row.seconds for row in case_rows)
            speedup = (
                baseline_median / median_seconds
                if baseline_median and median_seconds > 0
                else None
            )
            first = case_rows[0]
            speedup_text = f"{speedup:.2f}x" if speedup is not None else "n/a"
            summary.write(
                f"| {case} | {first.parallel} | {first.jobs} | {len(case_rows)} | "
                f"{median_seconds:.2f} | {speedup_text} |\n"
            )


def write_charts(rows: list[ResultRow], output_dir: Path) -> None:
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError as exc:
        raise RuntimeError(
            "matplotlib is not installed; rerun with --no-charts or use the benchmark Docker image"
        ) from exc

    grouped = grouped_ok_rows(rows)
    if not grouped:
        return

    labels = list(grouped)
    medians = [statistics.median(row.seconds for row in grouped[label]) for label in labels]
    baseline = grouped.get("sequential") or next(iter(grouped.values()))
    baseline_median = statistics.median(row.seconds for row in baseline)
    speedups = [baseline_median / seconds for seconds in medians]

    fig, ax = plt.subplots(figsize=(9, 4.8))
    ax.bar(labels, medians)
    ax.set_ylabel("Median wall time (seconds)")
    ax.set_title("Mutation verification time")
    ax.tick_params(axis="x", rotation=20)
    fig.tight_layout()
    fig.savefig(output_dir / "time-by-case.png", dpi=160)
    plt.close(fig)

    fig, ax = plt.subplots(figsize=(9, 4.8))
    ax.bar(labels, speedups)
    ax.axhline(1.0, color="black", linewidth=1)
    ax.set_ylabel("Speedup vs sequential")
    ax.set_title("Parallel verification speedup")
    ax.tick_params(axis="x", rotation=20)
    fig.tight_layout()
    fig.savefig(output_dir / "speedup-by-case.png", dpi=160)
    plt.close(fig)


def main() -> int:
    args = parser().parse_args()
    if args.repeats < 1:
        print("--repeats must be at least 1", file=sys.stderr)
        return 2

    db_path = Path(args.db)
    if not db_path.exists():
        print(f"database not found: {db_path}", file=sys.stderr)
        return 2
    args.db = db_path

    output_dir = Path(args.output_dir)
    (output_dir / "db").mkdir(parents=True, exist_ok=True)
    (output_dir / "logs").mkdir(parents=True, exist_ok=True)

    subprocess.run(
        ["git", "config", "--global", "--add", "safe.directory", str(Path.cwd())],
        check=False,
    )

    rows: list[ResultRow] = []
    failed = False
    for repeat in range(1, args.repeats + 1):
        for case in args.cases:
            row = run_case(args, case, repeat, output_dir)
            rows.append(row)
            write_csv(rows, output_dir / "results.csv")
            write_summary(rows, output_dir / "summary.md")
            if row.status != "ok":
                failed = True
                if not args.keep_going:
                    print(
                        f"Stopping after failed run. See {row.stdout_log} and {row.stderr_log}.",
                        file=sys.stderr,
                    )
                    return row.exit_code or 1

    if not args.no_charts:
        write_charts(rows, output_dir)

    print(f"Wrote benchmark results to {output_dir}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
