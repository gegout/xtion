# Compatibility matrix

## Hardware and Linux

The tested target is ASUS Xtion PRO, 1d27:0601, PS1080 0x00202020, FPGA 4, system 15, firmware 5.8.22. It negotiated USB 2 high speed on direct port 1-2 of Intel 8086:51ed xHCI at 0000:00:14.0. Configuration 1 exposes interface 0 alt 0 with bulk depth IN 0x81. Interface 0 has no kernel driver. Interfaces 1/2 have snd-usb-audio. During qualification power/control was on and runtime_status active. No runtime power policy, kernel binding or controller setting was changed.

Earlier kernel lines on port 1-4 explicitly identified interface 2 alt 1 in repeated SET_INTERFACE failures (-110) and audio endpoint 0x84 rate-setting errors. GET_INTERFACE on depth interface 0 also failed before any PS firmware command in that session. These different standard/class transactions share the device control endpoint. The kernel log establishes temporal correlation. Causation by the audio driver is unproven. The ALSA code prints the interface number and requested alternate in that order. [Linux endpoint_set_interface](https://code.googlesource.com/linux/torvalds/linux/%2B/e557793799c5a8406afb08aa170509619f7eac36/sound/usb/endpoint.c) The Linux transport now uses bAlternateSetting from the kernel after checking the enumerated devnum and claiming the interface. Standard GET_INTERFACE remains a fallback. SET_INTERFACE is issued only when necessary.

## Tested configurations

| Case | Result | Evidence |
|---|---|---|
| Packed 11-bit VGA/30, 1 MiB synchronous bulk | Default. Three development runs sustained 29.968 complete FPS. Final release results separately recorded. | [Stress results](stress-results.md) |
| Unpacked 16-bit VGA/30, 1 MiB | 29.77 and 29.87 complete FPS over 10 s each, 2 and 1 incomplete frames. Both stops verified. | [Raw results](traces/reliability-unpacked.jsonl) |
| Packed, 256 KiB synchronous | 2.52 complete FPS, 240 incomplete, 387 observed sequence gaps in 8.74 s. | [Raw results](traces/reliability-packed256.jsonl) |
| Packed, 32 KiB synchronous | 5.60 complete FPS, 208 incomplete, 1,005 observed gaps in 8.58 s. | [Raw results](traces/reliability-packed32.jsonl) |
| Multiple outstanding reads | Four 32 KiB requests delivered 119 full wire frames in 4 s. One delivered 3 in a separate 4 s window. Four 256 KiB requests delivered 119. | [Independent endpoint benchmark](traces/reliability-orphan.log) |
| Slow consumer | A two-second consumer pause caused 52 bounded queue drops. Further calibrated frames, normal stop and fresh capture all succeeded. | [Slow-consumer log](traces/reliability-slow.log) |
| Abnormal CLI exit | SIGTERM during active depth left firmware responsive and EP81 transmitting. Reopen, ordinary OFF and silence verification recovered idle without reset. | [Recovery log](traces/reliability-orphan.log) |
| Missing device / address change | Physical disappearance and address changes in earlier sessions; current bounded policy exercised by deterministic injected faults. | [Device history](device.md), [recovery design](recovery-design.md) |
| Hub, alternate physical port, other firmware or PS1080A6 | Historical failures on some connections. No sustained qualification on these configurations. | [Device history](device.md) |

The comparative endpoint benchmark used an external Rust program and nusb 0.2.9 to claim only IF0 of the already-streaming target. It read EP81 for four seconds per transfer-size/window combination, checked packet IDs, start/end and exact packed payload length, cancelled outstanding reads and released the interface. It performed no PS firmware writes. Its full-frame counts describe wire assembly. The synchronous stress results additionally require successful decode through the public API. Results support a scheduling/endpoint-service contribution on this host. They do not locate each dropped byte in the host controller or device. The existing synchronous 1 MiB path meets the packed target with no additional transport dependency.

During the second final streaming window, sysfs showed usbfs owning IF0 on behalf of the userspace handle and snd-usb-audio owning IF1 and IF2. The port over-current counter was 0. At one observation urbnum was 33,208 and active/connected durations both read 1,687,256 ms. urbnum counts requests and does not count failed transactions. The earlier EMI query in a kernel diagnostic string did not identify a physical fault mechanism.

Two hardware-backed fault tests suppressed command acknowledgments or injected a reader disconnection inside the test binary. The real sensor supplied all frames, readbacks and subsequent captures. Both queries of the independent diagnostic process (after fault tests and after orphan recovery) returned idle. Physical unplug during this qualification and an electrical failure requiring a fresh reset were not reproduced. [Fault test output](traces/final-fault-injection.log)

## Transfer memory and timing

Measured host latency spans USB completion to frame delivery. USB scheduling, sensor integration and optical exposure precede that interval. RSS is sampled process memory.

| Synchronous read size | Window | Peak RSS KiB | Maximum host latency ms | Successful bytes/s |
|---:|---:|---:|---:|---:|
| 32 KiB | 8.58 s | 4756 | 3 | 12,168,362 |
| 256 KiB | 8.74 s | 4592 | 3 | 12,461,303 |
| 1024 KiB | 120.03 s | 6880 | 5 | 12,707,198 |

The asynchronous comparison reserved one or four endpoint buffers of the specified size. Completion counts and measured elapsed times are retained in the benchmark log. These are transfer-service intervals, not optical latency. No separate asynchronous CPU or RSS measurement was taken.

Motion response uses the earlier user-confirmed spatial test on this physical unit. The central region changed from 629 mm to 510.5 mm and the three horizontal regions had different near-pixel counts. The current decoder also replays the original packed capture and its calibration through four different USB chunk boundaries. Exact hand trajectory, step length and absolute depth accuracy remain unmeasured. [Regional motion record](traces/packed-movement.csv)
