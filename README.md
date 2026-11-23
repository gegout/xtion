# Xtion and Xtion Vision

A small, experimental native Rust USB library and CLI for PrimeSense PS1080
structured-light depth sensors, intended as a reusable foundation for perception
applications. Uses `rusb` and open-source libusb directly. No OpenNI, OpenNI2,
PrimeSense Sensor, SensorKinect, ROS, proprietary runtime, or kernel camera driver
is installed or required.

The workspace builds two applications. **xtion** probes, captures, diagnoses and stress-tests the USB sensor. **xtion-vision** watches visible depth surfaces, measures how they move, and describes the scene locally. Its desktop and JSON-lines interfaces consume the same perception engine. A configured vision endpoint can add shape hypotheses with evidence IDs.

## Run Xtion Vision

Use Ubuntu with a Rust toolchain compatible with the locked dependencies (tested with Rust 1.99), a C toolchain, and Mesa software OpenGL for the desktop. The USB permissions below apply to live acquisition. Replay and synthetic scenes work without a sensor.

```bash
cargo build --release --locked --workspace
target/release/xtion probe
target/release/xtion-vision
target/release/xtion-vision --synthetic
target/release/xtion-vision --headless --seconds 30 --record session.xtv
target/release/xtion-vision --replay session.xtv --png view.png
target/release/xtion-vision --headless --fast --replay session.xtv --ply first.ply
cargo test --workspace --locked
```

Keep the camera fixed and start with an empty scene. The viewer learns the initial background automatically. Select **Relearn background** after moving the camera. Increasing **Foreground mm** requires larger depth differences. Stop capture before opening a replay or restarting the sensor. The colour legend shows positive axial depth through the configured far limit, with black for missing readings. The live stream currently supplies nominal lateral geometry and device-calibrated axial depth when available.

Headless mode writes an observation per delivered frame and separate event records. Live output favours the latest observation if the reader stalls. Use `--fast --replay` for lossless deterministic analysis of all recorded frames. Ctrl-C ends headless acquisition. `--seconds N` bounds a run. `--record` and `--ply` require a new file. At VGA/30, uncompressed recording uses roughly 1.1 GB per minute. A PNG contains the entire viewer.

### Optional AI

```bash
target/release/xtion-vision --ai-url http://localhost:8000/v1
target/release/xtion-vision --ai-url https://api.openai.com/v1 --ai-model MODEL_NAME
```

For a build that omits the AI HTTP client entirely, use `cargo build -p xtion-vision --release --no-default-features`.

AI is off by default. Enabling it sends reduced depth images, observations, recent events and motion context to the chosen endpoint. Local endpoints can operate without credentials. Set `OPENAI_API_KEY` in the process environment when required. The UI and JSON report hypotheses, their observation IDs and confidence. The local summary continues during discovery, outages or rejected replies.

Discovery tests up to four candidate models under a 30-second overall deadline. Each request times out after eight seconds. Four synthetic quadrant checks establish image use before model selection. Explicit model mode skips discovery. Chat Completions is the default. A missing Chat route falls back to Responses only when the endpoint advertises a matching POST operation in its OpenAPI document. `--ai-candidates`, `--ai-budget-ms`, `--ai-timeout-ms`, `--ai-probe-tokens`, `--ai-tokens` and `--ai-refresh-ms` set limits. The default refresh interval is two seconds and the default interpretation cap is 400 output tokens. Run `xtion-vision --help` for the full CLI.

### Recording format and integration

XTV v1 starts with the eight ASCII bytes `XTVISION` followed by version byte 1. Each record contains a little-endian u32 JSON-header length, a UTF-8 JSON frame header, then width × height little-endian u16 samples in row-major order. The header preserves dimensions, metric or raw units, wrapping device ticks, monotonic arrival nanoseconds, health counters and intrinsics with provenance. EOF at a record boundary terminates the session. A partial record is an error. Only generated scenes ship as replay fixtures.

Applications can use `xtion_vision::perception::Engine::observe`, `DepthFrame::point_cloud`, and the `CloudExport` trait without accessing USB. The [vision architecture](docs/vision-architecture.md), [verified input contract](docs/vision-inputs.md) and [validation report](docs/vision-validation.md) describe geometry, timing, tracking limits and evidence.

## Native driver

**Validated configuration:** ASUS Xtion PRO `1d27:0601`, PS1080 firmware 5.8.22, direct high-speed USB and 640×480 packed depth. Normal shutdown disables depth, verifies settings and checks endpoint silence. The ten-minute test delivered 29.965 complete frames/s after warm-up with two incomplete frames. Twenty consecutive start/stop cycles passed. Firmware 5.8.22 is the latest exact-model release found on ASUS support.

