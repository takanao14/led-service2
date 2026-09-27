#!/usr/bin/env python3
"""Check the release age of crates newly introduced in Cargo.lock."""

import argparse
import json
import re
import sys
import tomllib
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone
from pathlib import Path


CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
USER_AGENT = "led-service2-release-age-check/1.0 (https://github.com/takanao14/led-service2)"


def packages(path):
    with Path(path).open("rb") as lockfile:
        data = tomllib.load(lockfile)
        return {
            (package["name"], package["version"], package.get("source"))
            for package in data.get("package", [])
        }


def minimum_age(path):
    setting = json.loads(Path(path).read_text())["minimumReleaseAge"]
    match = re.fullmatch(r"(\d+) days?", setting)
    if not match:
        raise ValueError(f"Unsupported minimumReleaseAge: {setting!r}")
    return timedelta(days=int(match.group(1)))


def published_at(name, version):
    url = "https://crates.io/api/v1/crates/{}/{}".format(
        urllib.parse.quote(name, safe=""), urllib.parse.quote(version, safe="")
    )
    request = urllib.request.Request(
        url, headers={"Accept": "application/json", "User-Agent": USER_AGENT}
    )
    with urllib.request.urlopen(request, timeout=15) as response:
        data = json.load(response)
    timestamp = data["version"]["created_at"]
    published = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
    if published.tzinfo is None:
        raise ValueError(f"Missing timezone for {name} {version}: {timestamp}")
    return published.astimezone(timezone.utc)


def check(base_path, current_path, config_path, now=None, lookup=published_at):
    now = now or datetime.now(timezone.utc)
    age = minimum_age(config_path)
    cutoff = now - age
    introduced = sorted(
        packages(current_path) - packages(base_path),
        key=lambda package: (package[0], package[1], package[2] or ""),
    )
    failures = []
    checked = 0

    for name, version, source in introduced:
        if source is None:
            continue  # The workspace package has no registry release date.
        if source != CRATES_IO_SOURCE:
            failures.append(f"{name} {version}: unsupported source {source}")
            continue
        checked += 1
        try:
            published = lookup(name, version)
        except (KeyError, ValueError, OSError, urllib.error.URLError) as error:
            failures.append(f"{name} {version}: cannot verify publication date: {error}")
            continue
        if published > cutoff:
            eligible = published + age
            failures.append(
                f"{name} {version}: published {published.isoformat()}, "
                f"eligible after {eligible.isoformat()}"
            )

    print(
        f"Checked {checked} newly introduced crates.io versions "
        f"(cutoff: {cutoff.isoformat()})."
    )
    for failure in failures:
        print(f"::error::{failure}")
    return not failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, help="Cargo.lock from the PR base")
    parser.add_argument("--current", default="Cargo.lock")
    parser.add_argument("--config", default="renovate.json")
    args = parser.parse_args()
    try:
        passed = check(args.base, args.current, args.config)
    except (KeyError, ValueError, OSError, tomllib.TOMLDecodeError) as error:
        print(f"::error::Release-age check failed: {error}")
        return 1
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
