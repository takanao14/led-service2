import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / "roles/led_service2_monitor/files/led-service-check.py"
spec = importlib.util.spec_from_file_location("monitor", SOURCE)
monitor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(monitor)


class MonitorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for name, variable in [("systemctl", "ACTIVE_RC"), ("led-server", "GRPC_RC")]:
            command = self.root / name
            command.write_text(f"#!{sys.executable}\nimport os,sys\nsys.exit(int(os.getenv('{variable}', '0')))\n")
            command.chmod(0o755)
        self.environment = patch.dict(os.environ, {"PATH": str(self.root) + os.pathsep + os.environ["PATH"]})
        self.environment.start()
        self.addCleanup(self.environment.stop)

    def test_failure_replaces_success_and_refreshes_timestamp(self):
        output = self.root / "led_service.prom"
        with patch.object(monitor.time, "time", return_value=100):
            monitor.publish(output, monitor.collect(str(self.root / "led-server"), "http://localhost", "led-server", 1))
        self.assertIn("led_service_grpc_success 1", output.read_text())
        with patch.dict(os.environ, {"ACTIVE_RC": "3", "GRPC_RC": "1"}), patch.object(monitor.time, "time", return_value=200):
            monitor.publish(output, monitor.collect(str(self.root / "led-server"), "http://localhost", "led-server", 1))
        self.assertIn("led_service_active 0", output.read_text())
        self.assertIn("led_service_grpc_success 0", output.read_text())
        self.assertIn("led_service_check_timestamp_seconds 200.000", output.read_text())
        self.assertEqual(output.stat().st_mode & 0o777, 0o644)
        self.assertEqual(list(self.root.glob(".led-service-*")), [])

    def test_missing_binary_and_timeout_are_failed_probes(self):
        self.assertEqual(monitor.succeeds([str(self.root / "missing")], 1), 0)
        self.assertEqual(monitor.succeeds([sys.executable, "-c", "import time; time.sleep(5)"], 0.05), 0)

    def test_failed_publish_keeps_previous_snapshot(self):
        output = self.root / "led_service.prom"
        output.write_text("previous\n")
        with patch.object(monitor.os, "replace", side_effect=OSError("read-only")):
            with self.assertRaises(OSError):
                monitor.publish(output, "new\n")
        self.assertEqual(output.read_text(), "previous\n")
        self.assertEqual(list(self.root.glob(".led-service-*")), [])

    def test_cli_probe_failure_still_publishes_metrics(self):
        output = self.root / "led_service.prom"
        result = subprocess.run([sys.executable, str(SOURCE), "--binary", str(self.root / "missing"),
                                 "--url", "http://localhost", "--output", str(output)], check=False)
        self.assertEqual(result.returncode, 0)
        self.assertIn("led_service_grpc_success 0", output.read_text())


if __name__ == "__main__":
    unittest.main()
