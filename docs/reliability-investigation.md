# Reliability issue matrix

| Priority / issue | Evidence and mechanism | Change / verification |
|---|---|---|
| P0 ambiguous enable | EP0 received empty replies while EP81 produced calibrated frames. Device-internal cause unknown. | Distinguish write failures from missing replies and verify progressing complete frames. |
| P0 shutdown | SET_PARAM stream OFF and subsequent control queries timed out. Independent endpoint reads continued. | Check state before repeat writes, join the worker on failure, verify idle and provide targeted recovery. |
| P1 reported transfer drops | Source shows try_send Full increments dropped_usb_transfers after successful USB reads. Host overflow confirmed. | Measure queue loss separately from malformed packets and physical transfer errors. |
| P1 transfer size | Earlier captures showed different completeness with 32 KiB and 1 MiB. | Compare like-for-like steady windows. |
| P1 GET_INTERFACE | Historical audio interface 2 alt 1 timed out on 1-4, and depth interface 0 control also failed. Causal attribution to audio unsupported. | Read Linux's active alternate for the claimed interface where available. |
| P1 recovery | Target-only USB resets restored control and changed device address. | Match port chain, bus and identity, verify idle after recovery. |
| P2 enumeration | Repeated earlier intervals lacked a device descriptor. | Passive bounded rediscovery. Endpoint software cannot access absent hardware. |

Confidence is high for code paths and captured outcomes, low for a firmware-internal or electrical cause. See the validation and stress reports for outcomes.

## Implemented corrections

| Issue | Root cause and confidence | Fix | Regression evidence | Status |
|---|---|---|---|---|
| Stream activation | Previous code polled EP0 shortly after changing stream mode. Confirmed timing difference; a firmware-internal reason for missing replies remains unknown. | Delay first stream-mode read to 300 ms. Separate EP0 write failures, explicit NACKs and missing replies. Verify advancing decoded frames. | Real ACKs and repeated VGA starts; deterministic late/empty/fragmented and missing-reply tests. | Fixed and verified on hardware for this unit. |
| Shutdown | Earlier EP0 timeouts prevented confirmation and sometimes left depth enabled. Late writes could be retried without verifying state. | Issue OFF while reader is active, reconcile ambiguous writes by readback, join the worker, drain residual bytes and verify silence. | Repeated explicit stops, immediate reuse and orphan recovery. | Fixed and verified on hardware for this unit. |
| Reader loss reporting | Old counter counted host queue overflow as “dropped USB transfers”. Short caller deadlines and zero-byte USB timeouts unnecessarily discarded partial frames. | Preserve partial input across timeouts. Separate terminal USB errors, overflows, sequence gaps, parser discard bytes and incomplete frames. Timestamp at USB completion. | Consumer stall, multiple real transfer sizes, two captured formats at four replay boundaries. | Fixed and verified on hardware. |
| Small transfer sensitivity | Issuing the next synchronous read late correlates with packet gaps. Concurrent reads reduced losses at the same size. Exact hardware loss location unknown. | Keep one 1 MiB synchronous read as the default. Smaller reads are diagnostic. | One vs four requests at 32/256/1024 KiB. | Mitigated with known limitations. |
| Stale ACK / short operation timeout | A response from a previous command could terminate the current one. Sub-millisecond rusb timeouts are truncated to zero. | Validate ID/opcode/length, drain a complete matching late reply, clamp control transfer timeouts to at least 1 ms. | Fragmented/coalesced and stale responses, interrupted IN and bounded ambiguous write tests. | Fixed and verified offline only for those injected cases. |
| Interface selection / audio correlation | Kernel reported audio IF2 alt1 failures during a wider EP0 failure. No audio-specific cause established. | Use claimed IF0 sysfs alternate with devnum verification. Preserve fallback and bindings. | Successful access on unchanged xHCI and snd-usb-audio configuration. | Mitigated with known limitations. |
| Recovery / reconnect | A reset can change the USB address. Recovery previously required a separate tool. | Match topology and descriptors, reopen, reconcile stream and only permit a bounded target reset explicitly. | Live orphan recovery, earlier targeted reset evidence and fault-injected identity/reset tests. | Mitigated with known limitations. |
| Physical non-enumeration | No USB descriptor or attach indication on earlier attempts. Specific electrical cause unknown. | Bound passive discovery and preserve a retryable state. | Earlier kernel enumeration history; offline absent-target tests. | Unresolved due to external constraints. |

The clean cycles qualify the combination of the new timing, reply handling and state verification. They do not isolate an internal firmware mechanism. No firmware, controller, kernel driver or power setting was modified during qualification.

The successful timing trial has a [target-only transaction trace](traces/qualified-control.log). Compare stream-mode opcode 3, parameter 6 and the matching reply ID with the [earlier unpacked attempt](traces/startup-direct-shift16.log). The trial used a development build. Final release logs are identified separately in the stress report.
