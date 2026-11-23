#!/usr/bin/env python3
"""Inspect/restart unoccupied USB ports; never reset the shared host controller.

Uses the documented sysfs port/disable interface. Default operation is read-only.
--apply requires root via an ordinary administrator authentication mechanism.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

SYS = Path('/sys/bus/usb/devices')
CANDIDATES = {'0500', '0600', '0601', '0609'}


def read(path):
    try:
        return path.read_text().strip()
    except OSError:
        return None


def identity(path):
    vendor = read(path / 'idVendor')
    if vendor is None:
        return None
    return vendor, read(path / 'idProduct')


def is_target(pair):
    return pair is not None and pair[0] == '1d27' and pair[1] in CANDIDATES


def discover():
    found = []
    for device in sorted(SYS.iterdir()):
        if is_target(identity(device)):
            found.append({key: read(device / key) for key in
                          ['idVendor', 'idProduct', 'manufacturer', 'product',
                           'busnum', 'devnum', 'speed']}
                         | {'sysfs': str(device)})
    return found


def child_of(port):
    name = port.name
    if name.startswith('usb'):
        bus, number = name[3:].split('-port')
        return SYS / f'{bus}-{number}'
    hub, number = name.split('-port')
    return SYS / f'{hub}.{number}'


def group(port):
    paths = [port]
    peer = port / 'peer'
    if peer.exists():
        paths.append(peer.resolve())
    return paths


def available(ports):
    for port in ports:
        pair = identity(child_of(port))
        if pair is not None and not is_target(pair):
            return False, f'{child_of(port)} occupied by {pair[0]}:{pair[1]}'
        if read(port / 'disable') != '0':
            return False, f'{port.name} not initially enabled'
    return True, 'no unrelated child devices'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    if args.apply and os.geteuid() != 0:
        raise SystemExit('Administrator authentication is required for --apply.')
    report = {'apply': args.apply, 'euid': os.geteuid(), 'found_before': discover(), 'ports': []}
    if report['found_before']:
        print(json.dumps(report, indent=2), flush=True)
        return
    # Known former direct Xtion port first. Do not cycle occupied root port 4:
    # it currently serves the hub, mouse and LifeCam.
    root = SYS / 'usb1' / '1-0:1.0'
    ordered = [root / 'usb1-port2'] + [
        root / f'usb1-port{n}' for n in range(1, 13) if n != 2
    ]
    # Also inspect the formerly used downstream port, without resetting its hub.
    downstream = SYS / '1-4:1.0' / '1-4-port4'
    if downstream.exists():
        ordered.append(downstream)
    processed = set()
    for port in ordered:
        if not (port / 'disable').exists() or str(port.resolve()) in processed:
            continue
        ports = group(port)
        processed.update(str(p.resolve()) for p in ports)
        safe, reason = available(ports)
        item = {'port': str(port), 'companions': [str(p) for p in ports],
                'state': read(port / 'state'), 'eligible': safe, 'reason': reason}
        report['ports'].append(item)
        print(json.dumps(item), flush=True)
        if not args.apply or not safe:
            continue
        # Distinguish individual, absent and ganged power switching.
        # Logical disable/enable requests do not prove electrical VBUS removal.
        supported = True
        item['hub_capabilities'] = []
        item['electrical_power_cycle_verified'] = False
        item['individual_power_switching_advertised'] = True
        checked_hubs = set()
        for member in ports:
            name = member.name
            hub_name = 'usb' + name[3:].split('-port')[0] if name.startswith('usb') else name.split('-port')[0]
            if hub_name in checked_hubs:
                continue
            checked_hubs.add(hub_name)
            hub = SYS / hub_name
            bus, address = read(hub / 'busnum'), read(hub / 'devnum')
            if bus is None or address is None:
                supported = False
                break
            desc = subprocess.run(['/usr/bin/lsusb', '-v', '-s', f'{bus}:{address}'],
                                  capture_output=True, text=True, timeout=15,
                                  env=dict(os.environ, LC_ALL='C'))
            item['hub_capabilities'].append({'hub':hub_name,'exit':desc.returncode,'power_lines':[line.strip() for line in desc.stdout.splitlines() if 'power switching' in line.lower() or 'wHubChar' in line], 'stderr':desc.stderr.strip()})
            if re.search(r'no power switching', desc.stdout, re.I):
                item['individual_power_switching_advertised'] = False
            if not re.search(r'(?:per-port|individual port|no) power switching', desc.stdout, re.I):
                supported = False
                break
        if not supported:
            item['result'] = 'individual power switching not confirmed on both companions; skipped'
            print(json.dumps(item), flush=True)
            continue
        changed = []
        try:
            # Recheck immediately before the mutation, including USB3 companions.
            safe, reason = available(ports)
            if not safe:
                item['result'] = reason
                continue
            for p in ports:
                changed.append(p)
                (p / 'disable').write_text('1\n')
            item['disable_readback'] = [read(p / 'disable') for p in ports]
            print(f'Sent disable requests only to {port.name} and its empty companions; waiting 2 seconds', flush=True)
            time.sleep(2)
        except OSError as error:
            item['error'] = str(error)
        finally:
            # Always re-enable every port successfully disabled by this invocation.
            for p in reversed(changed):
                try:
                    (p / 'disable').write_text('0\n')
                except OSError as error:
                    item.setdefault('restore_errors', []).append(str(error))
        item['result'] = ('disable failed; restoration attempted' if item.get('error') else 'disable/enable requests completed') if changed else 'not changed'
        item['enable_readback'] = [read(p / 'disable') for p in ports]
        item['state_after'] = read(port / 'state')
        print(json.dumps(item), flush=True)
        if item.get('restore_errors'):
            raise SystemExit('Port restoration failed; stop and inspect the printed paths.')
        for _ in range(6):
            found = discover()
            if found:
                report['found_after'] = found
                print(json.dumps(report, indent=2), flush=True)
                return
            time.sleep(.5)
    report['found_after'] = discover()
    print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()
