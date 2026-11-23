# USB recovery findings

## Completed targeted recovery

The user ran `sudo python3 scripts/recover_usb.py --apply`. Linux accepted
logical disable/enable requests on eligible root ports and USB3 companions;
all inspected ports are enabled afterward. The Xtion did not enumerate.
`docs/port-recovery-result.json` records the scope and result.

Both root hubs advertise `No power switching` (0x000a). The script's original
`cycled` label meant disable/enable requests were sent; it did **not** verify that
VBUS power was removed. The helper now distinguishes these facts and records
readbacks. The external Genesys hub advertises ganged power switching (0x00e0),
so its downstream power cycle was skipped to protect its mouse and webcam.
All inspected `early_stop` flags are `no`; authorization is enabled, overcurrent
counts are zero, and the candidate ports remain `not attached`.

Linux documents the [port disable interface](https://github.com/torvalds/linux/blob/master/Documentation/ABI/testing/sysfs-bus-usb).
It cannot guarantee an electrical power cut when hardware lacks power switching.
No port restart has brought this current connection back. Earlier native firmware
queries and the 60-frame depth recording remain separate, valid observations.

## Prepared broader diagnostic — not executed

`scripts/recover_controller.py` prints a read-only plan by default. It targets
only the reviewed Intel 8086:51ed controller at `0000:00:14.0`, currently bound to
`xhci_hcd`. A restart would briefly disconnect the USB mouse, LifeCam, built-in
camera, Bluetooth and their hubs. It rebinds the existing driver; it installs
nothing and does not flash firmware or guarantee VBUS removal.

The user's original instruction protects unrelated USB devices, so explicit
approval of this broader disruption is required. The helper requires both
`--apply` and `--allow-unrelated-usb-disconnect` plus root authentication. It
attempts to restore the binding even after an ordinary interruption. Shared
controller recovery has **not** been run and is not a confirmed solution.

Six offline Python tests cover occupied-port protection, partial-write recovery,
controller rebinding after interruption, and refusing execution without broader
consent. Run `python3 -m unittest discover -s tests -p 'test_*recovery.py' -v`.
