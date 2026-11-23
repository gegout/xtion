# PS1080 protocol research

## Qualified bulk path

The current implementation uses IF0 EP81, packed 11-bit VGA shifts by default and a 300 ms receive delay on stream-mode writes. A complete write with missing reply is checked against two progressing complete frames. Shutdown requires setting readback and endpoint silence. Frequency algorithm 0x80 (request words 0x80, 0, 0, 0, 0 via opcode 22) supplies a little-endian float in ticks/µs. See [upstream source review](upstream-bugs.md) and [hardware measurements](stress-results.md). The remaining sections include earlier experiments.

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


## Current live descriptor evidence

The reconnected device is now identified as `1d27:0601`, USB 2.00 at 480 Mbit/s,
PrimeSense manufacturer/product strings. Interface 0 alternate 0 exposes bulk IN
`0x81` and `0x82` (512-byte packets); sensor interface 0 is unbound. This confirms
the modern transport's descriptor layout only. libusb open is denied by device-node
permissions, so firmware, protocol replies, calibration and depth remain unverified.
See device.md. Statements about initial absence below are historical research context.

## Evidence and scope (initial research)

No target enumerates on this host (see device.md). **All protocol details below
are source-confirmed, not live-confirmed.** The implemented initialization is an
experimental subset, not a claim of hardware compatibility. No target-specific
traffic or captured fixture exists yet.

Sources downloaded for reading only to `/tmp/xtion-reference`; nothing from these
projects was built, installed, or run:

