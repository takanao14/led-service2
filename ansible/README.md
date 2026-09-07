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
│   └── tasks/
│       ├── main.yaml
│       ├── stage.yaml
│       ├── verify_archive.yaml
│       ├── migrate.yaml
│       ├── activate.yaml
│       └── probe.yaml
└── tests/archive.yaml
```

The inventory owns device membership. The operational playbook selects hosts,
privilege escalation and serial execution. The single role owns release
defaults and deployment tasks. Add host/group variables or templates only when
needed; no external collections are required.

## Running

Use the existing Make targets from the repository root:

```sh
make migrate-release VERSION=v0.1.0 RPI_HOST=rpi3
make deploy-release VERSION=v0.1.1 RPI_HOST=rpi3
make rollback VERSION=v0.1.0 RPI_HOST=rpi3
```

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
```

The archive test invokes only the role's verification tasks on local temporary
files. It does not connect to the device or restart services. Its two expected
failures (bad checksum and a symlink in the archive) are rescued; the final recap
must report `failed=0`.
