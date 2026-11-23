# Hardware reliability results

## Method

The stress CLI accepts steady acquisition seconds per cycle and a cycle count. Every cycle uses the same owning sensor, waits for two valid advancing frames, excludes startup data and a one-second warm-up, and measures from the first qualifying USB completion. The frame count uses intervals between frames; the first frame establishes the baseline. Raw EOF ticks are differenced with wrapping arithmetic. Host completion, consumer delivery and firmware-clock FPS are distinct. Complete-frame rate counts only fully reconstructed frames. No frequency is inferred when a device reports none.

Counters in a stress row are steady-window differences. Missing tick intervals are estimates from the nominal cadence and the returned device clock in the final suite. Earlier development logs used median observed tick cadence, which can underestimate loss in the small-buffer trials. Sequence gaps, parser discard bytes and incomplete frames expose that loss directly. No-image USB timeouts during initialization do not contribute to steady-state results.

Every successful stop reads back volatile settings and verifies endpoint silence. A fresh diagnostic additionally verifies firmware and idle state after each group. CPU times include initialization and shutdown. RSS values are Linux process samples, not allocator proofs. Startup permits bounded buffering of eight transfer slots plus the in-flight read. The parser limit is 2 MiB. Full-frame memory and caller-owned frames are additional.

## Development runs

* Twenty consecutive start/stop cycles on one handle passed. [Cycle records](traces/reliability-cycles20.jsonl)
* Each of three approximately 120.03 s packed windows delivered 3,597 complete frame intervals, 29.968 FPS and about 12.71 MB/s. Each reported zero incomplete frames, queue drops, reader errors and estimated missing intervals. [Run A](traces/reliability-packed120a.jsonl), [run B](traces/reliability-packed120b.jsonl), [run C](traces/reliability-packed120c.jsonl)
* Unpacked depth used approximately 18.48 MB/s. Small synchronous requests performed worse despite transferring substantial data. Comparative wire results and public API measurements are in the [configuration matrix](compatibility-matrix.md).
* Host-side queue overflow was reproduced with a deliberately slow consumer. Abrupt CLI termination was recovered without resetting the sensor. Only the interrupted process lost its parameter snapshot. The test restored and verified the pre-test format, resolution and FPS values from a captured configuration trace. [Restore readbacks](traces/reliability-orphan-restore.log)

## Final release qualification

All six groups used the release binary with SHA-256 0fdc9c8711cfdafe6302dafcfc9fb80f9e8b59737cca29936131c2eb7dbe4520. The direct target stayed at the same address. The 20-cycle reuse test passed all stops on one handle with zero startup ACK timeouts. [Reuse records](traces/final-reuse.jsonl), [independent idle checks](traces/final-idle.jsonl)

| Run | Frame intervals | USB-completion FPS | Device-clock FPS | Incomplete | Estimated lost intervals | Queue drops | USB bytes/s | CPU user/system s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| [two-minute-a](traces/final-two-minute-a.jsonl) | 3,597 | 29.968 | 29.968 | 0 | 0 | 0 | 12,708,576 | 10.30 / 1.97 |
| [two-minute-b](traces/final-two-minute-b.jsonl) | 3,597 | 29.968 | 29.968 | 0 | 0 | 0 | 12,708,597 | 10.20 / 1.91 |
| [two-minute-c](traces/final-two-minute-c.jsonl) | 3,594 | 29.943 | 29.943 | 3 | 3 | 0 | 12,707,198 | 7.75 / 1.48 |
| [ten-minute](traces/final-ten-minute.jsonl) | 17,979 | 29.965 | 29.964 | 2 | 2 | 0 | 12,708,490 | 52.49 / 9.72 |
| [restart](traces/final-restart.jsonl) | 60 | 29.964 | 29.968 | 0 | 0 | 0 | 12,706,734 | 0.25 / 0.05 |

Over 600.002 seconds the ten-minute run reported 2 sequence gaps, 0 decoder errors, 3072 discarded parser bytes, 0 terminal USB errors and 0 USB read timeouts in its steady window. Maximum observed completion-to-delivery latency was 8 ms. Timed process data include startup and verified stop. [Process times](traces/final-ten-minute.time)

Independent ten-second samples ranged from 6,876 to 6,960 KiB RSS. The stress command also retained a bounded list of one tick interval per frame for its statistics. This ten-minute observation cannot establish indefinite stability. [Memory and thread samples](traces/final-ten-minute-memory.csv)

A new process acquired calibrated frames after the long stop, stopped normally and passed a new idle diagnosis. Hardware-backed fault injection independently verified recovery with missing replies and a simulated reader disconnect. [Fault injection](traces/final-fault-injection.log)

## Package checks

Formatting, Clippy with warnings denied, 30 ordinary Rust tests, the exclusive-borrow compile-fail doctest and six Python recovery tests passed. The two opt-in Rust hardware fault tests also passed. Cargo built the release binary and packaged and verified the crate offline. Cargo reported its advisory about missing package homepage/repository/documentation metadata. This workspace has no Git repository metadata.

The endurance executable and the subsequent packaging rebuild have different hashes. Test-only fault hooks, a private reader borrow change for injection, documentation and packaging exclusions were added while qualification ran. The rebuilt executable separately passed native probe, calibrated capture, three threaded start/stop cycles and an independent idle diagnosis. The hashes and the distinct validation scopes are recorded to make the results reproducible. [Packaging smoke test](traces/final-package-smoke.log)
