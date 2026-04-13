#!/usr/bin/env -S uv run --quiet --with requests --python 3.12 python3
"""Fetch the NASA Exoplanet Archive composite-parameters catalog for Ymir.

Downloads the full ``pscomppars`` (Planetary Systems Composite Parameters)
table and writes it to ``data/catalog/exoplanet_archive.csv``. One row
per confirmed planet (~5,700 rows as of 2026).

Catalog: NASA Exoplanet Archive, operated by Caltech under contract with
NASA. TAP service endpoint:
``https://exoplanetarchive.ipac.caltech.edu/TAP/sync``.

Citation: DOI ``10.26133/NEA12`` (``pscomppars`` table).

The selected columns match what ``ymir_catalog::exoplanet::ExoplanetCatalog``
consumes:

    pl_name, hostname, pl_letter, discoverymethod,
    pl_orbper, pl_orbsmax, pl_bmasse, pl_rade,
    disc_year, gaia_id (aliased from ``gaia_dr3_id``)

Usage
-----
From the repo root::

    ./scripts/fetch_exoplanet_catalog.py

The script is idempotent. It skips the download if the target CSV
already exists and is at least 1 MB. Force a refresh by deleting
``data/catalog/exoplanet_archive.csv`` first.

Manual fallback
---------------
If the TAP endpoint is unreachable, you can run the same query
interactively at

    https://exoplanetarchive.ipac.caltech.edu/TAP/

paste the ADQL printed by this script, and download the result as CSV.
Place the file at the expected path.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

import requests

TAP_BASE = "https://exoplanetarchive.ipac.caltech.edu/TAP/sync"
#  The TAP ``pscomppars`` schema exposes Gaia cross-matches as
#  ``gaia_dr2_id`` / ``gaia_dr3_id`` (both are strings like
#  "Gaia DR3 12345"). We select DR3 and alias it to ``gaia_id`` so the
#  Rust parser can key on a stable column name regardless of any future
#  DR4 rename upstream.
ADQL = (
    "select pl_name,hostname,pl_letter,discoverymethod,"
    "pl_orbper,pl_orbsmax,pl_bmasse,pl_rade,disc_year,"
    "gaia_dr3_id as gaia_id "
    "from pscomppars"
)
DEFAULT_OUT = Path("data/catalog/exoplanet_archive.csv")
MIN_EXPECTED_BYTES = 500 * 1024  # 500 KB sanity floor (~6k rows at ~125 B each)


def fetch_csv(adql: str) -> bytes:
    """Run the sync TAP query and return the CSV bytes."""
    resp = requests.get(
        TAP_BASE,
        params={"query": adql, "format": "csv"},
        timeout=600,
        stream=True,
    )
    resp.raise_for_status()
    chunks = []
    total = 0
    next_log = 1 * 1024 * 1024
    for chunk in resp.iter_content(chunk_size=1 << 20):
        chunks.append(chunk)
        total += len(chunk)
        if total >= next_log:
            print(
                f"[exo] downloaded {total / 1e6:.1f} MB",
                file=sys.stderr,
                flush=True,
            )
            next_log = total + 1 * 1024 * 1024
    print(f"[exo] total bytes: {total}", file=sys.stderr, flush=True)
    return b"".join(chunks)


def main() -> int:
    """Entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help="Output CSV path (default: data/catalog/exoplanet_archive.csv)",
    )
    parser.add_argument(
        "--print-query",
        action="store_true",
        help="Print the ADQL query and exit.",
    )
    args = parser.parse_args()

    if args.print_query:
        print(ADQL)
        return 0

    out = args.out
    if out.exists() and out.stat().st_size >= MIN_EXPECTED_BYTES:
        print(
            f"[exo] {out} already exists ({out.stat().st_size} bytes); "
            "skipping download. Delete it to force a refresh.",
            file=sys.stderr,
        )
        return 0

    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)

    print(f"[exo] fetching sync TAP query from {TAP_BASE}", file=sys.stderr)
    print(f"[exo] ADQL: {ADQL}", file=sys.stderr)
    csv_bytes = fetch_csv(ADQL)

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(csv_bytes)
    print(
        f"[exo] wrote {out} ({out.stat().st_size / 1e6:.1f} MB)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
