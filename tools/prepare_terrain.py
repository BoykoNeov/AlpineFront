"""Cut the game map out of the TINITALY 10 m elevation model.

Downloads the two TINITALY 1.1 tiles that cover Col di Lana (about 170 MB, kept out of the repo),
merges them, crops a square centred on the summit and writes:

    data/terrain/col_di_lana.heights   raw little-endian f32 heights in metres, row by row,
                                       first row = southern edge, west to east
    data/terrain/col_di_lana.toml      size, spacing, real-world position and the data citation

Usage:
    python tools/prepare_terrain.py --work-dir W:/temp/claude/terrain [--size-m 6000]

Needs Python with numpy and rasterio.
"""

import argparse
import pathlib
import urllib.request
import zipfile

import numpy as np
import rasterio
from rasterio.merge import merge
from rasterio.warp import transform

TILES = ["w51570", "w51070"]
TILE_URL = "https://tinitaly.pi.ingv.it/data_1.1/{t}_s10/{t}_s10.zip"

# Col di Lana summit in the TINITALY grid (UTM zone 32N, a cell centre). Checked against
# Wikipedia's coordinates (46.4967 N, 11.9592 E); the model gives 2,460 m there.
SUMMIT_E = 727085.0
SUMMIT_N = 5153485.0
SPACING = 10.0

CITATION = (
    "Tarquini S., I. Isola, M. Favalli, A. Battistini, G. Dotta (2023). TINITALY, a digital "
    "elevation model of Italy with a 10 meters cell size (Version 1.1). Istituto Nazionale di "
    "Geofisica e Vulcanologia (INGV). https://doi.org/10.13127/tinitaly/1.1"
)


def fetch(work: pathlib.Path) -> list[pathlib.Path]:
    tifs = []
    for t in TILES:
        tif = work / f"{t}_s10" / f"{t}_s10.tif"
        if not tif.exists():
            zpath = work / f"{t}.zip"
            if not zpath.exists():
                print(f"downloading {t} ...")
                urllib.request.urlretrieve(TILE_URL.format(t=t), zpath)
            with zipfile.ZipFile(zpath) as z:
                z.extractall(work)
        tifs.append(tif)
    return tifs


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--work-dir", required=True, type=pathlib.Path)
    ap.add_argument("--size-m", type=float, default=6000.0)
    ap.add_argument("--out", type=pathlib.Path,
                    default=pathlib.Path(__file__).resolve().parent.parent / "data" / "terrain")
    args = ap.parse_args()
    args.work_dir.mkdir(parents=True, exist_ok=True)
    args.out.mkdir(parents=True, exist_ok=True)

    half = args.size_m / 2.0
    samples = int(round(args.size_m / SPACING)) + 1  # grid points, including both edges
    # Bounds of the cells whose centres are the grid points.
    west = SUMMIT_E - half - SPACING / 2
    south = SUMMIT_N - half - SPACING / 2
    east = SUMMIT_E + half + SPACING / 2
    north = SUMMIT_N + half + SPACING / 2

    srcs = [rasterio.open(p) for p in fetch(args.work_dir)]
    nodata = srcs[0].nodata
    data, _ = merge(srcs, bounds=(west, south, east, north), res=SPACING, nodata=nodata)
    grid = data[0]
    assert grid.shape == (samples, samples), grid.shape
    if nodata is not None and np.any(grid == nodata):
        raise SystemExit("crop contains missing data")

    # Raster rows run north to south; the game stores rows south to north.
    grid = np.flipud(grid).astype("<f4")
    centre = samples // 2
    # Settsass (2,575 m) is inside the map and higher, so check only the summit's surroundings.
    near = grid[centre - 50:centre + 51, centre - 50:centre + 51]
    assert abs(grid[centre, centre] - near.max()) < 0.01, "summit is not the local high point"

    name = "col_di_lana"
    (args.out / f"{name}.heights").write_bytes(grid.tobytes())
    sw_lon, sw_lat = transform("EPSG:32632", "EPSG:4326",
                               [SUMMIT_E - half], [SUMMIT_N - half])
    meta = f"""# Game terrain cut from TINITALY 1.1 by tools/prepare_terrain.py. Do not edit by hand.
name = "Col di Lana"
heights_file = "{name}.heights"
# Grid points per side. Point (0, 0) is the south-west corner; rows run south to north.
columns = {samples}
rows = {samples}
spacing_m = {SPACING}
# Real-world position of the south-west grid point (UTM zone 32N, EPSG:32632).
utm_zone = "32N"
south_west_easting = {SUMMIT_E - half}
south_west_northing = {SUMMIT_N - half}
south_west_latitude = {sw_lat[0]:.6f}
south_west_longitude = {sw_lon[0]:.6f}
lowest_m = {float(grid.min()):.2f}
highest_m = {float(grid.max()):.2f}
source = "TINITALY 1.1, tiles {', '.join(TILES)}"
licence = "CC BY 4.0"
citation = "{CITATION}"
"""
    (args.out / f"{name}.toml").write_text(meta, encoding="utf-8")
    print(f"wrote {samples}x{samples} grid, {grid.min():.0f} to {grid.max():.0f} m, to {args.out}")


if __name__ == "__main__":
    main()
