#!/usr/bin/env -S uv run --quiet --python 3.12 python3
"""Build a committed test fixture from the full NASA Exoplanet Archive CSV.

Reads ``data/catalog/exoplanet_archive.csv`` and writes the subset of
rows whose host matches one of ten well-known systems to
``crates/ymir-catalog/fixtures/exoplanet_sample.csv``. That fixture is
committed and used by ``ymir-catalog`` unit tests so CI does not need
to fetch the full catalog.

Systems picked: single-planet, multi-planet, transit-only,
radial-velocity, direct-imaging, microlensing -- a spread that exercises
the parser and lookup code paths without dragging in the ~0.8 MB full
catalog.

Run ``scripts/fetch_exoplanet_catalog.py`` first to populate the full
catalog. This script is idempotent: it overwrites the fixture on each
run.
"""

from __future__ import annotations

import csv
import os
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FULL = REPO_ROOT / "data/catalog/exoplanet_archive.csv"
FIXTURE = REPO_ROOT / "crates/ymir-catalog/fixtures/exoplanet_sample.csv"

# Ten systems selected to cover a range of detection methods and
# observational histories:
#   * Proxima Centauri  -- nearest exoplanet host; radial velocity
#   * Tau Ceti          -- Gaia-cross-matched G8V; radial velocity
#   * TRAPPIST-1        -- 7-planet transit system, benchmark multi-planet
#   * Kepler-186        -- first Earth-sized HZ planet; transit
#   * KOI-351 (Kepler-90) -- 8-planet transit system. The Archive stores
#     the hostname as "KOI-351" rather than "Kepler-90"; we rely on the
#     shared Gaia DR3 ID to link them when the CAT-08 facade wires aliases.
#   * TOI-700           -- TESS-discovered HZ terrestrial; transit
#   * HD 10180          -- Sun-like; rich radial-velocity system
#   * 55 Cancri         -- binary host with multi-planet RV detections
#   * HR 8799           -- direct imaging of multi-planet system
#   * PDS 70            -- direct imaging of still-forming planets
SYSTEMS = {
    "Proxima Cen",
    "tau Cet",
    "TRAPPIST-1",
    "Kepler-186",
    "KOI-351",
    "TOI-700",
    "HD 10180",
    "55 Cnc",
    "HR 8799",
    "PDS 70",
}


def main() -> int:
    """Entry point."""
    os.chdir(REPO_ROOT)
    if not FULL.exists():
        print(
            f"error: {FULL} not found; run scripts/fetch_exoplanet_catalog.py first",
            file=sys.stderr,
        )
        return 1

    FIXTURE.parent.mkdir(parents=True, exist_ok=True)

    matched: list[dict[str, str]] = []
    with FULL.open(newline="") as src:
        reader = csv.DictReader(src)
        fieldnames = reader.fieldnames
        if fieldnames is None:
            print("error: source CSV has no header", file=sys.stderr)
            return 1
        for row in reader:
            if row.get("hostname", "").strip() in SYSTEMS:
                matched.append(row)

    if not matched:
        print("error: no matching hosts found in source CSV", file=sys.stderr)
        return 1

    # Sort by (hostname, planet letter) so the fixture is a stable diff.
    matched.sort(key=lambda r: (r.get("hostname", ""), r.get("pl_letter", "")))

    with FIXTURE.open("w", newline="") as dst:
        writer = csv.DictWriter(dst, fieldnames=fieldnames, quoting=csv.QUOTE_MINIMAL)
        writer.writeheader()
        for row in matched:
            writer.writerow(row)

    size = FIXTURE.stat().st_size
    hosts = sorted({r.get("hostname", "") for r in matched})
    print(
        f"wrote {len(matched)} rows to {FIXTURE.relative_to(REPO_ROOT)} "
        f"({size} bytes); hosts: {hosts}",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
