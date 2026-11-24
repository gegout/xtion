"""Compare axial and radial interpretations on the left wall of the static VGA clip."""
import json
import sys
from pathlib import Path

import numpy as np
from spatial_noise import frames

depths = [d[128:352:2, 24:176:2].astype(float) for d in frames(Path(sys.argv[1]))]
stack = np.stack(depths)
stack[stack == 0] = np.nan
depth = np.nanmedian(stack, axis=0) / 1000
v, u = np.mgrid[128:352:2, 24:176:2]
x = (u - 319.5) * np.tan(np.radians(29)) / 320
y = (v - 239.5) * np.tan(np.radians(22.5)) / 240
rays = np.stack([x, y, np.ones_like(x)], axis=-1)
valid = np.isfinite(depth)
# Hold out interleaved 16-pixel tiles so the same 3D samples never fit and score.
fit = ((u // 16 + v // 16) % 2 == 0) & valid
test = ~fit & valid
result = {}
for name, factor in [('axial', np.ones_like(x)), ('radial', np.linalg.norm(rays, axis=-1))]:
    points = rays * (depth / factor)[:, :, None]
    centre = points[fit].mean(axis=0)
    normal = np.linalg.svd(points[fit] - centre, full_matrices=False)[2][-1]
    residuals = (points[test] - centre) @ normal
    result[name] = float(np.sqrt(np.mean(residuals**2)) * 1000)
print(json.dumps(dict(frames=len(depths), fit=int(fit.sum()), test=int(test.sum()),
                      held_out_plane_rms_mm=result), indent=2))
