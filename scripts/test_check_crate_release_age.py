import io
import json
import tempfile
import unittest
from contextlib import redirect_stdout
from datetime import datetime, timezone
from pathlib import Path

from check_crate_release_age import check


class ReleaseAgeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.config = self.root / "renovate.json"
        self.config.write_text(json.dumps({"minimumReleaseAge": "2 days"}))

    def lockfile(self, name, packages):
        path = self.root / name
        content = 'version = 4\n'
        for crate, version in packages:
            content += (
                '\n[[package]]\n'
                f'name = "{crate}"\n'
                f'version = "{version}"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
            )
        path.write_text(content)
        return path

    def run_check(self, *args, **kwargs):
        with redirect_stdout(io.StringIO()):
            return check(*args, **kwargs)

    def test_checks_new_versions_only(self):
        base = self.lockfile("base.lock", [("old", "1.0.0")])
        current = self.lockfile("current.lock", [("old", "1.0.0"), ("new", "1.1.0")])
        seen = []

        def lookup(name, version):
            seen.append((name, version))
            return datetime(2026, 9, 24, tzinfo=timezone.utc)

        self.assertTrue(
            self.run_check(
                base,
                current,
                self.config,
                datetime(2026, 9, 27, tzinfo=timezone.utc),
                lookup,
            )
        )
        self.assertEqual(seen, [("new", "1.1.0")])

    def test_rejects_version_younger_than_two_days(self):
        base = self.lockfile("base.lock", [])
        current = self.lockfile("current.lock", [("new", "1.1.0")])
        self.assertFalse(
            self.run_check(
                base,
                current,
                self.config,
                datetime(2026, 9, 27, tzinfo=timezone.utc),
                lambda _name, _version: datetime(2026, 9, 26, tzinfo=timezone.utc),
            )
        )

    def test_rejects_unverifiable_version(self):
        base = self.lockfile("base.lock", [])
        current = self.lockfile("current.lock", [("new", "1.1.0")])

        def lookup(_name, _version):
            raise OSError("registry unavailable")

        self.assertFalse(self.run_check(base, current, self.config, lookup=lookup))


if __name__ == "__main__":
    unittest.main()
