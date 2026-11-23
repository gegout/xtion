# Validation — 2026-11-22

## Current qualification

The default packed path completed 20 consecutive verified stops, three two-minute windows and 600.002 seconds at 29.965 complete FPS. The long window had two incomplete frames, zero queue drops and zero terminal USB errors. A fresh acquisition succeeded after stopping. A later packaging rebuild captured a calibrated frame and completed three additional verified stops. The [stress results](stress-results.md) and [compatibility matrix](compatibility-matrix.md) cover the revised control timing, verified idle, steady-state measurements and recovery tests. Earlier entries below record the failure history that motivated these changes. The early “blocked” tables describe those sessions only.

## Direct-port packed acquisition at 22:58–22:59 local time

Linux enumerated `1d27:0601` on `1-2` at address 053 with a working user
ACL. GET_VERSION identified firmware 5.8.22. After packed stream activation,
a separate endpoint read returned 25,453,296 bytes in 2.003 seconds with
60 start/end pairs and 422,400 payload bytes per pair. The enable reply
timed out. A USB reset addressed only to 001/053 was followed by
re-enumeration at address 054 and a successful firmware query.

On address 054 the public API accepted a complete decoded frame as evidence of
activation and delivered 156 frames in a five-second consumer window, including
buffered startup data. The first frame had 87,511 invalid pixels and valid
calibrated values from 420 to 4,041 mm. Absolute accuracy remains unmeasured.
The reader reported six incomplete frames, zero decoder errors and 134 dropped
USB chunks across startup and acquisition. The disable command timed out and
endpoint traffic continued. No shared controller reset was performed.


## User-confirmed motion and recovery

During the user's hand sweep and step, a read-only diagnostic sampled four
sequence-contiguous packed VGA frames per approximately two-second window.
For each frame it counted valid pixels closer than 1,200 mm in each horizontal
third of rows 100–379. The central 160×160 region supplied a median valid
distance. The CSV contains the median of each statistic across sampled frames.

At 23:00:21 the central third had 14,860.5 near pixels and the central
distance was 629 mm. At 23:00:23 these were 40,689 pixels and 510.5 mm.
From 23:00:40 through 23:00:52 the central distance stayed within 551–556 mm.
These are regional scene changes. Individual gesture phases lacked synchronized
labels, so the exact trajectory and step distance remain unverified.
[Recorded regional samples](traces/packed-movement.csv)

A reset addressed only to 001/054 restored firmware communication at 001/055.
A two-second read then returned zero endpoint bytes. The idle status of streams
5, 6 and 7 was subsequently confirmed by firmware parameter reads.

## Repeat acquisition and final device state

On 001/055 a diagnostic tried the upstream idle-device soft initialization
(`XnHostProtocolReset`, SOFT_FIRST, SET_MODE opcode 6 value 3). The mode
command returned an empty USB IN transfer. This diagnostic runs outside the
library. Subsequent acquisition yielded 246 calibrated frames over an
eight-second consumer window, with 12,721,844 bulk bytes/s during that window.
The frame count includes startup buffering. Across startup and streaming the
reader reported seven incomplete frames, zero decode errors and 134 dropped
chunks. The CLI used 1.39 s user CPU and 0.37 s system CPU over its complete
23.29 s run including startup and shutdown attempts.

Normal shutdown still returned a restoration timeout. A targeted USB reset
of 001/055 then restored a responding device at 001/056 on the same physical
port. The final GET_VERSION succeeded and a 2.004-second endpoint check read
zero bytes. The shared USB controller stayed online.

## Earlier power-disconnect check

The kernel recorded disconnect of device 047 from `1-4` at 20:45:51. After the
user reported reconnecting to that same port, system/native discovery found no
1d27 device. A further 25-second passive enumeration watch also found none, and
no new USB connection event appeared in the inspected kernel log. The capture
command returned DeviceNotFound; no packed-format sensor commands were sent.

The current blocker is a stable enumerating physical USB connection. Repeated
port/ACL changes alone have not established one consistently. The observations
cannot distinguish the cable, sensor power/hardware, or host connection path.
Earlier native firmware/calibration replies and 60 complete depth frames remain
recorded evidence; they do not establish current accessibility or safe lifecycle
control. No additional camera runtime, kernel driver changes or firmware writes
were performed. Cargo package verification also passed with the captured fixture
included (41 files, 129.9 KiB compressed).


## Latest direct-port re-enumeration: control endpoint unresponsive

The sensor re-enumerated at 20:40:40 as `1d27:0601`, bus 001/address 047, directly
on laptop root path `1-4`. Persistent read/write permissions remain effective.
Device enumeration and cached descriptors are available, but opening acquisition
fails on the standard GET_INTERFACE request for interface 0, **before any firmware
query or packed-format command is sent**. The CLI now identifies this exact stage.

