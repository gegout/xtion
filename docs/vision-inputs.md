# Xtion Vision input contract

The native driver exposes one depth image through `Xtion::read_depth_frame_timeout`. The current requested mode is VGA 640×480 at 30 FPS. Check each frame's dimensions and units. The row-major slice contains u16 samples. Zero represents missing data. Raw shifts cannot supply metric distance. The public API has no RGB image.

`DepthFrame::timestamp` is a wrapping 32-bit EOF device counter. `clock_hz` may be unavailable. `host_arrival` is monotonic elapsed time since open. Reset timing history when reopening. USB sequence gaps and incomplete frames provide stronger evidence of loss than a device timestamp jump alone.

`Xtion::statistics` exposes received bytes and frames, startup discards, incomplete frames, packet gaps, decoder errors, discarded bytes, USB errors, timeouts and dropped host transfers. `state` and `last_recovery` describe lifecycle and recovery. Time since last arrival must be evaluated even while no frame arrives.

The calibration table converts shifts to millimetres. Neither the public device nor frame API exports a camera matrix, distortion coefficients or a measured principal point. Nominal geometry may assume a centred pinhole model. GeoVision's [Xtion PRO hardware specification, page 49](https://www.surveillance-video.com/media/lanot/attachments/customimport/81-XTIONPR-A002-Manual.pdf) gives 58° horizontal and 45° vertical. Unknown optics allow Z alone.

Phase 1 verification on the connected Ubuntu host: USB 1d27:0601 enumerated. Existing cargo test --offline passed 31 tests including a doctest, with two hardware tests ignored.
