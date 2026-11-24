"""Score tracked source cells against independent outlines. Write local review panels."""
import argparse
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from extract import REVISIONS, ROOT
sys.path.insert(0, str(ROOT / "scripts"))
from spatial_noise import frames


def reference(depth, polygon, border):
    mask = Image.new("L", depth.shape[::-1])
    ImageDraw.Draw(mask).polygon([tuple(p) for p in polygon], fill=255)
    inside = np.asarray(mask.filter(ImageFilter.MinFilter(border * 2 + 1))) > 0
    outside = np.asarray(mask.filter(ImageFilter.MaxFilter(border * 2 + 1))) == 0
    return inside & (depth > 0), outside & (depth > 0)


def compare(objects, inside, outside):
    primary = np.zeros(inside.size, dtype=bool)
    union = primary.copy()
    if objects:
        # Rust max_by_key resolves tied maxima to the last candidate.
        _, members = max(reversed(objects), key=lambda o: len(o[1]))
        primary[members] = True
    for _, members in objects:
        union[members] = True
    main = primary.reshape(inside.shape)
    all_tracks = union.reshape(inside.shape)
    hits = int((inside & main).sum())
    leak = int((outside & main).sum())
    return dict(target=int(inside.sum()), recovered=hits, background=leak,
                other_tracks=int((inside & all_tracks & ~main).sum()),
                missed=int((inside & ~all_tracks).sum())), main


def tile(depth, inside, outside, track):
    gray = (np.minimum(depth, 4200) / 4200 * 195 + 40).astype("uint8")
    gray[depth == 0] = 0
    rgb = np.repeat(gray[:, :, None], 3, axis=2)
    if track is None:
        rgb[inside] = [55, 180, 150]
        rgb[~(inside | outside) & (depth > 0)] = [182, 150, 88]
    else:
        rgb[inside & track] = [55, 180, 150]
        rgb[inside & ~track] = [243, 166, 72]
        rgb[outside & track] = [215, 65, 120]
    return Image.fromarray(rgb).resize((224,168), Image.Resampling.NEAREST)


def report(labels, recordings, output, split, revisions):
    result = {}
    for clip, settings in labels["clips"].items():
        selected = {int(i): poly for i, poly in settings["frames"].items()
                    if (int(i) >= settings["holdout_from"]) == (split == "holdout")}
        masks = {name: dict(json.loads(line) for line in
                            (output / f"{clip}-{rev}.jsonl").read_text().splitlines())
                 for name, rev in revisions.items()}
        rows = []
        result[clip] = {}
        for i, raw in enumerate(frames(recordings / f"{clip}.xtv")):
            if i not in selected:
                continue
            step = labels["grid_step"]
            depth = raw[::step, ::step]
            inside, outside = reference(depth, selected[i], labels["uncertain_border_cells"])
            raw_view = Image.fromarray(np.uint8(np.minimum(depth, 4200) / 4200 * 255)).resize((224,168), Image.Resampling.NEAREST)
            row = [raw_view, tile(depth, inside, outside, None)]
            for name in revisions:
                counts, track = compare(masks[name][i], inside, outside)
                result[clip].setdefault(name, {})[str(i)] = counts
                row.append(tile(depth, inside, outside, track))
            rows.append((i, row))
            if i == max(selected):
                break
        sheet = Image.new("RGB", (224*(2+len(revisions)), 50+len(rows)*192), "#25232a")
        draw = ImageDraw.Draw(sheet)
        for j, title in enumerate(["Depth", "Reference", *revisions]):
            draw.text((j*224+5,8),title)
        for y, (i, row) in enumerate(rows):
            draw.text((5,28+y*192), f"{clip}  frame {i}")
            for x, img in enumerate(row):
                sheet.paste(img,(x*224,49+y*192))
        suffix = "" if "production" in revisions else "-" + "-".join(revisions.values())
        sheet.save(output/f"{clip}-{split}{suffix}.png")
        for name, data in result[clip].items():
            totals = {k: sum(row[k] for row in data.values()) for k in next(iter(data.values()))}
            tp, fp = totals["recovered"], totals["background"]
            data["pooled"] = dict(totals, recall=tp/totals["target"],
                                  precision=tp/(tp+fp) if tp+fp else None)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("recordings", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("split", choices=["development","holdout"])
    args = parser.parse_args()
    labels = json.loads((ROOT/"docs/spatial/silhouettes.json").read_text())
    print(json.dumps(report(labels, args.recordings, args.output, args.split, REVISIONS), indent=2))