The existing snd-usb-audio driver independently logged frequency/control-request
failures and repeated `usb_set_interface failed (-110)` on the Xtion beginning at
20:40:45. This predates the packed capture attempt and does not establish which
component caused the unresponsive state. A two-second read-only check of bulk
endpoint 0x81 returned zero bytes. Earlier captured 60-frame observations remain
valid for the previous session, but no frames were received on this connection.

No kernel driver was detached or changed, no host USB reset was sent, and no
firmware was flashed. The sensor's control endpoint must respond again before
native initialization or the packed comparison can be tested. A complete physical
power disconnect is the next non-invasive troubleshooting step; its outcome is
not assumed. Continually selecting ports alone has not resolved this state.


## Packed-format trial awaiting enumeration

After the user reported the fresh connection ready, the sensor was absent from
`lsusb`, sysfs topology and native discovery. A bounded nine-second recheck found
no 1d27 device. The kernel recorded its disconnect from `1-2` at 20:37:52, with no
subsequent reconnect event in the inspected log. The packed capture command exited
with DeviceNotFound before sending sensor commands. The packed startup trial has
therefore not yet run. Earlier complete-frame observations remain valid historical
evidence, but current live testing requires the device to enumerate again.


## Earlier unpacked recording

The sensor is directly connected at `1-2`, bus 001/address 046, with working
persistent permissions. Native firmware/calibration queries succeed. A new
16-bit depth-enable attempt again timed out waiting for its control reply, but
the bulk reader reported 145,219,956 bytes. This revealed that acquisition had
actually started despite the missing acknowledgement.

A read-only diagnostic opened only interface 0 and read endpoint `0x81`, sending
no vendor commands or resets. With 32 KiB reads it captured 30,906,548 bytes in
2.001 s, with frequent sequence gaps and incomplete frames. With 1 MiB reads it
captured **37,373,880 bytes in 2.021 s**, yielding **60 complete 640×480 frames**
(about 29.69 complete frames/s). The concatenated recording has two initial
sequence gaps/3,072 discarded bytes, but the 60 accepted frames have exact
614,400-byte pixel payloads and no internal sequence gaps. This is an uncontrolled
scene recording, not a known-distance accuracy or stationary-scene experiment.

The first complete frame contains 204,091 valid raw shifts, range 363–1010.
Device-calibrated conversion yields 204,091 valid pixels, calculated range
472–4,463 mm, mean 1,633.49 mm, and 103,109 invalid pixels. These are actual frame
statistics with source-grounded device calibration; physical accuracy has not been
checked against a reference distance. The EOF tick value is 1,755,969,030.

A complete actual wire frame and device-read calibration prefixes are saved in
`tests/fixtures/`; an offline regression passes that capture through the library's
packet parser, frame assembler and calibration to produce a public metric
DepthFrame with matching statistics. `docs/live-depth-observation.json` records
the measurements. Full temporary recording: `/tmp/xtion-live-large.usbraw`;
calculated depth export: `/tmp/xtion-observed-depth.pgm`.

