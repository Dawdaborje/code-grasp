#!/usr/bin/env python3
"""
Benchmark `cg search` vs `rg` on a fixed corpus (same queries, comparable globs).

Usage (from code-grasp repo root):
  BENCH_ROOT=/path/to/repo python3 scripts/bench_search.py
  BENCH_ITER=30 python3 scripts/bench_search.py   # default 30

Requires: `cargo`, `rg` (ripgrep), Python 3.10+.
"""

from __future__ import annotations

import csv
import math
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_BENCH_ROOT = "/home/borje/Documents/Personal/personal/arvora/arvora-os.git/os-distro"

QUERIES = [
    "pacman",
    "mkinitcpio",
    "linux-firmware",
    "base-devel",
    "archinstall",
    "GRUB",
    "systemd",
    "calamares",
    "airootfs profile",
    "HOOKS=",
]


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def parse_rust_str_array(block_name: str, src: str) -> list[str]:
    i = src.index(f"const {block_name}")
    j = src.index("];", i)
    block = src[i:j]
    return sorted(set(re.findall(r'"([a-z0-9.]+)"', block)))


def build_rg_globs(extensions: list[str], well_known: list[str]) -> list[str]:
    globs: list[str] = []
    for ext in extensions:
        globs.append(f"*.{ext}")
    for name in well_known:
        globs.append(name)
    return globs


def percentile_nearest_rank(sorted_vals: list[float], p: float) -> float:
    """Nearest-rank p in [0, 100]; sorted_vals sorted ascending."""
    if not sorted_vals:
        return 0.0
    n = len(sorted_vals)
    k = min(n, max(1, math.ceil(p / 100.0 * n)))
    return sorted_vals[k - 1]


def median_sorted(sorted_vals: list[float]) -> float:
    return float(statistics.median(sorted_vals))


def run_timed(cmd: list[str], cwd: Path | None, env: dict[str, str]) -> float:
    t0 = time.perf_counter()
    r = subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    dt = time.perf_counter() - t0
    if r.returncode != 0:
        raise RuntimeError(f"command failed ({r.returncode}): {' '.join(cmd)}")
    return dt


