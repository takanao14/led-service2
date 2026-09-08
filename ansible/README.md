# LED service deployment

This directory manages versioned releases of the LED service. Build and release
publication remain in Cargo and GitHub Actions. See [RELEASE.md](../RELEASE.md)
for prerequisites, initial migration, update behavior and recovery limitations.

```text
ansible/
├── ansible.cfg
├── inventories/homelab/hosts.yaml
├── playbooks/ops-led_service2_release.yaml
├── roles/led_service2_release/
│   ├── defaults/main.yaml
│   ├── templates/               # Initial unit and environment configuration
│   └── tasks/
│       ├── main.yaml
│       ├── validate_state.yaml
│       ├── install.yaml
│       ├── stage.yaml
│       ├── verify_archive.yaml
│       ├── migrate.yaml
│       ├── activate.yaml
│       └── probe.yaml
└── tests/                      # Archive, state selection and installation tests
```

The inventory owns device membership. The operational playbook selects hosts,
privilege escalation and serial execution. The single role owns release
defaults, deployment tasks and initial configuration templates. Add host/group
variables when needed; no external collections are required.

## Running

Use the existing Make targets from the repository root:

```sh
make migrate-release VERSION=v0.1.0 RPI_HOST=rpi3
make deploy-release VERSION=v0.1.1 RPI_HOST=rpi3
make rollback VERSION=v0.1.0 RPI_HOST=rpi3
```

Use `deploy-release` for both a new installation and an existing release-based
service. Use `migrate-release` only when preserving an existing source-based
systemd service. Fresh installs create a unit and environment file; updates
preserve configuration. See [first installation](../RELEASE.md#first-installation)
for initial settings and failure cleanup.

Make explicitly selects this directory's `ansible.cfg`. To run Ansible directly,
change to this directory so the same config, inventory and role paths are used:

```sh
cd ansible
ansible-playbook playbooks/ops-led_service2_release.yaml --limit rpi3 \
  -e led_service2_release_version=v0.1.1 \
  -e led_service2_release_action=deploy
```

The role uses the `led_service2_release_` variable prefix. The version is required;
the action defaults to `deploy` and also accepts `migrate` and `rollback`.
Repository, installation paths and probe URL defaults are in
[`defaults/main.yaml`](roles/led_service2_release/defaults/main.yaml).
Override values with inventory variables or `-e`; the Make interface retains
`VERSION`, `RPI_HOST`, `RELEASE_REPO`, `PROBE_URL` and `ANSIBLE_ARGS`.

## Local validation

From this directory:

```sh
ansible-playbook playbooks/ops-led_service2_release.yaml --syntax-check
ansible-lint .
ansible-playbook -i localhost, tests/archive.yaml
ansible-playbook -i localhost, tests/state.yaml
```

The archive test invokes only the role's verification tasks on local temporary
files. It does not connect to the device or restart services. Its two expected
failures (bad checksum and a symlink in the archive) are rescued; the final recap
must report `failed=0`.

`tests/install.yaml` additionally tests first startup, failure cleanup, retry and
preservation of existing configuration. Run it as root in an isolated Linux
environment (or with `-e ansible_become=true` on the CI runner). It uses a fake
`systemctl` and temporary unit files, never the real service manager or hardware.

New installations grant the service the `audio` supplementary group so WAV playback remains available after the LED library drops its UID/GID to `daemon`. Existing units are preserved; add `SupplementaryGroups=audio` in a systemd service override and restart when enabling audio on an installation created before this setting.
