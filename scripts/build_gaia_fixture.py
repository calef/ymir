#!/usr/bin/env -S uv run --quiet --with pyarrow --with pandas --python 3.12 python3
"""Build a committed test fixture from the full Gaia DR3 working catalog.

Reads ``data/catalog/gaia_dr3_100pc.parquet`` and writes the 50 rows
with the largest parallax (i.e., the nearest stars) to
``crates/ymir-catalog/fixtures/gaia_sample.parquet``. That fixture is
committed and used by ``ymir-catalog`` unit tests so CI does not need
to fetch the full ~10 MB catalog.

Run ``scripts/fetch_gaia_catalog.py`` first to populate the full
catalog. This script is idempotent: it overwrites the fixture on each
run, which keeps it trivially reproducible from the logs.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path

import pyarrow.parquet as pq

REPO_ROOT = Path(__file__).resolve().parent.parent
FULL = REPO_ROOT / "data/catalog/gaia_dr3_100pc.parquet"
FIXTURE = REPO_ROOT / "crates/ymir-catalog/fixtures/gaia_sample.parquet"
N = 50


def main() -> int:
    """Entry point."""
    os.chdir(REPO_ROOT)
    if not FULL.exists():
        print(
            f"error: {FULL} not found; run scripts/fetch_gaia_catalog.py first",
            file=sys.stderr,
        )
        return 1
    table = pq.read_table(FULL)
    # Sort by descending parallax to surface the nearest stars, then
    # slice. ``take`` keeps arrow-native data types without a pandas
    # detour, so the fixture is a tiny file with the same schema as
    # the full catalog.
    parallax = table.column("parallax").to_pylist()
    indices = sorted(range(len(parallax)), key=lambda i: -parallax[i])[:N]
    sample = table.take(indices)
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(sample, FIXTURE, compression="zstd")
    size = FIXTURE.stat().st_size
    print(
        f"wrote {sample.num_rows} rows to {FIXTURE.relative_to(REPO_ROOT)} "
        f"({size} bytes)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
