#!/usr/bin/env python3
"""Review a shared USB controller restart; apply only with explicit broader consent.

This temporarily interrupts all USB devices on Intel 0000:00:14.0. It does not
install a driver, flash firmware, reboot the laptop, or promise a VBUS power cut.
Default operation only prints the concrete plan. Both flags and root are required.
"""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time

BDF = '0000:00:14.0'
PCI = Path('/sys/bus/pci/devices') / BDF
USB = Path('/sys/bus/usb/devices')


def read(path):
    try:
        return path.read_text().strip()
    except OSError:
        return None


def attached():
    try:
        return (PCI / 'driver').resolve(strict=True).name == 'xhci_hcd'
    except OSError:
        return False


def devices():
    result = []
    for device in sorted(USB.iterdir()):
        if (device / 'idVendor').exists() and device.resolve().is_relative_to(PCI.resolve()):
            result.append({'path': device.name, 'vendor': read(device / 'idVendor'),
                           'product_id': read(device / 'idProduct'),
                           'product': read(device / 'product')})
    return result


def restart(driver):
    """Always attempt to rebind the original driver after a successful unbind."""
    try:
        (driver / 'unbind').write_text(BDF + '\n')
        time.sleep(2)
    finally:
        # Defer ordinary termination while restoring the controller binding.
        mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
        try:
            for _ in range(3):
                if attached():
                    break
                try:
                    (driver / 'bind').write_text(BDF + '\n')
                except OSError as error:
                    print(f'Rebind attempt: {error}', flush=True)
                if attached():
                    break
                time.sleep(1)
            if not attached():
                raise RuntimeError(f'Rebind failed. Restore with: echo {BDF} | sudo tee {driver}/bind')
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, mask)



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    parser.add_argument('--allow-unrelated-usb-disconnect', action='store_true')
    args = parser.parse_args()
    if read(PCI / 'vendor') != '0x8086' or read(PCI / 'device') != '0x51ed':
        raise SystemExit('Controller identity differs from the reviewed Intel 8086:51ed; refusing.')
    if not attached():
        raise SystemExit('Expected controller driver is not currently bound; refusing.')
    driver = (PCI / 'driver').resolve(strict=True)
    print(json.dumps({'controller': BDF, 'driver': str(driver), 'affected_devices': devices(),
                      'apply': args.apply, 'electrical_power_cycle_verified': False}, indent=2), flush=True)
    if not args.apply:
        return
    if not args.allow_unrelated_usb_disconnect:
        raise SystemExit('Explicit consent to interrupt the listed unrelated USB devices is required.')
    if os.geteuid() != 0:
        raise SystemExit('Administrator authentication is required.')
    def interrupt(_signum, _frame):
        raise KeyboardInterrupt
    previous = {sig: signal.signal(sig, interrupt) for sig in [signal.SIGINT, signal.SIGTERM]}
    try:
        restart(driver)
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)
    settle = subprocess.run(['/usr/bin/udevadm', 'settle', '--timeout=15'], check=False)
    print('udev settle exit:', settle.returncode, flush=True)
    print(json.dumps({'driver_restored': attached(), 'devices_after': devices()}, indent=2), flush=True)


if __name__ == '__main__':
    main()
