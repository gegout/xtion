# Architecture

Xtion is a Rust library plus a thin synchronous CLI. Its only direct crate
dependency is rusb, with vendored open-source libusb for header-independent builds.
No application framework or vendor camera runtime participates.

## Boundaries

| Module | Responsibility |
|---|---|
| `usb` | Candidate enumeration, actual descriptors, handle/interface ownership, EP0 transfers and bulk reads |
| `protocol` | Word encoding, command/reply validation, bounded packet parsing, firmware identity |
| `depth` | Sequence-aware frame reconstruction, padding, invalid shifts, calibrated LUT |
| `frame` | Owned contiguous pixels, explicit units, timestamps, mode, statistics and comparisons |
| `device` | Startup, volatile setting snapshots/readback, acquisition deadlines, shutdown, iterator |
| `error` | Distinct absence, permissions/disconnect, timeout, protocol/NACK and cleanup failures |
| `main` | CLI arguments, diagnostics, statistics and optional PGM output |

Transport and lifecycle types remain private. `Xtion`, `DepthFrame`, `DepthMode`,
`DepthUnits`, statistics, firmware and descriptor structures form the small public
surface. This experimental 0.1 API may evolve after actual hardware validation.
Offline `DepthFrame::new` makes consumers testable without USB.

## Ownership and lifecycle

`Xtion` owns a libusb handle through `Usb`; interface 0 is claimed only on a
candidate identity, with no automatic kernel detach. Current configuration and
alternate are read from the target. Opening sends only GET_VERSION. Startup
requires known firmware/chip, PS mode, and all three firmware streams idle.

Opening, starting, streaming, stopping, idle, recovering and disconnected states are represented in the library. The owner checks mode, snapshots volatile settings, starts its endpoint worker, and validates two advancing complete frames on startup. All restoration remains explicit and bounded. See the [state and recovery design](recovery-design.md).

## Reconstruction and delivery

The USB thread uses a 200 ms read timeout, one 1 MiB buffer and eight bounded staging slots. It records the host completion instant on every successful transfer. Queue overflow counts host-side discards and marks the next enqueued chunk discontinuous. A timeout with no data preserves partial protocol input. Joining the worker precedes handle release.

The consumer parses bounded chunks with a cursor, reassembles contiguous start-to-end frames of the expected size, and decodes packed 11-bit or unpacked 16-bit shifts. A device-derived LUT converts shifts to millimetres. An application can borrow a frame iterator, examine contiguous pixels, or pass owned frames across its own channel.

Offline tests decode real packed and unpacked fixtures through multiple transfer boundaries. A compile-fail borrowing test covers simultaneous mutating calls through an outstanding iterator. See [hardware and fault tests](validation.md).
