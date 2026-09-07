# Releases and deployment

Release binaries target Raspberry Pi running Debian 13 (trixie), ARM64, with
glibc 2.41. macOS development continues to use `make run`; no emulator binary
is published. Build jobs run in a Debian trixie container on an ARM64 runner.
Install the runtime ALSA library (`libasound2t64`) on the device. Other runtime
libraries are recorded in `build-info.json`; executing `--version` checks that
the dynamic loader can resolve them before
activation. CI does not verify LED or audio hardware behavior.

## Publishing

1. Update the package version in `Cargo.toml` and `Cargo.lock` together.
2. Merge the reviewed change after CI passes.
3. Create and push an annotated matching tag, for example `v0.1.0`.

Tag pushes validate the version, run CI, build the archive, verify its contents
and checksums, and create a GitHub Release. Tags containing a prerelease suffix
such as `v0.2.0-rc.1` publish a prerelease. Published releases are not overwritten.
Release publication never connects to a device. PR and main builds upload
temporary artifacts for verification; only tagged releases are deployment inputs.

Each release contains a binary archive, a source archive (including the API
submodule and vendored Cargo dependencies with their license files), and
`checksums.txt`. The source archive can be built with `cargo build --offline
--locked --release --bin led-server --no-default-features --features rpi` after
installing Rust, build-essential, pkg-config, libasound2-dev and protobuf-compiler. Set
`LED_BUILD_REVISION` to the commit recorded in `build-info.json` to preserve the
revision log. Vendoring does not bundle the operating system toolchain or system
libraries. Archive metadata is normalized; bit-identical binary builds are not
guaranteed. Source archive generation uses tracked files, so commit new source
files before packaging locally.

## Initial migration

The existing `led-server.service` must already be installed and working. Keep
its assets and configuration in place. This is not a fresh-device provisioning
command. The playbook requires a single ExecStart executable without arguments.

On the development machine, install Ansible and GitHub CLI (`gh`), authenticate
with access to the release repository, and configure SSH access to `rpi3`.
The remote account needs sudo privileges. Pass `ANSIBLE_ARGS=--ask-become-pass`
if sudo requires a password. Debian Python 3 must be installed for Ansible's
built-in modules; this repository has no custom Python deployment scripts.
Downloads occur locally, so GitHub credentials are not copied to the device.
The Ansible layout and direct commands are documented in [ansible/README.md](ansible/README.md).
Inventory defaults to `ansible/inventories/homelab/hosts.yaml`, containing the SSH alias `rpi3`.
For other devices, supply an inventory with a `led_devices` group using
`ANSIBLE_INVENTORY`, and select the host with `RPI_HOST`.

```sh
make migrate-release VERSION=v0.1.0 RPI_HOST=rpi3
```

This backs up the current executable and effective service configuration under
`/opt/led-service2/releases/legacy-<timestamp>/`, then creates a systemd drop-in
at `/etc/systemd/system/led-server.service.d/90-release.conf` overriding only
ExecStart to `/opt/led-service2/current/led-server`. Existing environment,
WorkingDirectory, audio rules and assets remain in use; keep the old checkout
if WorkingDirectory points there. Migration restarts the service. A failed
activation restores the previous binary; failed migration removes its drop-in
and restores the original service command. Backups and release directories are
retained for inspection.

To undo a successful migration, remove only the above drop-in, run
`sudo systemctl daemon-reload`, then `sudo systemctl restart led-server`.
This relies on the original executable remaining available at its original path.
After confirming recovery, remove the `current` symlink if migrating again.

## Updating and rolling back

```sh
make deploy-release VERSION=v0.1.1 RPI_HOST=rpi3
make rollback VERSION=v0.1.0 RPI_HOST=rpi3
```

Releases are stored at `/opt/led-service2/releases/<tag>/`. A device-wide lock
prevents concurrent updates. Ansible verifies SHA-256, archive entries,
metadata and `--version` before atomically switching `current` and restarting
systemd. Existing version directories cannot be replaced with different bytes.
Rollback requires a previously installed tagged release. No automatic pruning
occurs. Checksums detect corruption; trust comes from the selected GitHub
repository and authenticated HTTPS download.

Applying an already active version verifies it without restarting the service.
Ansible releases its lock in an `always` block. If the controller is killed or
SSH becomes unreachable, inspect the service and current link before manually
removing `/opt/led-service2/.ansible-deploy-lock` and rerunning. Automated rescue
cannot recover an unreachable host. `ANSIBLE_ARGS=--check` validates inputs and
OS facts only; it deliberately ends before downloads and service mutations.

The post-restart check requires systemd to be active and three consecutive gRPC
responses. `led-server --check http://127.0.0.1:50051` sends a request without an
image and checks its expected validation error; it never queues a display request.
Override `PROBE_URL` when the service listens elsewhere. This confirms service
responsiveness, not successful physical LED/audio output. Check that separately
for the first release and hardware-related changes.

If the new service fails, the playbook switches back and restarts the previous
version. It exits unsuccessfully even when recovery succeeds; inspect its output
and `journalctl -u led-server` before retrying. `assets/` is never synchronized or
deleted by release deployment. The old `make deploy` remains available for source
sync and device builds; do not use `make install` after migration to overwrite
the existing service configuration unintentionally.
