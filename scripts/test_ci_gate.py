"""Regression coverage for the merge gate's failure and skip policy."""

import unittest

from check_ci_gate import REQUIRED_JOBS, failures


class CiGateTests(unittest.TestCase):
    def needs(self, job=None, result=None):
        needs = {name: {"result": "success"} for name in REQUIRED_JOBS}
        if job:
            needs[job]["result"] = result
        return needs

    def test_all_success(self):
        self.assertEqual(failures(self.needs(), "pull_request", False), [])

    def test_every_failure_cancellation_and_unexpected_skip_blocks(self):
        for job in REQUIRED_JOBS:
            for result in ("failure", "cancelled", "skipped", None):
                with self.subTest(job=job, result=result):
                    self.assertTrue(
                        failures(self.needs(job, result), "pull_request", False)
                    )

    def test_only_intentional_age_skip_is_allowed(self):
        for event, security in (("pull_request", True), ("push", False)):
            for job in REQUIRED_JOBS:
                with self.subTest(event=event, job=job):
                    errors = failures(self.needs(job, "skipped"), event, security)
                    self.assertEqual(bool(errors), job != "crate-release-age")
            for result in ("failure", "cancelled"):
                self.assertTrue(
                    failures(self.needs("crate-release-age", result), event, security)
                )

    def test_missing_dependency_blocks(self):
        needs = self.needs()
        del needs["package"]
        self.assertTrue(failures(needs, "pull_request", False))


if __name__ == "__main__":
    unittest.main()
