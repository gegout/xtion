"""Compare the two largest replay regions with measured depths in a fixed, empty scene."""

import argparse
import json
import tempfile
import warnings
from pathlib import Path

import numpy as np

from scripts.spatial_noise import frames
from .extract import extract


def clear_depth(path):
    # The replay instrumentation reports membership on the four-pixel grid.
    stack = np.stack([frame[::4, ::4] for frame in frames(path)]).astype(float)
    stack[stack == 0] = np.nan
    with warnings.catch_warnings():
        warnings.filterwarnings("ignore", message="All-NaN slice encountered")
        depth = np.nanmedian(stack, axis=0)
    depth[np.isfinite(stack).mean(axis=0) < 0.9] = np.nan
    return depth.ravel()


def support(members, raw, reference, noise):
    observed, clear = raw[members], reference[members]
    valid = np.isfinite(clear) & (observed > 0)
    delta = clear[valid] - observed[valid]
    tolerance = 3 * (noise["a_mm"] + noise["b_mm_per_m2"] * (clear[valid] / 1000) ** 2)
    return dict(samples=len(members), comparable=int(valid.sum()),
                near=int((delta > 250).sum()), background=int((np.abs(delta) <= tolerance).sum()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recordings", type=Path)
    parser.add_argument("clip", help="name of the XTV clip without its extension")
    parser.add_argument("clear", type=Path, help="clear XTV clip from the same camera pose")
    parser.add_argument("--frames", type=int, nargs="+", required=True)
    parser.add_argument("--revisions", nargs="+", required=True)
    args = parser.parse_args()
    reference = clear_depth(args.clear)
    noise_file = Path(__file__).resolve().parents[2] / "vision/config/spatial-noise.json"
    noise = json.loads(noise_file.read_text())
    labels = {"clips": {args.clip: {"frames": list(map(str, args.frames))}}}
    with tempfile.TemporaryDirectory() as tmp:
        output = Path(tmp)
        masks = {}
        for revision in args.revisions:
            extract(revision, args.recordings.resolve(), output, labels)
            lines = (output / f"{args.clip}-{revision}.jsonl").read_text().splitlines()
            masks[revision] = dict(map(json.loads, lines))
        result = []
        for index, frame in enumerate(frames(args.recordings / f"{args.clip}.xtv")):
            if index not in args.frames:
                continue
            raw = frame[::4, ::4].ravel()
            if raw.shape != reference.shape:
                raise ValueError("Empty-scene grid differs from the occupied recording")
            row = {"index": index}
            for revision, readings in masks.items():
                regions = sorted(readings[index], key=lambda item: len(item[1]), reverse=True)
                total = sum(len(members) for _, members in regions)
                row[revision] = dict(
                    dominant=bool(regions and 5 * len(regions[0][1]) >= 4 * total),
                    regions=[dict(id=identity, **support(members, raw, reference, noise))
                             for identity, members in regions[:2]],
                )
            result.append(row)
            if index == max(args.frames):
                break
        if len(result) != len(set(args.frames)):
            raise ValueError("Requested frame is outside the recording")
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