See the [compatibility matrix](docs/compatibility-matrix.md), [measured reliability results](docs/stress-results.md), [firmware audit](docs/firmware-compatibility.md) and [issue investigation](docs/reliability-investigation.md).

## Scope and compatibility

- Candidate USB IDs: `1d27:0500`, `1d27:0600`, `1d27:0601`, `1d27:0609`, from upstream
  PS1080 enumeration. Only `1d27:0601` has been verified on hardware.
- Initialization requires firmware 5.1–5.8, chip PS1080 or PS1080A6, PS operating
  mode, idle streams, and a matching bulk depth endpoint `0x81` on interface 0.
- One requested spatial mode: 640×480, 30 FPS. Packed 11-bit shifts are the tested default; `XTION_WIRE_FORMAT=16` selects the observed unpacked mode.
- Frames report `Millimetres` only when device-read calibration is usable;
  otherwise `RawShift`. In either representation zero means invalid.
- Isochronous transfers, legacy bulk command transport, other firmware, RGB/IR,
  audio, other compressed depth formats, registration, and point clouds are unsupported.

Tested build environment: Ubuntu 26.04.1 LTS x86_64, kernel 7.0.0-43-generic,
Rust/Cargo 1.99.0. Other environments are unverified.

## Build and development

Requires Rust/Cargo and a working C compiler for vendored libusb. Cargo fetches
`rusb` and its transport/build dependencies; it does not fetch a camera runtime.
The first build needs access to crates.io. `Cargo.lock` records resolved versions.

```bash
cargo build --release --locked
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The executable is `target/release/xtion`. Optionally use `cargo install --path .
--locked` to install only this CLI. No service is needed. Rustfmt configuration
is in `rustfmt.toml`; Clippy and the unsafe-code prohibition are in `Cargo.toml`.

## Device permissions

Start with `lsusb`, `lsusb -t`, and `target/release/xtion probe`. If `lsusb` does
not show the sensor, changing permissions cannot make it enumerate. Check its
USB/power connection. A normal RGB webcam is not a substitute.

For this detected `1d27:0601` device, `packaging/70-xtion.rules` provides a
persistent rule granting the existing `plugdev` group access and tagging it for
local desktop ACLs. This host's user already belongs to `plugdev`. To install it
from the repository in an authenticated administrator terminal:

```bash
sudo install -m 0644 packaging/70-xtion.rules /etc/udev/rules.d/70-xtion.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --action=change --subsystem-match=usb --attr-match=idVendor=1d27 --attr-match=idProduct=0601
```

The final command matches the sensor by vendor and product ID. A future
reconnection also applies the rule. A one-time `setfacl` grant on a numbered USB
node is lost when the device disconnects and receives a new address. The rule is
installed by the user and verified effective on this host.

The library claims depth interface 0 and leaves kernel bindings in place. Explicit stop reports errors restoring volatile settings. Drop is best effort. The CLI can recover an orphaned stream after a process exits. Only explicitly requested device-scoped recovery can reset the target. Normal use requires access to the device node only.

## CLI

```bash
./target/release/xtion probe                 # candidate identities
./target/release/xtion info                  # descriptors and firmware query
./target/release/xtion capture               # one frame, dimensions and statistics
./target/release/xtion capture scene.pgm     # optional 16-bit grayscale PGM
./target/release/xtion stream 10             # bounded seconds, default 10, max 3600
./target/release/xtion diagnose              # descriptors and a short acquisition
./target/release/xtion diagnose --json       # sample firmware and current depth endpoint
./target/release/xtion stress 120 3         # seconds per cycle, cycle count, JSON lines
./target/release/xtion recover              # reconcile an orphan and verify idle
./target/release/xtion recover --device-reset # permit a target reset after failed reopen attempts
```

No-device, permission, protocol, unsupported-firmware, and timeout failures return
a nonzero exit code. `info` queries firmware but does not activate depth.
`diagnose`, `capture`, and `stream` activate the requested depth mode. With multiple
sensors, the CLI refuses to choose; applications can use `Xtion::open_at(bus,address)`.

Streaming prints frame statistics in the stated units, frame differences, host FPS,
successfully received endpoint bytes/s, incomplete frames, decode errors, parser
resynchronization bytes, and USB timeouts. These are observed host counters, not
an exact device frame-loss metric. A threshold of 100 mm (or 5 raw shifts) is a
simple signal comparison, not a motion-tracking claim. A timeout at the end of a
successful bounded stream is normal; an earlier acquisition timeout is an error.
PGM output preserves pixel values and units in a comment; it is not contrast-scaled.
Output files are created exclusively so existing captures are not overwritten.

Target-only diagnostics, once hardware enumerates:

```bash
XTION_TRACE=1 ./target/release/xtion diagnose
XTION_DUMP=depth.usbraw ./target/release/xtion stream 10
```

Tracing prints EP0 command/reply bytes to stderr. Dumps concatenate successfully
read depth-endpoint bytes, including protocol headers, without usbmon URB metadata.
rusb 0.9.4 returns partial data from timeout/interrupted bulk reads. A zero-byte timeout is counted without discarding a partial frame. Sequence and length validation protect frame boundaries. No unrelated endpoints are captured.

## Rust API

Independent applications can add a path dependency:

```toml
[dependencies]
xtion = { path = "/path/to/xtion" }
```

```rust,no_run
use xtion::Xtion;

