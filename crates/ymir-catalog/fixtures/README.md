# `ymir-catalog` Test Fixtures

## `gaia_sample.parquet`

A 50-row slice of the pre-filtered Gaia DR3 working catalog, used by the unit
tests in `src/gaia.rs`. The fixture holds the 50 stars with the largest
parallax (i.e., the nearest stars) so the file is tiny (~11 KB) and exercises
the reader against a schema identical to the full catalog.

### How it was built

1. Run `scripts/fetch_gaia_catalog.py` from the repo root. That hits the ESA
   Gaia archive TAP service and writes the full working catalog to
   `data/catalog/gaia_dr3_100pc.parquet` (~10 MB, ~116k rows).
2. Run `scripts/build_gaia_fixture.py`. That script reads the full catalog,
   sorts descending by `parallax`, keeps the top 50 rows, and writes them to
   `crates/ymir-catalog/fixtures/gaia_sample.parquet` with Zstd compression.

### Schema

Identical to the full catalog:

| Column            | Type   | Source (Gaia DR3)                        |
|-------------------|--------|------------------------------------------|
| `source_id`       | int64  | `gaia_source.source_id`                  |
| `ra`              | double | `gaia_source.ra` (deg)                   |
| `dec`             | double | `gaia_source.dec` (deg)                  |
| `parallax`        | double | `gaia_source.parallax` (mas)             |
| `parallax_error`  | double | `gaia_source.parallax_error` (mas)       |
| `teff_gspphot`    | double | `gaia_source.teff_gspphot` (K)           |
| `lum_flame`       | double | `astrophysical_parameters.lum_flame` (L_sun) |
| `mh_gspphot`      | double | `astrophysical_parameters.mh_gspphot` (dex) |
| `mass_flame`      | double | `astrophysical_parameters.mass_flame` (M_sun) |
| `radius_flame`    | double | `astrophysical_parameters.radius_flame` (R_sun) |
| `age_flame`       | double | `astrophysical_parameters.age_flame` (Gyr) |

### Citation

Gaia Collaboration, Vallenari A., Brown A.G.A., Prusti T., et al. 2023,
*A&A* 674, A1 ("Gaia Data Release 3: Summary of the content and survey
properties"). Plus the ESA/Gaia mission paper (Gaia Collaboration 2016,
*A&A* 595, A1) when publishing derived results.

## `exoplanet_sample.csv`

A slice of the NASA Exoplanet Archive's `pscomppars` composite-parameters
table, used by the unit tests in `src/exoplanet.rs`. Holds every planet
record for ten well-known systems so the file stays tiny (~5.5 KB) while
exercising the parser against schema identical to the full catalog.

Systems included (as of 2026):

- Proxima Cen (1 planet; radial velocity)
- tau Cet (4 planets; radial velocity)
- TRAPPIST-1 (7 planets; transit)
- Kepler-186 (5 planets; transit)
- KOI-351 / Kepler-90 (8 planets; transit)
- TOI-700 (4 planets; transit)
- HD 10180 (6 planets; radial velocity)
- 55 Cnc (5 planets; radial velocity + transit)
- HR 8799 (4 planets; direct imaging)
- PDS 70 (2 planets; direct imaging)

### How it was built

1. Run `scripts/fetch_exoplanet_catalog.py` from the repo root. That
   hits the NASA Exoplanet Archive TAP service and writes the full
   CSV to `data/catalog/exoplanet_archive.csv` (~0.8 MB, ~6.2k rows).
2. Run `scripts/build_exoplanet_fixture.py`. That script reads the full
   catalog, filters on the ten hostnames above, sorts by
   `(hostname, pl_letter)`, and writes the result to
   `crates/ymir-catalog/fixtures/exoplanet_sample.csv`.

### Schema

Identical to the fetch script output (one row per planet):

| Column           | Type   | Source (pscomppars)                                    |
|------------------|--------|--------------------------------------------------------|
| `pl_name`        | string | Planet identifier, e.g. "TRAPPIST-1 b"                 |
| `hostname`       | string | Canonical host name                                    |
| `pl_letter`      | string | Planet letter (b, c, ...)                              |
| `discoverymethod`| string | Detection method                                       |
| `pl_orbper`      | double | Orbital period (days)                                  |
| `pl_orbsmax`     | double | Semi-major axis (AU)                                   |
| `pl_bmasse`      | double | Planet mass (Earth masses)                             |
| `pl_rade`        | double | Planet radius (Earth radii)                            |
| `disc_year`      | int    | Discovery year                                         |
| `gaia_id`        | string | Alias of `gaia_dr3_id`, e.g. "Gaia DR3 12345"          |

### Citation

NASA Exoplanet Archive, operated by the California Institute of
Technology under contract with NASA. DOI: `10.26133/NEA12`
(`pscomppars` table).
