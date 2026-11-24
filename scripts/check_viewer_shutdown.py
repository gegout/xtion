"""Exercise desktop signal cleanup on synthetic input or, with --live, the attached sensor."""
import argparse
import json
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/xtion-vision"


def count_frames(recording):
    with recording.open("rb") as source:
        if source.read(9) != b"XTVISION\x01":
            raise ValueError("missing recording header")
        count = 0
        size = recording.stat().st_size
        while length := source.read(4):
            meta = json.loads(source.read(struct.unpack("<I", length)[0]))
            source.seek(2 * meta["width"] * meta["height"], 1)
            if source.tell() > size:
                raise ValueError("partial final frame")
            count += 1
        return count


def summary(stderr):
    messages = [json.loads(line) for line in stderr.splitlines() if line.startswith("{")]
    return next(m for m in messages if m.get("type") == "capture_summary")


def check(live):
    results = []
    with tempfile.TemporaryDirectory() as tmp:
        for sig in (signal.SIGTERM, signal.SIGINT):
            path = Path(tmp)/f"{sig.name}.xtv"
            args = ["--capture-fps", "30"] if live else ["--synthetic"]
            with subprocess.Popen([BINARY, *args, "--record", path, "--seconds", "15"],
                                  stderr=subprocess.PIPE, text=True) as proc:
                deadline = time.monotonic() + 10
                while proc.poll() is None and time.monotonic() < deadline:
                    if path.exists() and path.stat().st_size > (3_000_000 if live else 60_000):
                        break
                    time.sleep(0.05)
                else:
                    raise RuntimeError(f"no frames before {sig.name} test")
                proc.send_signal(sig)
                _, log = proc.communicate(timeout=12)
                result = summary(log)
                frames = count_frames(path)
                if (proc.returncode != 0 or result["status"] != "stopped" or frames == 0
                        or result["processed"] != frames or result["recording_drops"]):
                    raise AssertionError((proc.returncode, frames, result))
                results.append(dict(signal=sig.name, verified_recorded_frames=frames, summary=result))
        missing = subprocess.run([BINARY, "--replay", Path(tmp)/"absent.xtv", "--seconds", "1"],
                                 capture_output=True, text=True, timeout=8, check=True)
        empty = summary(missing.stderr)
        if empty["processed"] != 0 or empty["health"] is not None or not empty["status"].startswith("capture problem:"):
            raise AssertionError(empty)
    return results


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", help="open the attached Xtion twice")
    print(json.dumps(check(parser.parse_args().live), indent=2))
