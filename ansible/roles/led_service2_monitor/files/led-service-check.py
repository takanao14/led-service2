#!/usr/bin/env python3
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
import time


def succeeds(command, timeout):
    try:
        return int(subprocess.run(command, timeout=timeout, check=False,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0)
    except (OSError, subprocess.TimeoutExpired):
        return 0


def collect(binary, url, service, timeout):
    active = succeeds(["systemctl", "is-active", "--quiet", service], timeout)
    grpc = succeeds([binary, "--check", url], timeout)
    return (
        "# HELP led_service_active Whether the systemd service is active.\n"
        "# TYPE led_service_active gauge\n"
        f"led_service_active {active}\n"
        "# HELP led_service_grpc_success Whether the non-displaying gRPC probe succeeded.\n"
        "# TYPE led_service_grpc_success gauge\n"
        f"led_service_grpc_success {grpc}\n"
        "# HELP led_service_check_timestamp_seconds Unix time of the last completed check, including failures.\n"
        "# TYPE led_service_check_timestamp_seconds gauge\n"
        f"led_service_check_timestamp_seconds {time.time():.3f}\n"
    )


def publish(output, metrics):
    # Readers must see either the old complete snapshot or the new one.
    output = Path(output)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=output.parent,
                                         prefix=".led-service-", suffix=".tmp", delete=False) as stream:
            temporary = stream.name
            stream.write(metrics)
            stream.flush()
            os.fchmod(stream.fileno(), 0o644)
        os.replace(temporary, output)
    finally:
        if temporary is not None:
            Path(temporary).unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--url", required=True)
    parser.add_argument("--service", default="led-server.service")
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    publish(args.output, collect(args.binary, args.url, args.service, timeout=5))


if __name__ == "__main__":
    main()
