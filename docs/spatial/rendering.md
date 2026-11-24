# Camera-frame point view

The organised cloud borrows the immutable depth frame. Grid coordinates identify the original source pixel, including missing pixels. Point lookup requires metric depth and known optics. A stride changes sampling for display. Consumers can still request every original point. The coordinate frame is optical, in metres, with X right, Y down and Z forward. A manual height and tilt define a separate floor-relative pose. A virtual orbit affects the view alone.

The calibrated shift lookup uses a fixed baseline and reference-plane distance for every pixel. As an empirical check, scripts/spatial_axial.py compared two interpretations on a visible wall patch in the 224-frame static clip. Fitting planes on alternating tiles produced held-out orthogonal RMS residuals of 11.61 mm for axial depth and 14.87 mm for radial range. This supports using axial Z. The wall orientation and camera intrinsics lack independent survey measurements.

## Renderer choice

Keep the existing eframe 0.37 glow window. Project the same source samples in Rust for both paths. The CPU path paints depth-ordered points into a reusable RGB buffer and uploads a texture. The glow path batches point quads into an egui mesh. Both paths cache unchanged frames and camera settings. The Intel Raptor Lake-P adapter reported accelerated Mesa GL 26.0.8. llvmpipe is available for software GL. A points or split launch tries the glow path when the render node is accessible; depth-only launch retains the existing software default. The point-renderer switch in the running window changes how points are drawn within the existing GL context.

The headless library builds with no desktop dependency via `cargo test -p xtion-vision --lib --no-default-features`. Both desktop paths support screenshots. Run `xvfb-run -a python3 scripts/check_point_view.py /tmp/xtion-point-check` after a release build. It renders the same three known source points with both paths, locates each coloured centroid in its PNG, and checks the expected screen coordinates within two pixels. The isolated X11 validation used Ubuntu's Xvfb and XKB libraries extracted under /tmp.

## Controls

Select Depth, 3D points or Split in the sidebar. Drag to orbit, secondary-drag to pan, scroll to zoom, and use Reset camera to restore the physical viewpoint. Select orthographic projection for constant display scale. The half-metre grid lies in a sensor-frame plane; it has no surveyed floor alignment. Choose depth, sensor X, sensor Y or monochrome colouring. Enter an operator-measured sensor height and downward tilt to enable height colouring. Distances derived from nominal optics are approximate. The frustum marks the physical camera. Orbiting reveals gaps where the sensor saw no surface.

## Dependencies of later phases

The tracker and physical-floor estimation depend on segmentation. Three reopened attempts failed the joint replay gate. Region and motion colouring, estimated floor pose, tracked boxes, mesh, measurement UI, further exports and AI geometry updates remain pending. The existing AI, depth view, recording, replay and ASCII cloud export stay available. The package remains at version 0.1 during this partial implementation.

## Version 0.2 surface path

**Surface** and **Surface in split view** use a latest-only reconstruction worker. Source vertices are projected once per virtual-camera update. The CPU path paints projected triangles into an RGB texture; the glow path submits indexed egui mesh geometry. Both order faces by view depth. Strong self-overlap or intersecting projected triangles can therefore give an imperfect visibility order. Picking follows that same draw order.

The repeatable surface check creates a synthetic plane with a hole, launches each path and checks three interior screen locations. On a Wayland desktop, unset WAYLAND_DISPLAY inside the Xvfb test so screenshot and simulated pointer tools use the same X server as the viewer.

```sh
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET xvfb-run -a python3 scripts/check_surface_view.py /tmp/xtion-surface-check
```

The public benchmark flag writes stage rates, source health and the active draw interval. Compare draws against both that interval and total session duration when compositor visibility changes. See the [version 0.2 validation](v0.2-release.md).
