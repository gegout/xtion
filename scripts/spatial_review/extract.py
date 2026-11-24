"""Replay pinned revisions with read-only instrumentation of flood-fill membership."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
REVISIONS = {"production": "4882d47", "noise": "b992145", "area": "fde92fd", "growth": "18cd346",
             "retained": "32e4085"}


def run(command, cwd=ROOT, **kwargs):
    return subprocess.run(command, cwd=cwd, check=True, **kwargs)


def extract(rev, recordings, output, labels):
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp) / "source"
        run(["git", "worktree", "add", "--detach", str(tree), rev], stdout=sys.stderr)
        try:
            path = tree / "vision/src/perception.rs"
            code = path.read_text()
            for anchor, extra in [
                ("pub samples: usize,", "pub members: Vec<usize>,"),
                ("let mut count = 0;", "let mut members = Vec::new();"),
                ("count += 1;", "members.push(k);"),
                ("samples: count,", "members,"),
            ]:
                if code.count(anchor) != 1:
                    raise ValueError(f"unexpected source for {rev}: {anchor}")
                code = code.replace(anchor, anchor + "\n" + extra)
            path.write_text(code)
            (tree / "vision/examples/spatial_masks.rs").write_bytes(
                (Path(__file__).with_name("masks.rs")).read_bytes())
            env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "target"))
            run(["cargo", "build", "--release", "--locked", "--offline", "-p",
                 "xtion-vision", "--example", "spatial_masks"], cwd=tree, env=env)
            for clip, settings in labels["clips"].items():
                with (output / f"{clip}-{rev}.jsonl").open("w") as out:
                    run([ROOT / "target/release/examples/spatial_masks",
                         recordings / f"{clip}.xtv", *settings["frames"]], stdout=out)
        finally:
            run(["git", "worktree", "remove", "--force", str(tree)])


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("recordings", type=Path)
    p.add_argument("output", type=Path)
    args = p.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    labels = json.loads((ROOT / "docs/spatial/silhouettes.json").read_text())
    for rev in REVISIONS.values():
        extract(rev, args.recordings.resolve(), args.output, labels)
