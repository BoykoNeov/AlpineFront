"""Rebuild the Col di Lana summit as it was before the Italian mine of 17 April 1916.

Today's elevation data shows the mine's crater (an oblong double crater about 35 x 25 m and 12 m
deep, per Striffler). At 10 m detail it appears as an oblong hollow about 6 m deep, 30 to 50 m
north-north-west of today's high point. This fills that hollow with the smoothest surface that meets
the ground around it, never lowering any point. See docs/decisions.md, decision 28.

`prepare_terrain.py` calls `rebuild()` every time it builds the map, always on the untouched
elevation data. After changing a number below, run prepare_terrain.py again (it keeps the
downloaded tiles in its work folder).
"""

import numpy as np

# Centre of the filled oval, in metres east and north of the summit grid point (the map centre).
CRATER_EAST_M = -15.0
CRATER_NORTH_M = 47.0
# Half-widths of the oval. A little larger than the recorded crater (35 x 25 m) because the 10 m
# grid smears the hole over the neighbouring points.
CRATER_HALF_EAST_M = 20.0
CRATER_HALF_NORTH_M = 26.0

# How much higher than today's high point the summit stood before the blast. No source gives a
# height from before the war, so this is 0 (decision 28). If one turns up, set it here: a rounded cap
# centred between today's high point and the crater is raised until the top reaches today's high
# point plus this many metres. These centre and radius values are the ones measured for decision 28
# (10 m: all tests pass, two by under a metre). Any change to the ground means re-measuring the tests.
RAISE_TOP_M = 0.0
RAISE_CENTRE_EAST_M = -15.0
RAISE_CENTRE_NORTH_M = 32.0
RAISE_RADIUS_M = 80.0

FILL_ITERATIONS = 4000


def _offsets(shape: tuple[int, int], spacing: float) -> tuple[np.ndarray, np.ndarray]:
    rows, cols = shape
    yy, xx = np.mgrid[0:rows, 0:cols]
    return (xx - cols // 2) * spacing, (yy - rows // 2) * spacing


def rebuild(grid: np.ndarray, spacing: float) -> np.ndarray:
    """Returns the rebuilt grid (rows south to north, summit at the centre point), as float32."""
    g = grid.astype(np.float64)
    east, north = _offsets(g.shape, spacing)
    crater = (((east - CRATER_EAST_M) / CRATER_HALF_EAST_M) ** 2
              + ((north - CRATER_NORTH_M) / CRATER_HALF_NORTH_M) ** 2) <= 1.0
    # Laplace fill: each point inside the oval becomes the average of its four neighbours, repeated
    # until it settles. The edge of the oval is held at the surrounding ground.
    h = g.copy()
    for _ in range(FILL_ITERATIONS):
        avg = 0.25 * (np.roll(h, 1, 0) + np.roll(h, -1, 0) + np.roll(h, 1, 1) + np.roll(h, -1, 1))
        h[crater] = avg[crater]
    h = np.maximum(h, g)

    if RAISE_TOP_M > 0.0:
        rows, cols = g.shape
        top = g[rows // 2, cols // 2] + RAISE_TOP_M
        d = np.hypot(east - RAISE_CENTRE_EAST_M, north - RAISE_CENTRE_NORTH_M)
        w = np.where(d < RAISE_RADIUS_M, np.cos(np.clip(d / RAISE_RADIUS_M, 0, 1) * np.pi / 2) ** 2, 0.0)
        lo, hi = 0.0, 200.0
        for _ in range(60):  # find the lift whose resulting highest point is exactly `top`
            mid = (lo + hi) / 2
            if (h + w * mid).max(initial=0, where=w > 0) < top:
                lo = mid
            else:
                hi = mid
        h = h + w * lo

    return h.astype("<f4")

