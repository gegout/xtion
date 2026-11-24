"""Copy a half-open frame interval from an XTV1 recording, preserving source bytes."""
import argparse
import json
import struct
from pathlib import Path


def clip(path, output, start, stop):
    if not 0 <= start < stop:
        raise ValueError("expected 0 <= start < stop")
    with path.open("rb") as src, output.open("xb") as dst:
        header = src.read(9)
        if header != b"XTVISION\1":
            raise ValueError("expected XTV1")
        dst.write(header)
        for i in range(stop):
            length = src.read(4)
            metadata = src.read(struct.unpack("<I", length)[0])
            info = json.loads(metadata)
            size = 2 * info["width"] * info["height"]
            pixels = src.read(size)
            if len(pixels) != size:
                raise ValueError("incomplete input")
            if i >= start:
                dst.write(length + metadata)
                dst.write(pixels)


if __name__ == "__main__":
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("source", type=Path)
    args.add_argument("output", type=Path)
    args.add_argument("start", type=int)
    args.add_argument("stop", type=int)
    options = args.parse_args()
    clip(options.source, options.output, options.start, options.stop)