fn main() -> xtion::Result<()> {
    let mut sensor = Xtion::open()?;
    sensor.start_depth()?;
    let result = sensor.read_depth_frame();
    let cleanup = sensor.stop_depth(); // attempt shutdown even if the read failed
    let frame = result?;
    cleanup?;
    println!("{}x{} {:?}: {:?}",
        frame.width(), frame.height(), frame.units(), frame.stats());
    Ok(())
}
```

`DepthFrame` owns a contiguous `Vec<u16>` exposed by `pixels()`. It includes mode,
raw wrapping EOF device ticks (`timestamp()`), and monotonic elapsed host arrival
(`host_arrival()`). When the sensor supplies algorithm 0x80, `clock_hz()` gives the reported tick frequency for the current configuration. Host arrival is stamped on USB completion before queueing. Absolute distance accuracy requires a reference measurement.
`pixel(x,y)` checks bounds. `DepthFrame::new` supports normalized offline frames.
`changed_pixels(&other, threshold)` checks mode and units and skips invalid pairs.

`depth_frames()` is a borrowing iterator of `Result<DepthFrame>`; it stops after
its first error. `read_depth_frame_timeout(Duration)` sets a whole-frame deadline.
The normal read uses a five-second frame deadline and one recover/restart attempt on a stall or disconnect. The explicit timeout API preserves the caller’s deadline. An internal USB reader starts before depth enable
and uses eight 1 MiB staging slots. A slow consumer can lose transfers; queue
overflow is counted, and affected frame assembly is invalidated. There is no
unbounded queue. `dropped_usb_transfers` counts host queue overflow after successful USB reads. `usb_errors` counts terminal reader errors. `sequence_gaps` counts observed counter discontinuities, including across frame boundaries. `incomplete_frames` counts interrupted assemblies. A single lost chunk can affect multiple counters. `statistics()` exposes acquisition counters and
`calibration_error()` explains raw-shift fallback after startup.

```bash
cargo run --example enumerate
cargo run --example capture
cargo run --example depth_stream           # up to 300 frames
cargo run --example motion -- 100          # configurable threshold in mm
```

The motion example uses only public frame access and refuses to interpret raw
shifts as millimetres. Applications can build motion detection, visualization,
point clouds, WebSocket services, or MCP adapters in separate crates without
changing the USB layer. None of those frameworks is part of Xtion.

## Layout and known limits

`src/usb.rs` owns transport and discovery; `protocol.rs` parses commands/packets;
`depth.rs` assembles frames, unpacks shifts and applies calibration; `frame.rs` defines application data;
`device.rs` owns lifecycle; `main.rs` provides the CLI. Offline public API tests are
in `tests/`; hardware examples are in `examples/`.
[Architecture](docs/architecture.md) describes ownership and recovery.

Mode changes preserve factory filtering and correction defaults. The calibration LUT has a 10,000 mm cutoff. Thermal corrections, optical accuracy, color alignment and intrinsics remain outside the verified scope.

The default single 1 MiB read and eight-slot queue performed best among the synchronous configurations tested. For reproducing transfer tests, use `XTION_READ_KIB=32|256|1024`. Smaller single-transfer buffers lost frames on this host. Use `XTION_WIRE_FORMAT=16` for the measured alternate VGA encoding.

`state()`, `last_frame_timestamp()` and `last_recovery()` describe the current object. Use `recover(false)` for bounded in-process recovery. Retain `info().clone()` to identify the same port later. The [recovery design](docs/recovery-design.md) details deadlines, failure states and limits after process death.

Xtion Vision is MIT licensed under [LICENSE](LICENSE). The existing native driver and its original contributions retain the [Apache License 2.0](LICENSE-APACHE) and [NOTICE](NOTICE). Historical host-level experiments are recorded in [recovery findings](docs/recovery.md). Application recovery uses only the target USB device.

Ordinary workspace tests run without sensor access. With the sensor exclusively available, run the Vision checks with `cargo test --release -p xtion-vision --test live -- --ignored --test-threads=1`. The native driver's historical capture regression reads private fixture files on the validation host. Run that extra check there with `cargo test -p xtion captured_hardware_frame_decodes_to_public_metric_frame -- --ignored`. Git includes the two synthetic Vision recordings. Personal depth captures stay local.
