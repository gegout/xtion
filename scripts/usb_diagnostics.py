#!/usr/bin/env python3
"""Read-only USB hub and selected xHCI port diagnostics. No resets or writes."""
import json
import os
from pathlib import Path
import subprocess

SYS = Path('/sys/bus/usb/devices')


def read(path):
    try:
        return path.read_text().strip()
    except OSError as error:
        return f'unavailable: {error}'


def main():
    print(json.dumps({'euid': os.geteuid(), 'operation': 'read-only USB diagnostics'}), flush=True)
    for device in sorted(SYS.iterdir()):
        class_file = device / 'bDeviceClass'
        if not class_file.exists() or read(class_file) != '09':
            continue
        bus, address = read(device / 'busnum'), read(device / 'devnum')
        print(json.dumps({'hub': device.name, 'bus': bus, 'address': address}), flush=True)
        if not bus.isdecimal() or not address.isdecimal():
            continue
        try:
            result = subprocess.run(['/usr/bin/lsusb', '-v', '-s', f'{bus}:{address}'],
                                    capture_output=True, text=True, timeout=10,
                                    env=dict(os.environ, LC_ALL='C'))
            print(result.stdout, end='', flush=True)
            if result.stderr:
                print(result.stderr, end='', flush=True)
            print('lsusb exit:', result.returncode, flush=True)
        except subprocess.TimeoutExpired:
            print('Hub descriptor/status query timed out.', flush=True)
    debug = Path('/sys/kernel/debug/usb/xhci/0000:00:14.0/ports')
    try:
        ports = sorted(debug.glob('port*/portsc'))
        for port in ports:
            print(str(port), read(port), flush=True)
        if not ports:
            print('xHCI port debugfs entries not available.', flush=True)
    except OSError as error:
        print('xHCI debugfs:', error, flush=True)


if __name__ == '__main__':
    main()
