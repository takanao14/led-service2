"""Validate all CI job results, allowing only the release-age policy skip."""

import json
import os
import sys

REQUIRED_JOBS = {"format", "emulator", "rpi", "package", "crate-release-age"}


def failures(needs, event_name, security):
    errors = []
    if set(needs) != REQUIRED_JOBS:
        errors.append("Unexpected dependency set")
    for job, outcome in needs.items():
        result = outcome.get("result")
        if result == "success":
            continue
        if (
            job == "crate-release-age"
            and result == "skipped"
            and (event_name != "pull_request" or security)
        ):
            continue
        errors.append(f"{job}: {result}")
    return errors


if __name__ == "__main__":
    errors = failures(
        json.loads(os.environ["CI_NEEDS"]),
        os.environ["CI_EVENT_NAME"],
        os.environ["CI_SECURITY"] == "true",
    )
    if errors:
        print("CI gate failed: " + "; ".join(errors), file=sys.stderr)
        sys.exit(1)
    print("All required CI jobs passed (or release age was intentionally skipped).")
