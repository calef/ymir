#!/usr/bin/env -S uv run --quiet --with requests --with pyarrow --with pandas --python 3.12 python3
"""Fetch a pre-filtered Gaia DR3 working catalog for Ymir.

Downloads stars within 100 parsecs of Sol with well-characterized
parallax, T_eff, and luminosity, and writes the result to
``data/catalog/gaia_dr3_100pc.parquet``.

Catalog: Gaia DR3 main source table via the ESA Gaia archive TAP service.
Citation: Gaia Collaboration 2023, A&A 674, A1 (DR3 data release paper).

The ADQL filter follows ARCHITECTURE.md §6.1:

    parallax > 10                 (distance < 100 pc)
    parallax_over_error > 5       (parallax error < 20%)
    teff_gspphot IS NOT NULL      (known effective temperature)
    lum_flame IS NOT NULL         (known luminosity)

This produces roughly 250-400k rows depending on current DR3 revisions.

Usage
-----
From the repo root::

    ./scripts/fetch_gaia_catalog.py

The script is idempotent. It skips the download if the target Parquet
file already exists and is at least 100 MB. Force a refresh by deleting
``data/catalog/gaia_dr3_100pc.parquet`` first.

Manual fallback
---------------
If the async TAP job fails (ESA archive maintenance, network issues),
you can submit the same ADQL interactively at

    https://gea.esac.esa.int/archive/

pick "Advanced (ADQL)", paste the query printed by this script, and
download the result as CSV or Parquet. Place the file at the expected
path.
"""

from __future__ import annotations

import argparse
import io
import os
import sys
import time
import xml.etree.ElementTree as ET
from pathlib import Path

import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq
import requests

TAP_BASE = "https://gea.esac.esa.int/tap-server/tap"
#   Gaia DR3 splits astrophysical parameters across two tables:
#     gaiadr3.gaia_source                -- astrometry (parallax, ra, dec)
#                                           plus GSP-Phot T_eff and [M/H]
#     gaiadr3.astrophysical_parameters   -- FLAME luminosity, mass, radius, age
#
#   We join on source_id so rows without FLAME results fall out naturally.
ADQL = """
SELECT
    gs.source_id,
    gs.ra,
    gs.dec,
    gs.parallax,
    gs.parallax_error,
    gs.teff_gspphot,
    ap.lum_flame,
    ap.mh_gspphot,
    ap.mass_flame,
    ap.radius_flame,
    ap.age_flame
FROM gaiadr3.gaia_source AS gs
JOIN gaiadr3.astrophysical_parameters AS ap
  ON gs.source_id = ap.source_id
WHERE gs.parallax > 10
  AND gs.parallax_over_error > 5
  AND gs.teff_gspphot IS NOT NULL
  AND ap.lum_flame IS NOT NULL
""".strip()

DEFAULT_OUT = Path("data/catalog/gaia_dr3_100pc.parquet")
MIN_EXPECTED_BYTES = 20 * 1024 * 1024  # 20 MB sanity floor


def submit_async_job(adql: str) -> str:
    """Submit an async TAP job and return the job id."""
    resp = requests.post(
        f"{TAP_BASE}/async",
        data={
            "REQUEST": "doQuery",
            "LANG": "ADQL",
            "FORMAT": "csv",
            "PHASE": "RUN",
            "QUERY": adql,
        },
        allow_redirects=False,
        timeout=60,
    )
    if resp.status_code not in (200, 303):
        raise RuntimeError(f"Submit failed: {resp.status_code} {resp.text[:500]}")
    # Location header contains the job URL; id is the trailing path segment.
    location = resp.headers.get("Location", "")
    if not location:
        # Some TAP servers return the job UWS document in the body.
        root = ET.fromstring(resp.text)
        job_id = root.findtext(".//{*}jobId") or ""
        if not job_id:
            raise RuntimeError(
                f"Could not extract job id from response: {resp.text[:500]}"
            )
        return job_id
    return location.rsplit("/", 1)[-1]


def poll_job(job_id: str, poll_seconds: int = 10) -> str:
    """Block until the job finishes, return the final phase."""
    phase_url = f"{TAP_BASE}/async/{job_id}/phase"
    deadline = time.time() + 3600  # 1 h cap
    last = None
    while time.time() < deadline:
        r = requests.get(phase_url, timeout=30)
        phase = r.text.strip()
        if phase != last:
            print(f"[gaia] phase={phase}", file=sys.stderr, flush=True)
            last = phase
        if phase in ("COMPLETED", "ERROR", "ABORTED"):
            return phase
        time.sleep(poll_seconds)
    raise RuntimeError("Job polling timed out after 1 h")


def fetch_results(job_id: str) -> bytes:
    """Download the CSV results for a completed job."""
    url = f"{TAP_BASE}/async/{job_id}/results/result"
    r = requests.get(url, timeout=600, stream=True)
    r.raise_for_status()
    buf = io.BytesIO()
    total = 0
    next_log = 16 * 1024 * 1024
    for chunk in r.iter_content(chunk_size=1 << 20):
        buf.write(chunk)
        total += len(chunk)
        if total >= next_log:
            print(
                f"[gaia] downloaded {total / 1e6:.1f} MB",
                file=sys.stderr,
                flush=True,
            )
            next_log = total + 16 * 1024 * 1024
    print(f"[gaia] total bytes: {total}", file=sys.stderr, flush=True)
    return buf.getvalue()


def csv_to_parquet(csv_bytes: bytes, out_path: Path) -> int:
    """Convert Gaia CSV output to Parquet. Returns the row count."""
    df = pd.read_csv(io.BytesIO(csv_bytes))
    # Normalize column dtypes: source_id is a signed int64 in Gaia CSV but
    # logically a u64 identifier. Keep as int64 on the Python side since
    # Gaia source ids fit in 63 bits.
    for col in (
        "ra",
        "dec",
        "parallax",
        "parallax_error",
        "teff_gspphot",
        "lum_flame",
        "mh_gspphot",
        "mass_flame",
        "radius_flame",
        "age_flame",
    ):
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors="coerce")
    table = pa.Table.from_pandas(df, preserve_index=False)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(table, out_path, compression="zstd")
    return len(df)


def main() -> int:
    """Entry point."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=DEFAULT_OUT,
        help="Output Parquet path (default: data/catalog/gaia_dr3_100pc.parquet)",
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
            f"[gaia] {out} already exists ({out.stat().st_size} bytes); "
            "skipping download. Delete it to force a refresh.",
            file=sys.stderr,
        )
        return 0

    # Run from repo root so relative paths work regardless of invocation cwd.
    repo_root = Path(__file__).resolve().parent.parent
    os.chdir(repo_root)

    print(f"[gaia] submitting async TAP job to {TAP_BASE}", file=sys.stderr)
    print(f"[gaia] ADQL:\n{ADQL}\n", file=sys.stderr)
    job_id = submit_async_job(ADQL)
    print(f"[gaia] job id: {job_id}", file=sys.stderr)

    phase = poll_job(job_id)
    if phase != "COMPLETED":
        # Best-effort error detail from UWS document.
        try:
            err = requests.get(
                f"{TAP_BASE}/async/{job_id}/error", timeout=30
            ).text
        except requests.RequestException:
            err = "(could not fetch error document)"
        raise SystemExit(f"[gaia] job ended in phase {phase}: {err[:800]}")

    print("[gaia] fetching results", file=sys.stderr)
    csv_bytes = fetch_results(job_id)
    rows = csv_to_parquet(csv_bytes, out)
    print(
        f"[gaia] wrote {rows} rows to {out} "
        f"({out.stat().st_size / 1e6:.1f} MB)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