def main() -> int:
    root = repo_root()
    bench_root = Path(os.environ.get("BENCH_ROOT", DEFAULT_BENCH_ROOT)).resolve()
    n_iter = int(os.environ.get("BENCH_ITER", "30"))
    cargo_release = os.environ.get("BENCH_RELEASE", "1") not in ("0", "false", "no")

    if not (root / "Cargo.toml").is_file():
        print("Run from code-grasp repository root (Cargo.toml missing).", file=sys.stderr)
        return 1
    if not bench_root.is_dir():
        print(f"BENCH_ROOT not a directory: {bench_root}", file=sys.stderr)
        return 1

    gitignore = (root / "crates/cg-core/src/walker/gitignore.rs").read_text()
    extensions = parse_rust_str_array("BUILTIN_INDEX_EXTENSIONS", gitignore)
    well_known = parse_rust_str_array("WELL_KNOWN_TEXT_NAMES", gitignore)

    bench_dir = root / "bench"
    bench_dir.mkdir(exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    out_md = bench_dir / f"results-{ts}.md"
    out_csv = bench_dir / f"results-{ts}.csv"

    cg_prefix = [
        "cargo",
        "run",
        "-q",
    ]
    if cargo_release:
        cg_prefix.append("--release")
    cg_prefix += ["-p", "code-grasp", "--"]

    env = os.environ.copy()
    env.setdefault("RUST_LOG", "warn")

    print("Building `code-grasp` once (avoids timing `cargo` on first search)…")
    build_cmd = ["cargo", "build"]
    if cargo_release:
        build_cmd.append("--release")
    build_cmd += ["-q", "-p", "code-grasp"]
    subprocess.run(build_cmd, cwd=root, env=env, check=True)

    rows_csv: list[list[str]] = []

    def cg_cmd(*args: str) -> list[str]:
        return [*cg_prefix, *args]

    # --- Index once ---
    print(f"Indexing: {bench_root} …")
    t0 = time.perf_counter()
    subprocess.run(
        cg_cmd("index", str(bench_root)),
        cwd=root,
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=True,
    )
    index_s = time.perf_counter() - t0

    st = subprocess.run(
        cg_cmd("status", str(bench_root)),
        cwd=root,
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    status_out = st.stdout
    print(status_out)
    chunks = 0
    for line in status_out.splitlines():
        if line.startswith("chunks:"):
            chunks = int(line.split(":", 1)[1].strip())
    if chunks == 0:
        print(
            "ERROR: index has 0 chunks; benchmark search vs rg is meaningless. Fix walker/config.",
            file=sys.stderr,
        )
        return 2

    cg_data = bench_root / ".code-grasp"
    du_b = 0
    if cg_data.exists():
        du_b = sum(f.stat().st_size for f in cg_data.rglob("*") if f.is_file())

    rg_base = ["rg", "-F", "--hidden", "-j", str(os.cpu_count() or 4)]
    for g in build_rg_globs(extensions, well_known):
        rg_base.extend(["--iglob", g])

    summary: list[tuple[str, str, float, float, int, str]] = []

    for qi, query in enumerate(QUERIES):
        qid = f"Q{qi + 1}"
        cg_times: list[float] = []
        rg_times: list[float] = []

        for it in range(n_iter):
            # cg search
            dt = run_timed(
                cg_cmd("search", str(bench_root), query, "--limit", "10"),
                cwd=root,
                env=env,
            )
            cg_times.append(dt)
            rows_csv.append(["cg_search", qid, query, str(it + 1), f"{dt:.6f}"])

            # rg: literal fixed string; limit work with --max-count 1 per file? cg returns top 10 hits not files.
            # Match "work done" roughly: find files containing pattern (like first stage).
            dt = run_timed(
                rg_base + ["--max-count", "10", "--", query, str(bench_root)],
                cwd=None,
                env=env,
            )
            rg_times.append(dt)
            rows_csv.append(["rg", qid, query, str(it + 1), f"{dt:.6f}"])

        cg_s = sorted(cg_times)
        rg_s = sorted(rg_times)
        cg_med = median_sorted(cg_s)
        rg_med = median_sorted(rg_s)
        faster = (cg_med / rg_med) if rg_med > 0 else 0.0
        note = f"rg ~{faster:.0f}× faster wall (subprocess cg/embed each run)" if faster else ""
        summary.append(
            (
                qid,
                query[:40] + ("…" if len(query) > 40 else ""),
                median_sorted(cg_s),
                percentile_nearest_rank(cg_s, 95),
                n_iter,
                note,
            )
        )
        summary.append(
            (
                qid + "-rg",
                query[:40] + ("…" if len(query) > 40 else ""),
                median_sorted(rg_s),
                percentile_nearest_rank(rg_s, 95),
                n_iter,
                "ripgrep baseline",
            )
        )

    with out_csv.open("w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["method", "query_id", "query", "iter", "seconds"])
        w.writerows(rows_csv)

    uname = subprocess.run(["uname", "-sr"], capture_output=True, text=True).stdout.strip()
    try:
        cargo_v = subprocess.run(
            ["cargo", "--version"], capture_output=True, text=True, check=True
        ).stdout.strip()
    except Exception:
        cargo_v = "?"
    try:
        rg_v = subprocess.run(["rg", "--version"], capture_output=True, text=True, check=True)
        rg_v = rg_v.stdout.splitlines()[0] if rg_v.stdout else "?"
    except Exception:
        rg_v = "?"

    md_lines = [
        f"# CodeGrasp search benchmark ({ts})",
        "",
        "## Environment",
        "",
        f"- **BENCH_ROOT**: `{bench_root}`",
        f"- **Repo**: `{root}`",
        f"- **Iterations per query / tool**: {n_iter}",
        f"- **cg build**: `cargo run` {'`--release`' if cargo_release else '(debug)'}",
        f"- **{uname}**",
        f"- **{cargo_v}**",
        f"- **{rg_v}**",
        f"- **Index wall time**: {index_s:.3f}s",
        f"- **`.code-grasp/` size (sum of files)**: {du_b / 1024:.1f} KiB",
        f"- **rg globs**: {len(extensions)} extensions + {len(well_known)} well-known basenames (`--iglob` each)",
        "",
        "## Median / p95 (seconds)",
        "",
        "| query_id | tool | query (trim) | p50 (s) | p95 (s) | n | notes |",
        "|----------|------|--------------|---------|---------|---|-------|",
    ]

    for qid, qshort, p50, p95, n, note in summary:
        tool = "cg_search" if not qid.endswith("-rg") else "rg"
        qid_clean = qid.removesuffix("-rg")
        md_lines.append(
            f"| {qid_clean} | {tool} | `{qshort}` | {p50:.4f} | {p95:.4f} | {n} | {note} |"
        )

    md_lines += [
        "",
        "## How to read",
        "",
        "- **cg_search**: full subprocess `cg search ROOT QUERY --limit 10` (loads settings, ONNX query embed, hybrid search).",
        "- **rg**: `rg -F` with CodeGrasp-like `--iglob` filters; `--max-count 10` caps matches per file (rough analog to bounded hits).",
        "- **p95**: nearest-rank percentile on sorted samples (not interpolated).",
        "",
        f"Raw CSV: `{out_csv.relative_to(root)}`",
        "",
    ]
    out_md.write_text("\n".join(md_lines), encoding="utf-8")

    print(f"\nWrote {out_md.relative_to(root)}")
    print(f"Wrote {out_csv.relative_to(root)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
