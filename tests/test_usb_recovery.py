"""Offline safeguards for the explicitly requested privileged port recovery helper."""
import contextlib
import importlib.util
import io
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'recovery', Path(__file__).resolve().parents[1] / 'scripts/recover_usb.py')
recovery = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(recovery)


class RecoverySafetyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.ports = []
        for bus in [1, 2]:
            hub = self.root / f'usb{bus}'
            port = hub / f'{bus}-0:1.0' / f'usb{bus}-port2'
            port.mkdir(parents=True)
            (hub / 'busnum').write_text(str(bus))
            (hub / 'devnum').write_text('1')
            (port / 'disable').write_text('0')
            (port / 'state').write_text('not attached')
            self.ports.append(port)
        (self.ports[0] / 'peer').symlink_to(self.ports[1], target_is_directory=True)

    def run_helper(self, found=None):
        descriptor = SimpleNamespace(stdout='Hub Descriptor:\nPer-port power switching', stderr='', returncode=0)
        with patch.object(recovery, 'SYS', self.root), \
             patch.object(recovery.os, 'geteuid', return_value=0), \
             patch.object(recovery.time, 'sleep'), \
             patch.object(recovery.subprocess, 'run', return_value=descriptor) as usb, \
             patch.object(recovery, 'discover', side_effect=found or (lambda: [])), \
             patch('sys.argv', ['recover_usb.py', '--apply']), \
             contextlib.redirect_stdout(io.StringIO()):
            recovery.main()
        return usb

    def test_occupied_companion_prevents_any_restart(self):
        child = self.root / '2-2'
        child.mkdir()
        (child / 'idVendor').write_text('05e3')
        (child / 'idProduct').write_text('0620')
        usb = self.run_helper()
        usb.assert_not_called()
        self.assertEqual([(p / 'disable').read_text() for p in self.ports], ['0', '0'])

    def test_empty_port_pair_is_reenabled_after_cycle(self):
        self.run_helper(found=iter([[], [{'idVendor': '1d27', 'idProduct': '0601'}]]))
        self.assertEqual([(p / 'disable').read_text().strip() for p in self.ports], ['0', '0'])

    def test_partial_disable_failure_restores_both_ports(self):
        original = Path.write_text
        writes = []
        def fail_peer(path, text, *args, **kwargs):
            writes.append((path, text))
            if path == self.ports[1] / 'disable' and text.strip() == '1':
                raise PermissionError('synthetic port write failure')
            return original(path, text, *args, **kwargs)
        with patch.object(Path, 'write_text', fail_peer):
            self.run_helper(found=iter([[], [{'idVendor': '1d27', 'idProduct': '0601'}]]))
        for port in self.ports:
            self.assertIn((port / 'disable', '0\n'), writes)
            self.assertEqual((port / 'disable').read_text().strip(), '0')


if __name__ == '__main__':
    unittest.main()
