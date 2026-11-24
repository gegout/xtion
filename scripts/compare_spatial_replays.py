"""Compare two built spatial_baseline examples on exactly the same private recordings."""

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path


def benchmark(executable, clip, start):
    result = subprocess.run(
        [executable.resolve(), clip, str(start)], check=True, capture_output=True, text=True
    )
    metrics = json.loads(result.stdout)
    del metrics["input"]
    return metrics


def compare(baseline, candidate, clips):
    result = {}
    for name, path, start in clips:
        with path.open("rb") as source:
            checksum = hashlib.file_digest(source, "sha256").hexdigest()
        before = benchmark(baseline, path, start)
        after = benchmark(candidate, path, start)
        if not all(before[k] == after[k] for k in ("frames", "seconds", "score_from_frame")):
            raise ValueError(f"Different scoring intervals for {name}")
        result[name] = {"file": path.name, "sha256": checksum, "baseline": before, "candidate": after}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path, help="production spatial_baseline executable")
    parser.add_argument("candidate", type=Path, help="candidate spatial_baseline executable")
    parser.add_argument("original", type=Path, help="folder containing the original depth clips")
    parser.add_argument("repositioned", type=Path, help="folder containing the repositioned sensor clips")
    parser.add_argument("--archive", type=Path, help="additional historical final-session.xtv")
    parser.add_argument(
        "--check", action="store_true",
        help="fail on fewer dominant frames during a passage or more motion events in a static clip",
    )
    args = parser.parse_args()
    clips = [
        ("entry", args.original / "entry.xtv", 265),
        ("occupied_start", args.original / "crossing.xtv", 0),
        ("static", args.original / "static-interior.xtv", 0),
        ("repositioned_occupied", args.repositioned / "occupied.xtv", 134),
        ("repositioned_clear", args.repositioned / "clear-fixed.xtv", 0),
        ("passage", args.repositioned / "passage-before-cue.xtv", 140),
    ]
    if args.archive:
        clips.append(("archive", args.archive, 0))
    results = compare(args.baseline, args.candidate, clips)
    print(json.dumps(results, indent=2))
    if args.check:
        failures = []
        dominant = "frames_with_80pct_dominant_region"
        for name in ("entry", "occupied_start", "repositioned_occupied", "passage"):
            if results[name]["candidate"][dominant] < results[name]["baseline"][dominant]:
                failures.append(f"{name}: fewer frames with a dominant region")
        for name in ("static", "repositioned_clear"):
            if results[name]["candidate"]["moving_events"] > results[name]["baseline"]["moving_events"]:
                failures.append(f"{name}: more movement events in a static scene")
        if failures:
            print("\n".join(failures), file=sys.stderr)
            raise SystemExit(1)


if __name__ == "__main__":
    main()
