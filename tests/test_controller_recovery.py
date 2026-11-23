"""Offline tests; all PCI paths and driver writes use a temporary filesystem."""
import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'controller', Path(__file__).resolve().parents[1] / 'scripts/recover_controller.py')
controller = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(controller)


class ControllerRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.pci = root / '0000:00:14.0'
        self.pci.mkdir()
        self.driver = root / 'drivers/xhci_hcd'
        self.driver.mkdir(parents=True)
        for name in ['bind', 'unbind']:
            (self.driver / name).write_text('')
        (self.pci / 'driver').symlink_to(self.driver, target_is_directory=True)
        (self.pci / 'vendor').write_text('0x8086')
        (self.pci / 'device').write_text('0x51ed')
        self.patch = patch.object(controller, 'PCI', self.pci)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def fake_write(self, original):
        def write(path, text, *args, **kwargs):
            if path == self.driver / 'unbind':
                (self.pci / 'driver').unlink()
            elif path == self.driver / 'bind':
                (self.pci / 'driver').symlink_to(self.driver, target_is_directory=True)
            return original(path, text, *args, **kwargs)
        return write

    def test_normal_restart_restores_driver(self):
        with patch.object(Path, 'write_text', self.fake_write(Path.write_text)), \
             patch.object(controller.time, 'sleep'):
            controller.restart(self.driver)
        self.assertTrue(controller.attached())
        self.assertEqual((self.driver / 'unbind').read_text().strip(), controller.BDF)
        self.assertEqual((self.driver / 'bind').read_text().strip(), controller.BDF)

    def test_interrupted_restart_still_restores_driver(self):
        with patch.object(Path, 'write_text', self.fake_write(Path.write_text)), \
             patch.object(controller.time, 'sleep', side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                controller.restart(self.driver)
        self.assertTrue(controller.attached())

    def test_apply_without_broader_consent_never_unbinds(self):
        with patch('sys.argv', ['recover_controller.py', '--apply']), \
             patch.object(controller, 'devices', return_value=[]), \
             contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(SystemExit, 'Explicit consent'):
                controller.main()
        self.assertTrue(controller.attached())
        self.assertEqual((self.driver / 'unbind').read_text(), '')


if __name__ == '__main__':
    unittest.main()