- [OpenNI2](https://github.com/OpenNI/OpenNI2/tree/115cf06c6efea32304182d293eca16ca883c9150), commit `115cf06c6efea32304182d293eca16ca883c9150`.
- [PrimeSense Sensor](https://github.com/PrimeSense/Sensor/tree/60170bfe8de166b2947ea2d604506f0bdfa0565c), commit `60170bfe8de166b2947ea2d604506f0bdfa0565c`.
- [SensorKinect](https://github.com/avin2/SensorKinect/tree/15f1975d5e50d84ca06ff784f83f8b7836749a7b), commit `15f1975d5e50d84ca06ff784f83f8b7836749a7b`.
- [OpenNI](https://github.com/OpenNI/OpenNI/tree/1e9524ffd759841789dadb4ca19fb5d4ac5820e7), commit `1e9524ffd759841789dadb4ca19fb5d4ac5820e7`: framework/API, not the sensor wire implementation.

OpenNI2's Apache-2.0 PS1080 sources are the implementation reference. PrimeSense
Sensor corroborates IDs and protocol; SensorKinect's examined protocol header is
LGPL-3.0-or-later, and its device IO selects Microsoft Kinect IDs. No SensorKinect
code is incorporated. The Rust implementation expresses protocol facts and the
published geometric conversion independently; attribution is in NOTICE.

## Discovery and transport

Paths below are relative to OpenNI2 `Source/Drivers/PS1080/` unless stated.

[Sensor/XnDeviceEnumeration.cpp](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnDeviceEnumeration.cpp)
`ms_supportedProducts` lists `1d27:0500`, `0600`, `0601`, and `0609`.
They are candidate PS1080 IDs, not verified identities for the absent device.
The matching older list is Sensor `Source/XnDeviceSensorV2/XnDeviceSensorIO.cpp`.
Actual firmware/product support cannot be selected without the device.

[Sensor/XnDeviceSensorIO.cpp](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnDeviceSensorIO.cpp),
`OpenDevice` and `OpenDataEndPoints`, use EP0 vendor control on modern devices;
old devices have bulk commands OUT 0x04 / IN 0x85. The executable code chooses
modern **depth IN 0x81**, image IN 0x82, misc IN 0x83. A nearby historical comment
reverses image/depth; the actual endpoint-opening code is the authority.
Xtion rejects old bulk command layouts and requires a descriptor-confirmed bulk
0x81 on interface 0. Isochronous-only layouts are unsupported by this synchronous
rusb implementation. No configuration switch or kernel detach is performed.

`Sensor/XnHostProtocol.cpp::XnHostProtocolInitFWParams` selects bulk alternate 1
for firmware 5.0–5.4 and alternate 0 for >=5.5; Xtion checks these against actual
descriptors, and only attempts firmware 5.1–5.8 with PS1080/PS1080A6 chip IDs.
This restriction bounds untested behavior; it is not a measured compatibility matrix.

## Commands and replies

[Sensor/XnHostProtocol.h](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnHostProtocol.h),
`XnHostProtocolHeaderV26`, `EPsProtocolOpCodes`, `XnHostProtocolReplyHeader`;
and `.cpp::{InitHeader,USBSend,USBReceive,ValidateReplyV26}` establish:

- EP0 OUT bmRequestType 0x40; IN 0xc0; bRequest, wValue, wIndex all zero.
- Eight-byte little-endian header: magic u16, payload length in **16-bit words**
  u16, opcode u16, request ID u16. Host magic 0x4d47; reply magic 0x4252.
- Reply word count includes a u16 status before returned data; zero is ACK,
  13 means NOT_READY. Verify magic, word count, opcode, ID, and status.
- Commands may need multiple IN reads. Xtion bounds response length, polling,
  and retries; NOT_READY can retry the same request ID. It does not blindly
  resend writes after ambiguous timeouts.
- Version opcode 0 returns minor then major bytes, a little-endian build
  (BCD for >=5), chip u32, FPGA u16, system u16. This is not USB bcdDevice.

Implemented safe subset:

| Opcode | Purpose | Payload |
|---|---|---|
| 0 | Get version | empty |
| 1 | Keep alive | empty |
| 2 | Get parameter | u16 ID |
| 3 | Set volatile parameter | u16 ID, u16 value |
| 4 | Read fixed calibration | u16 offset in 32-bit words |
| 5 | Get operating mode | empty; require PS mode 1 |
| 22 | Read algorithm parameters | five u16: ID, format=0, resolution, FPS, offset in 16-bit words |

No firmware upload, flash writes, I2C/AHB writes, resets, maintenance modes, emitter
calibration, laser power changes, or protection changes are implemented.

## Minimal depth startup and shutdown

[Sensor/XnParams.h](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnParams.h),
`Include/XnStreamParams.h`, root `Include/PS1080.h`, and
[Sensor/XnSensorDepthStream.cpp](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnSensorDepthStream.cpp)
`ConfigureStreamImpl`, `OpenStreamImpl`, `CloseStreamImpl` define:

1. Claim only unbound interface 0; read version and mode. Require idle streams
   (parameters 5,6,7 = 0), rather than take over another stream.
2. Keep alive; select verified bulk alternate. Save every volatile parameter
   before changing it and restore on stop or a failed start.
3. Set depth format 18=0 (uncompressed 16-bit **shifts**, not distances),
   resolution 19=1 (VGA), FPS 20=30; registration 2=0, mirror 23=0,
   decimation 24=0, cropping 55=0. Preserve factory gain/hole-filter/GMC defaults.
4. Set stream1 parameter 6=2 (depth). Stop using 6=0; restore changed parameters
   and alternate setting; release the interface. Explicit stop reports failures;
   Drop makes a best-effort retry. No reset is used for recovery.

Upstream also configures correction and filtering properties and fetches CMOS
blanking information. Xtion deliberately leaves those device defaults intact;
whether further initialization is necessary remains a live validation question.

## Streaming and reconstruction

[Sensor/XnDeviceSensorProtocol.h](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/Sensor/XnDeviceSensorProtocol.h)
`XnSensorProtocolResponseHeader` and `.cpp::XnDeviceSensorProtocolUsbEpCb`:

| Offset | Size | Meaning |
|---|---|---|
| 0 | 2 | magic 0x4252, little endian |
| 2 | 2 | packet type, little endian |
| 4 | 2 | packet sequence ID, little endian |
| 6 | 2 | **big endian total byte length, including header** |
| 8 | 4 | timestamp/padding, little endian |

Depth types: 0x7100 start, 0x7200 data, 0x7500 end. USB read boundaries are
independent of protocol packet/frame boundaries. Start and end packets can carry
pixel payloads. Track sequence IDs and reject incomplete, oversized, odd-sized,
or interrupted frames; recover at the next start. ID zero is a permitted upstream
sequence restart (`Sensor/XnDataProcessor.cpp`).

`Sensor/XnFrameStreamProcessor.cpp` uses the **end** packet timestamp.
`Sensor/XnDepthProcessor.cpp::OnStartOfFrame` says that firmware >=5.1 uses the
start timestamp field as two padding counts: high 16 bits prepend invalid pixels,
low 16 bits append invalid pixels. Xtion preserves EOF ticks as u32 without assuming
clock rate or converting them to time. Host arrival uses a separate monotonic clock.
No timestamp synchronization/rollover conversion is claimed.

`Sensor/XnUncompressedDepthProcessor.cpp` treats values >=2047 as invalid;
Xtion normalizes these and zero to zero. Other values are raw structured-light
shifts. Packed 11/12-bit and PrimeSense compressed formats are known from
`XnPacked11DepthProcessor.cpp`, `XnPacked12DepthProcessor.cpp`, and
`XnPSCompressedDepthProcessor.cpp`, but are deliberately not selected or decoded.

## Calibration and physical units

`Sensor/XnDeviceSensor.h::XnFixedParams` places emitter baseline float32 at byte 92,
reference distance at 100, reference pixel size at 104. Read-only opcode 4 fetches
these in chunks with offsets measured in four-byte words. Algorithm 0 returns
`XnDepthInformation::nConstShift` (u16) for VGA/30. Reference pixel-size factor is 1
for depth VGA/30 with other streams off
(`XnSensorFirmwareParams.cpp::RecalculateReferenceResolution`).

[DDK/XnShiftToDepth.cpp](https://github.com/OpenNI/OpenNI2/blob/115cf06c6efea32304182d293eca16ca883c9150/Source/Drivers/PS1080/DDK/XnShiftToDepth.cpp),
`XnShiftToDepthUpdate`, and `XnSensorDepthStream.cpp` constants describe:

```
metric = ((shift - 4 * const_shift) / 4 - 0.375) * reference_pixel_size
millimetres = 10 * reference_distance * baseline / (baseline - metric)
```

Reference distance is truncated to an integer as in `XnSensorFixedParams.cpp`.
Xtion builds a LUT only from finite, positive device-read geometry and nonzero
constant shift. Invalid or out-of-range results (>=10000 mm) map to zero.
If calibration cannot be read or validated, frames stay explicitly `RawShift` and
the calibration error is exposed. Mathematical conversion is source-grounded;
physical accuracy, thermal/wavelength correction and range have **not** been measured.

## Capture status and unresolved work

usbmon capture is blocked by privileges and no target. `XTION_TRACE=1` prints
commands and replies for the selected target only. `XTION_DUMP=path.usbraw`
records successfully read bulk endpoint bytes without USB URB headers, using a
new file only. Neither is evidence of a capture until run on actual hardware.
No speculative commands were sent. The device's actual descriptors, firmware,
startup requirements, image stability, throughput, and calibration accuracy
remain unknown. No movement request is useful until frames can be acquired.
