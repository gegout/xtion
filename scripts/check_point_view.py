"""Check both desktop point renderers on three known source pixels. Run under xvfb-run."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
LANDMARKS = [(4, 4, 1000, [191, 127, 63]), (8, 4, 2000, [127, 255, 127]),
             (12, 4, 3000, [63, 127, 191])]


def check(path):
    rgb = np.asarray(Image.open(path).convert("RGB"))
    background = (rgb == [28, 28, 28]).all(axis=2)
    rows = np.flatnonzero(background.sum(axis=1) > 100)
    cols = np.flatnonzero(background.sum(axis=0) > 100)
    left, top = cols[0], rows[0]
    width, height = np.ptp(cols) + 1, np.ptp(rows) + 1
    errors = []
    for u, v, _, colour in LANDMARKS:
        x = left + int(width / 2 + min(width,height) * (u-7.5)/15)
        y = top + int(height / 2 + min(width,height) * (v-7.5)/18)
        yy, xx = np.where((rgb == colour).all(axis=2))
        if len(xx) == 0:
            raise AssertionError(f"{path}: missing landmark {u},{v}")
        error = float(np.hypot(xx.mean()-x, yy.mean()-y))
        if error > 2:
            raise AssertionError(f"{path}: {error:.2f} pixel landmark error")
        errors.append(round(error, 3))
    return errors


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("directory", type=Path)
    args = p.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        fixture = Path(tmp)/"landmarks.xtv"
        data = np.zeros((16,16), dtype="<u2")
        for u,v,z,_ in LANDMARKS:
            data[v,u] = z
        with (ROOT/"vision/tests/fixtures/still.xtv").open("rb") as template:
            magic = template.read(9)
            length = template.read(4)
            meta = template.read(struct.unpack("<I", length)[0])
        fixture.write_bytes(magic + length + meta + data.tobytes())
        result = {}
        for renderer in ["cpu","glow"]:
            out = Path(tmp)/f"landmarks-{renderer}.png"
            subprocess.run([ROOT/"target/release/xtion-vision","--view","points",
                            "--replay",fixture,"--point-renderer",renderer,
                            "--seconds","2","--png",out],check=True,timeout=15)
            result[renderer] = check(out)
            (args.directory/out.name).write_bytes(out.read_bytes())
        print(json.dumps({"landmark_errors_px":result}))


if __name__ == "__main__":
    main()
