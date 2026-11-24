"""Render a metric plane with a native hole on both paths and test projected pixels."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]


def check(output):
    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp) / "surface.xtv"
        with (ROOT / "vision/tests/fixtures/still.xtv").open("rb") as src:
            magic = src.read(9)
            meta = json.loads(src.read(struct.unpack("<I", src.read(4))[0]))
        w, h = 128, 96
        meta.update(width=w, height=h)
        meta["intrinsics"].update(cx=63.5, cy=47.5, fx=160, fy=160)
        encoded = json.dumps(meta).encode()
        plane = np.full((h, w), 2000, dtype="<u2")
        plane[32:64, 48:80] = 0
        fixture.write_bytes(magic + struct.pack("<I", len(encoded)) + encoded + plane.tobytes())
        stats = {}
        for backend in ("cpu", "glow"):
            image = output / f"surface-{backend}.png"
            subprocess.run([ROOT / "target/release/xtion-vision", "--view", "surface", "--point-renderer", backend,
                            "--replay", fixture, "--seconds", "2", "--png", image], check=True, timeout=15)
            rgb = np.asarray(Image.open(image).convert("RGB"))
            mask = np.all(rgb == [127, 255, 127], axis=2)
            yy, xx = np.nonzero(mask)
            if len(xx) < 100:
                raise AssertionError(f"{backend}: missing plane")
            y = (yy.max() + yy.min()) // 2
            # Median over central rows tolerates the metric grid/axis overlay.
            x0, x1 = xx.min(), xx.max()
            samples = [float(mask[y-3:y+4, x-3:x+4].mean())
                       for x in (x0+(x1-x0)//5, (x0+x1)//2, x1-(x1-x0)//5)]
            if samples[0] < .75 or samples[1] > .05 or samples[2] < .75:
                raise AssertionError((backend, samples))
            stats[backend] = {"surface_pixels": len(xx), "left_hole_right": samples}
        print(json.dumps(stats))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    directory = parser.parse_args().output
    directory.mkdir(parents=True, exist_ok=True)
    check(directory)
