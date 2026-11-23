# USB investigation — 2026-11-22

## Reliability qualification

The sensor enumerated directly at 1-2 throughout the current reliability tests. Sensor and idle checks use its USB port chain and descriptors. Bus addresses in the observations below identify historical sessions. The [compatibility matrix](compatibility-matrix.md) records active settings, the separate audio-interface timeouts and the configurations tested.

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


## Latest observation: device identified, access denied

After the user moved the USB connection, the kernel enumerated the sensor at
19:04:38 Europe/Paris. Native `xtion probe` now discovers it. Earlier absence
findings below describe the initial investigation and are superseded by this check.

| Property | Live observation |
|---|---|
| VID:PID | `1d27:0601` (lsusb database labels ASUS Xtion) |
| Manufacturer / product | `PrimeSense` / `PrimeSense Device`, from sysfs/kernel |
| USB address / topology | bus 001, address 040; `1-2.4`, behind Genesys hub `1-2` |
| USB version / negotiated speed | USB 2.00 / 480 Mbit/s high speed |
| bcdDevice | 0.01; this is not a firmware version query |
| Serial string | No serial string descriptor |
| Configuration | One configuration, value 1, bus powered, 500 mA descriptor maximum |
| Interface 0 | Vendor specific; no kernel driver bound |
| Interface 0, alternate 0 | Bulk IN `0x81`, `0x82`, 512-byte maximum packets |
| Interface 0, alternate 1 | Isochronous IN `0x81` (2×880 bytes), `0x82` (3×860), interval 1 |
| Interface 0, alternate 2 | Isochronous IN `0x81` (2×640 bytes), interval 4; `0x82` (3×860), interval 1 |
| Interface 1, alternate 0 | USB audio control, interrupt IN `0x83`, 4 bytes, interval 8 |
| Interface 2 | Audio streaming, alternate 0 has no endpoints; alternate 1 has isochronous IN `0x84`, 192 bytes, interval 4 |
| Audio binding | Existing `snd-usb-audio` on interfaces 1 and 2; left untouched |
| libusb open | Failed with `Access denied (insufficient permissions)` |
| Firmware / depth frames | Not queried/acquired because libusb cannot open the device |

`/dev/bus/usb/001/040` is root:root, mode 0664, with no user ACL.
The current account is not root and has no write permission on that node.
`sudo -n true` requires interactive authentication, so no permission changes were
attempted. Descriptor discovery succeeds without opening the target; access is
insufficient for vendor commands or depth acquisition. Product naming alone does
not establish the exact retail PRO variant or firmware.

The actual bulk alternate and endpoint layout match the implementation's modern
PS1080 transport expectations. Firmware compatibility still needs a successful
read-only version query. See [device-recheck.txt](device-recheck.txt) for actual
CLI/access output. An administrator can grant narrowly scoped access to this node
or install a `1d27:0601` udev rule; no runtime or kernel driver installation is needed.

## Initial investigation (historical)

## Observed environment

Ubuntu 26.04.1 LTS (Resolute), x86_64; kernel
`7.0.0-43-generic #43-Ubuntu SMP PREEMPT_DYNAMIC`.
Rust 1.99.0 and Cargo 1.99.0 are available. The system has libusb-1.0.so.0,
but no libusb development headers/pkg-config file. Cargo therefore builds
vendored open-source libusb; this is a USB transport library, not a camera driver.

Commands executed: `uname -a`, `cat /etc/os-release`, `lsusb`, `lsusb -t`,
`usb-devices`, targeted `lsusb -d`, and sysfs enumeration.

## Actual device inventory

| USB ID | Bus/address | Identity | Binding |
|---|---|---|---|
| 1d6b:0002 | 001/001 | USB 2 root hub | xhci_hcd |
| 04f2:b729 | 001/002 | CHICONY USB2.0 Camera | uvcvideo; application interface unbound |
| 8087:0036 | 001/003 | Intel BE200 Bluetooth | btusb |
| 05e3:0610 | 001/037 | GenesysLogic USB2.0 Hub | hub |
| 275d:0ba6 | 001/038 | USB OPTICAL MOUSE | usbhid |
| 045e:0779 | 001/039 | Microsoft LifeCam HD-3000 | uvcvideo, snd-usb-audio |
| 1d6b:0003 | 002/001 | USB 3 root hub | xhci_hcd |

**No ASUS/PrimeSense sensor is enumerated.** The presence of an Xtion physically
near this laptop does not establish USB visibility. No 1d27 vendor device was
found. The LifeCam is an unrelated RGB webcam and must not receive PS1080 commands.

## Target-specific findings

VID/PID, product/manufacturer strings, USB version, configurations, interfaces,
alternate settings, endpoints, kernel bindings, libusb open permissions, and
firmware version of the requested Xtion are **unavailable** because it is absent
from the inventory. These are not inferred from the commercial name. Candidate
IDs from upstream are documented separately in protocol.md.

Discovery can run without privileges. Existing USB nodes are root-owned,
mode 0664; this does not establish permissions for an absent target. The library
reports actual target descriptor and open failures when one enumerates. It never
automatically detaches a kernel driver, resets a device, or changes configuration.

## Monitoring

`usbmon` was not loaded; reading `/sys/kernel/debug/usb/usbmon` was denied.
`sudo -n modprobe usbmon` failed: interactive authentication is required. No
security setting was changed. No bus-wide traffic was captured. Target-only Rust
transfer tracing and optional raw depth endpoint dumps provide a nonprivileged
alternative once the target is accessible. No firmware query was sent in this run.

Phase 1's physical identification criterion remains blocked by USB visibility;
subsequent work proceeds using explicitly identified upstream reference facts.
