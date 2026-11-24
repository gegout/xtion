"""Estimate temporal axial SD (mm) and lateral edge jitter (pixels) on static XTV1 data."""
import argparse
import json
import struct
from pathlib import Path

import numpy as np


def frames(path):
    with path.open("rb") as source:
        if source.read(9) != b"XTVISION\1":
            raise ValueError("expected XTV1")
        while length := source.read(4):
            meta = json.loads(source.read(struct.unpack("<I", length)[0]))
            if not meta["metric"]:
                raise ValueError("metric recording required")
            shape = (meta["height"], meta["width"])
            yield np.frombuffer(source.read(2 * int(np.prod(shape))), "<u2").reshape(shape)


def edge_positions(first, axis):
    # Sample strong, isolated step edges, away from invalid pixels, at baseline.
    image = first if axis == 1 else first.T
    rows, cols = np.where((image[:, :-1] > 0) & (image[:, 1:] > 0)
                          & (np.abs(np.diff(image.astype(float), axis=1)) > 250))
    keep = (cols >= 4) & (cols < image.shape[1] - 5)
    rows, cols = rows[keep][::8], cols[keep][::8]
    near, far = image[rows, cols], image[rows, cols + 1]
    return rows, cols, (near.astype(float) + far) / 2, near < far, np.minimum(near, far)


def measure(path):
    edges, offsets = [], [[], []]
    for n, depth in enumerate(frames(path), 1):
        if n == 1:
            count = np.zeros(depth.shape, np.int64)
            total = np.zeros(depth.shape)
            squares = np.zeros(depth.shape)
            edges = [edge_positions(depth, axis) for axis in [0, 1]]
        valid = depth > 0
        count += valid
        total += depth
        squares += np.square(depth, dtype=float)
        for axis, (rows, cols, middle, sign, _) in enumerate(edges):
            image = depth if axis == 1 else depth.T
            strip = image[rows[:, None], cols[:, None] + np.arange(-3, 5)]
            sides = strip > middle[:, None]
            good = ((sides[:, 1:] != sides[:, :-1]) & (sides[:, 1:] == sign[:, None])
                    & (strip[:, 1:] > 0) & (strip[:, :-1] > 0))
            unique = good.sum(axis=1) == 1
            offsets[axis].append(np.where(unique, good.argmax(axis=1) - 3, np.nan))
    if n < 200:
        raise ValueError("need at least 200 frames")
    z = total / np.maximum(count, 1) / 1000
    sd = np.sqrt(np.maximum(0, squares - total * z * 1000) / np.maximum(1, count - 1))
    eligible = count >= .9 * n
    bins = []
    for low in np.arange(0.5, 4., .25):
        mask = eligible & (z >= low) & (z < low + .25)
        if mask.sum() >= 100:
            bins.append([float(np.median(z[mask])), float(np.median(sd[mask])), int(mask.sum())])
    if len(bins) < 3:
        raise ValueError("insufficient occupied distance bins")
    zs, ys, _ = np.array(bins).T
    b, a = np.polyfit(zs**2, ys, 1)
    residual = ys - a - b * zs**2
    denom = ((ys - ys.mean())**2).sum()
    r2 = float(1 - residual @ residual / denom) if denom else None
    lateral = []
    for edge, values in zip(edges, offsets):
        arr = np.stack(values)
        keep = np.isfinite(arr).sum(axis=0) >= .9*n
        std = np.nanstd(arr[:, keep], axis=0, ddof=1)
        depths = edge[-1][keep] / 1000
        lateral.append([dict(range_m=[lo,lo+1], edges=int(mask.sum()), median_sd_px=float(np.median(std[mask])))
                        for lo in [0.5,1.5,2.5] if (mask := (depths>=lo)&(depths<lo+1)).any()])
    return dict(frames=n, eligible_pixels=int(eligible.sum()), bins_z_m_sd_mm_count=bins,
                a_mm=a, b_mm_per_m2=b, r_squared=r2, rmse_mm=float(np.sqrt(np.mean(residual**2))),
                edge_jitter_y_x=lateral)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recording", type=Path)
    print(json.dumps(measure(parser.parse_args().recording), indent=2, allow_nan=False))
