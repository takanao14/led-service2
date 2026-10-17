# Releases and deployment

Release binaries target Raspberry Pi on Debian 13 (trixie), ARM64, with glibc
2.41. No emulator binary is published. CI verifies software behavior but not
physical LED or audio hardware.

## Requirements

The macOS controller requires Ansible, an authenticated GitHub CLI (`gh`), and
SSH access to the device. The remote account needs sudo privileges; use
`ANSIBLE_ARGS=--ask-become-pass` when required. The device requires Python 3 and
the runtime libraries listed in `build-info.json`, including `libasound2t64`.

Commands below run from the repository root. They use the default inventory at
`ansible/inventories/homelab/hosts.yaml`; override it with `ANSIBLE_INVENTORY`.
See [ansible/README.md](ansible/README.md) for direct playbook commands.

## Pull requests and hardware validation

All changes to `main`, including owner and Bot changes, go through a pull request.
The main ruleset requires zero approving reviews and successful `Format`,
`Lint and test (emulator)`, `Lint and build (rpi)`, `package / build`, and
`CI gate` checks from GitHub Actions. Deletion and force pushes are blocked,
with no bypass actors. Branches must be up to date before merging.

`CI gate` requires every dependency to succeed. Only the crate release age job
may be skipped: on a PR labeled `security`, or on non-PR runs (including tag
releases). Failures and cancellations still block the gate. Adding or removing
the label reruns CI. Use `security` only for intentional security updates.
Reusable Release checks have the `checks /` prefix; they are not the PR check
names configured in the ruleset.

For changes affecting the hardware backend, display timing, audio, runtime
configuration, or deployment, record the manual device validation in the PR:
LED output (static, scrolling, animation and eye-catch as affected), jingle/audio,
and shutdown behavior. Record the tested commit, device/OS, configuration, and
result, or explicitly state what remains untested. CI uses a hosted ARM runner
and cannot verify panel wiring, USB interference, physical display, or audio.
Arrange device validation explicitly; source and documentation checks do not
authorize deploying or restarting the device. The maintainer decides whether
any outstanding hardware validation blocks merging or release.

## Publishing

1. Update the version in `Cargo.toml` and `Cargo.lock` in a PR.
2. Merge the PR after all required checks pass and review any manual hardware
   validation results. A merge does not publish or deploy a release.
3. Manually decide whether to release. Fetch `main`, select the merged commit,
   and confirm its version, CI results, and required device validation.
4. Create and push a matching annotated tag on that commit, such as `v0.2.0`.

A tag push validates the version, runs CI, builds and verifies the archives,
and creates a GitHub Release. A suffix such as `v0.2.0-rc.1` creates a
prerelease. Publication never connects to a device or overwrites an existing
release.

Each release contains a binary archive, a vendored source archive, and
`checksums.txt`. To build the source archive on Raspberry Pi, install Rust,
Git, `build-essential`, `pkg-config`, `libasound2-dev`, and
`protobuf-compiler`, then run:

```sh
cargo build --offline --locked --release --bin led-server \
  --no-default-features --features rpi
```

Set `LED_BUILD_REVISION` to the commit in `build-info.json` to preserve the
revision log. The archive includes tracked project files and vendored Cargo
dependencies, but not the operating-system toolchain or libraries.

## Installation and migration

Install a new service or update an existing release-based service with:

```sh
make deploy-release VERSION=v0.2.0 RPI_HOST=rpi3
```

A new installation creates `/etc/led-service2/environment`, installs and enables
`led-server.service`, and runs `/opt/led-service2/current/led-server`. Releases
are retained under `/opt/led-service2/releases/`. The service runs as root and
does not require a source checkout or permissive audio udev rule.

The initial configuration uses a 32x64 panel and listens on
`0.0.0.0:50051`. Eye-catch and jingle playback remain disabled until their files
and environment variables are added. Release deployment never uploads or deletes
media. Existing environment files are preserved during updates.

To migrate a registered source-based service, run:

```sh
make migrate-release VERSION=v0.2.0 RPI_HOST=rpi3
```

Migration requires one `ExecStart` executable without arguments. It backs up the
executable and effective unit configuration under
`/opt/led-service2/releases/legacy-<timestamp>/`, then overrides only
`ExecStart`. Existing configuration, working directory, assets, and audio rules
remain in use; retain the old checkout if the working directory refers to it.

## Updating and rolling back

```sh
make deploy-release VERSION=v0.2.1 RPI_HOST=rpi3
make rollback VERSION=v0.2.0 RPI_HOST=rpi3
```

Deployment verifies the SHA-256 checksum, archive layout, metadata, executable
version, and runtime linkage before atomically changing `current`. Applying the
active version verifies it without restarting. Rollback requires a previously
installed release. Releases are not pruned automatically.

The post-activation probe requires three consecutive gRPC responses and an
active systemd service. Override `PROBE_URL` if the service listens elsewhere.
The probe does not display an image and cannot verify LED or audio output.

## Failure recovery

If activation fails, Ansible restores and restarts the previous version, then
exits unsuccessfully even when recovery succeeds. Inspect the playbook output
and `journalctl -u led-server` before retrying.

A failed first installation removes the unit, current link, and any environment
file created by that run. Existing configuration, assets, and downloaded
releases remain. Migration failure removes its override and restores the
original service command.

A device-wide lock prevents concurrent deployments. If the controller stops or
loses SSH connectivity, inspect the service and `current` link before manually
removing `/opt/led-service2/.ansible-deploy-lock`. Automated recovery cannot
repair an unreachable host. `ANSIBLE_ARGS=--check` validates inputs and OS facts
without downloading or changing the service.

Do not use the legacy `make install` target after migration; it can overwrite
the preserved service configuration.
