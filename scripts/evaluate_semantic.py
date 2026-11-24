"""Score reviewed activity intervals by recording. Never substitute cue text for ground truth."""
import argparse
import hashlib
import json
import subprocess
from collections import defaultdict
from pathlib import Path
from statistics import median


def score(expected, predicted, tolerance_ns):
    """Maximum-overlap greedy one-to-one segment matching with explicit edge tolerance."""
    pairs = []
    for i, (a, b) in enumerate(expected):
        for j, (c, d, detected) in enumerate(predicted):
            # Give credit near either reviewed edge, preserving the original detection time.
            c_aligned = a if abs(c - a) <= tolerance_ns else c
            d_aligned = b if abs(d - b) <= tolerance_ns else d
            overlap = max(0, min(b, d_aligned) - max(a, c_aligned))
            span = max(b, d_aligned) - min(a, c_aligned)
            quality = overlap / span if span else float(c_aligned == a)
            if quality >= 0.5:
                pairs.append((quality, i, j, (detected - a) / 1e9))
    used_truth, used_predictions, delays = set(), set(), []
    for _, i, j, delay in sorted(pairs, reverse=True):
        if i not in used_truth and j not in used_predictions:
            used_truth.add(i)
            used_predictions.add(j)
            delays.append(delay)
    return len(delays), len(predicted) - len(delays), len(expected) - len(delays), delays


def evaluate(binary, directory, split):
    manifest = json.loads((directory / "manifest.json").read_text())
    if manifest["source"] != "sensor":
        raise ValueError("Human evaluation requires sensor recordings")
    counts = defaultdict(lambda: [0, 0, 0, []])
    reviewed = files = instructed = 0
    for clip in manifest["recordings"]:
        if clip["split"] != split:
            continue
        path = directory / clip["recording"]
        with path.open("rb") as source:
            if hashlib.file_digest(source, "sha256").hexdigest() != clip["sha256"]:
                raise ValueError(f"Changed recording {path.name}")
        instructed += 1
        review = path.with_suffix(".reviewed.json")
        if not review.exists():
            continue
        annotations = json.loads(review.read_text())
        if isinstance(annotations, dict):
            annotations = [annotations]
        if any(a["status"] != "reviewed" for a in annotations):
            raise ValueError("Review has an unreviewed interval")
        reviewed += len(annotations)
        files += 1
        hypotheses = {}
        with subprocess.Popen([binary, "--headless", "--fast", "--replay", path],
                              stdout=subprocess.PIPE, text=True) as proc:
            for line in proc.stdout:
                packet = json.loads(line)
                graph = packet.get("semantic")
                if graph is None:
                    continue
                for event in graph["activities"]:
                    key = event["id"]
                    detected = hypotheses.get(key, (None, None, None, graph["ns"]))[3]
                    hypotheses[key] = (event["kind"], event["start_ns"],
                                       event["end_ns"] or graph["ns"], detected)
            if proc.wait():
                raise RuntimeError(f"Replay failed for {path.name}")
        # Score only reviewed time. Unreviewed scenes cannot supply false positives.
        for kind in {a["label"] for a in annotations} | {v[0] for v in hypotheses.values()}:
            truth = [(a["start_ns"], a["end_ns"]) for a in annotations if a["label"] == kind]
            pred = [(a, b, d) for k, a, b, d in hypotheses.values() if k == kind and
                    any(a <= v["end_ns"] and b >= v["start_ns"] for v in annotations)]
            tp, fp, fn, delays = score(truth, pred, min(a["tolerance_ms"] for a in annotations) * 10**6)
            tally = counts[kind]
            tally[0] += tp
            tally[1] += fp
            tally[2] += fn
            tally[3].extend(delays)
    return {"split": split, "instructed_files": instructed, "reviewed_files": files,
            "reviewed_intervals": reviewed, "activities": {
                kind: {"tp": tp, "fp": fp, "fn": fn,
                       "precision": tp / (tp + fp) if tp + fp else None,
                       "recall": tp / (tp + fn) if tp + fn else None,
                       "median_detection_delay_s": median(delays) if delays else None}
                for kind, (tp, fp, fn, delays) in counts.items()}}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--split", choices=["development", "held-out"], default="held-out")
    options = parser.parse_args()
    print(json.dumps(evaluate(options.binary.resolve(), options.directory, options.split), indent=2))