Control GET_VERSION still times out even with a concurrent bulk drain (55,522,296
bytes over that diagnostic's three-second window). Safe shutdown and restored
settings remain unverified. The normal `xtion capture` command still fails at
startup acknowledgement; its success is not claimed. The initial small-buffer
receive problem is separate from the control-reply problem.

The next comparison uses the upstream default packed 11-bit format (parameter
18=3), with a new MSB-first decoder and 1 MiB receive buffers. The packed decoder
has offline tests, including full VGA reconstruction, but this startup variant
has not yet been tested live. `XTION_WIRE_FORMAT=16` retains unpacked diagnostics.
A fresh physical reconnect has been requested before that trial. No speculative
commands, firmware writes, kernel resets, driver detach or vendor runtime were used.


## Direct-port reconnection check

After the user reported connecting directly, the kernel showed the old hub-path
`1-2.4` disconnect at 19:30:49, but no new USB connection event. System enumeration
and native probing found no `1d27` device, including a bounded 10-second recheck.
This is USB enumeration failure, not an access-denied result. No firmware or depth
commands could be attempted on the new connection. The physical cable/port
connection must enumerate again before the live investigation can continue.


## Persistent access verified; activation still blocked

After the user installed `packaging/70-xtion.rules` and triggered the target,
`/dev/bus/usb/001/041` is root:plugdev with mode 0660 and a cgegout read/write ACL.
The native firmware query succeeds. The earlier permission blocker is resolved.

The corrected startup was attempted with the bounded USB reader already running.
Configuration commands still ACK, but stream1 depth enable receives only empty
control reads until timeout. Cleanup replies are also unavailable, and a subsequent
GET_VERSION attempt times out. The device remains enumerated at address 041;
no new disconnect appeared in the kernel log for this attempt. No complete depth
frame was acquired. The actual control trace is preserved at
`docs/traces/startup-hub-attempt.log` (not a depth-stream fixture).

The current sysfs topology is still `1-2.4`, beneath the Genesys `1-2` hub. A direct
laptop-port reconnect has been requested to obtain a fresh session and distinguish
hub-related behavior from missing initialization. The cause of the timeout is
unknown. The source-matching reader ordering correction alone did not resolve it.
No host reset, driver detach, firmware write, or speculative command was used.


## Live access and startup attempt (after manual ACL grant)

The user's ACL grant on `/dev/bus/usb/001/040` was verified as
`user:cgegout:rw-`. Native `xtion info` then successfully claimed only interface 0
and returned firmware **5.8.22**, chip **0x00202020 (PS1080)**, FPGA **4**, system
**15**. Manufacturer/product strings read via libusb were PrimeSense / PrimeSense
Device. The actual GET_VERSION response is preserved as the small fixture
`tests/fixtures/version-5.8.22.hex` and tested offline.

A traced capture attempt received valid replies for PS operating mode, idle stream
parameters, keepalive, fixed parameters, depth algorithm 0, and depth format,
resolution and FPS writes/readbacks. Calibration data included emitter baseline
7.5, reference distance 120, reference pixel-size bytes `19 73 d7 3d`, and constant
shift 200; these are calibration parameters, not measured scene distances.

The SET_PARAM stream1=depth request received no reply before the original two-second
deadline. Cleanup attempts also timed out. Kernel logs then recorded disconnect
at 19:07:16 and re-enumeration at 19:07:17 as bus 001/address 041, same `1d27:0601`
identity and sysfs path `1-2.4`. No USB reset command was sent. The physical reason
for disconnection is unknown; parameter restoration cannot be verified.
The new node is root:root 0664 without the old ACL, so further queries are denied.

Comparison with `XnSensorDepthStream.cpp::ConfigureStreamImpl` exposed a startup
ordering omission: upstream starts USB reads before firmware configuration and
depth enable. The Rust implementation now starts a bounded reader first, and its
control deadline matches upstream's five seconds. This correction is offline-tested
but **has not yet been retested on the physical device**. No complete depth frame,
FPS, motion response, reliable range or accuracy result is claimed.

A narrowly scoped persistent permission rule is prepared at
`packaging/70-xtion.rules`; installation requires administrator authentication in
the user's own terminal. `sudo -n true` still fails for this process. No udev rule
was installed or host security bypassed. Later live testing must verify startup,
valid frames and explicit shutdown; the current blocker is access to the newly
created USB node, rather than the original grant having failed.


## Earlier hardware recheck (before ACL grant)

After moving the cable, the sensor enumerates as `1d27:0601` at bus 001/address
040, with bulk sensor endpoints on interface 0, alternate 0. `xtion probe`
succeeds. `xtion info` obtains descriptors but fails opening the target:
`USB: Access denied (insufficient permissions)`. The node is root:root 0664 with
no per-user write ACL. Noninteractive sudo requires authentication.

**The current blocker is USB read/write permission, superseding initial device
absence.** No vendor command, firmware response, real depth frame, or live signal
measurement is claimed. The original validation transcript and test table below
remain historical evidence. Current results are in
[device-recheck.txt](device-recheck.txt). An administrator must grant access before
hardware acquisition and movement tests can proceed.

## Initial outcome (before reconnection)

**Offline implementation verified; physical sensor acceptance criteria blocked.**
The device was not visible in either system USB enumeration or native libusb
candidate discovery. No real depth frame, firmware response, calibration,
motion response, or acquisition performance measurement is claimed.

The release executable contains the native transport and source-grounded
initialization/decoder; successful compilation does not demonstrate hardware
compatibility. Protocol facts and remaining initialization questions are in
[protocol.md](protocol.md). The environment/inventory is in [device.md](device.md).

## Actual checks

| Check | Result |
|---|---|
| `uname`, os-release, lsusb, topology, usb-devices, sysfs | Ubuntu 26.04.1; no 1d27 target |
| Kernel journal for 2026-11-22 | Hub, mouse, LifeCam enumeration; no Xtion/PrimeSense identity |
| `sudo -n modprobe usbmon` | Authentication required; module not loaded |
| `cargo fmt --all --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test` | Passed: 12 library, 1 CLI and 3 public API tests (16 total) |
| `cargo build --release` | Passed |
| `cargo package --offline` | Passed, including packaged-crate build verification |
| `ldd target/release/xtion` | libc and system loader only; no vendor camera runtime |

The final command transcript, including individual command exit codes and CLI
output, is [validation-run.txt](validation-run.txt). A probe that returns exit 1
with `DeviceNotFound` is the observed absence test, not successful sensor access.
The enumeration example returns an empty list successfully, as expected for a
library discovery API.

## Offline tests and fixtures

The initial packet/frame fixtures were **synthetic**. The later GET_VERSION
fixture contains a real control reply; depth packet fixtures remain synthetic.
Tests exercise:

- Command byte layout, fragmented replies, bad magic/opcode/request ID/status/size.
- Firmware version byte ordering, BCD build decoding and unsupported firmware gate.
- Split/coalesced packets, partial magic, malformed header recovery.
- Full 640×480 reconstruction through 1-, 511-, 16384- and 32768-byte USB chunks.
- Start/end payload inclusion, pixel alignment across packets, sequence wrap,
  padding, missing packets, oversize and incomplete frames, invalid shift values.
- Synthetic geometric calibration and rejection of invalid/NaN geometry. Synthetic
  conversion expectations are mathematical tests, not measured sensor distances.
- Volatile setting restoration after rejected startup and retryable stop failure.
- External public frame access, statistics, invalid pairs, unit/mode mismatches,
  configurable frame comparison and invalid CLI arguments before acquisition.
- Error classification for access denial, timeout and disconnection (offline
  injected values). These do not establish physical handling under those conditions.

## Requested live scenarios

| Scenario | Status | Missing evidence |
|---|---|---|
| A: stationary scene | Blocked: no device | Valid pixels, frame variation, stability |
| B: moving hand | Blocked: no frames | Spatially localized depth change |
| C: changing distance | Blocked: no frames | Metric trend versus observed movement |
| D: bounded continuous acquisition | Blocked: no device | Actual FPS, throughput, incomplete frames, CPU, stability |
| E: device unavailable | Verified on this host | All hardware CLI commands report absence |
| E: permissions, USB timeout, failed init, safe stop | Offline coverage only | Actual sensor error/cleanup behavior |
| Final end-to-end capture | Blocked: no device | Initialization, depth data, decoding and shutdown on hardware |

No movement request was made: without frames it could not validate anything.
No unrelated webcam, hub, HID or Bluetooth interface was claimed or sent sensor
commands. No kernel driver, firmware, udev rule or other system configuration
was modified. usbmon's unavailable privilege was reported rather than bypassed.

## Exact remaining requirement and repeatable procedure

The ASUS/PrimeSense sensor must first enumerate and be exposed to this process
with read/write access. Its actual VID/PID, descriptors and firmware must then be
recorded. If its protocol differs from the guarded subset, research that variant
before extending initialization. Missing USB visibility cannot be fixed by the
frame decoder or by a camera runtime installation.

Once visible:

1. Run `xtion probe` and `xtion info`; save actual descriptor and firmware output.
2. Run `XTION_TRACE=1 xtion diagnose`; inspect ACKs and startup/stop readbacks.
3. Run `xtion capture scene.pgm`; verify 640×480 storage, explicit units and useful
   valid pixels. Check distances against a known stationary reference before
   calling measurements accurate.
4. Record an empty/stationary scene with `xtion stream 10`. Compare invalid counts,
   successive frame changes, pixel statistics and observed FPS.
5. Request a hand sweep at about 1.5 m, then approach/retreat, recording before,
   during and after. Analyze changed pixel coordinates using `pixels()`;
   the simple example's global count alone is not spatial motion validation.
6. Use `XTION_DUMP=depth.usbraw xtion stream 30` and `/usr/bin/time -v` where
   available for bounded performance/CPU evidence. Dumps contain endpoint bytes,
   not usbmon URBs. Host byte counters exclude partial failed transfers.
7. Check explicit shutdown readbacks and repeat a fresh capture. Exercise only
   safe error cases; never unbind unrelated devices or change firmware.

Record actual measurements and replace blocked statuses only when those tests
have been performed. Hardware-dependent frame acquisition and movement definition-of-done items remain open.

Current offline verification after adding packed decoding and the real depth fixture: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (21 tests), and `cargo build --release` all passed. Packed startup and physical movement tests remain pending.

## Fault injection with the connected sensor

The independently invoked hardware tests swallowed the matching enable and disable replies **after the sensor had responded**. Acquisition required two complete advancing frames. Shutdown still verified firmware state, saved settings and endpoint silence. A second test injected a reader NoDevice error while the physical sensor stayed on its port. A normal read recovered by reopening and restarting without a reset. Both tests passed and a separate diagnostic confirmed idle afterward. This tests the application’s recovery decision and real USB reopen path. Re-enumeration with a different address is covered separately by earlier hardware evidence and deterministic identity tests. [Test output](traces/final-fault-injection.log)

To reproduce on a dedicated physical sensor, run:

```sh
cargo test --lib hardware_faults -- --ignored --test-threads=1
```
