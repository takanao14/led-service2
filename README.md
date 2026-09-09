# led-service2

An LED panel display service for Raspberry Pi. Receives images via gRPC and renders them on an RGB LED matrix panel. On macOS, a minifb-based emulator is available for development.

## Features

- Display PNG / JPEG / GIF / PPM (PNM) images
- Animated GIF playback
- Horizontal scroll display (for wide images)
- Eye-catch GIF display when request processing starts
- Simultaneous jingle (WAV) playback
- macOS emulator with LED dot-style rendering

## Architecture

```
Client (led-client)
    │  gRPC (image.v1.ImageService/SendImage)
    ▼
led-server
    ├── gRPC thread (tonic / tokio)
    │       └── enqueue received requests to channel
    └── main thread (display loop)
            ├── eye-catch GIF display
            ├── jingle playback (separate thread)
            └── image display (static / scroll / animation)
```

## Requirements

| Environment | Requirements |
|-------------|-------------|
| macOS (local development) | Rust toolchain, `protoc` |
| macOS (deployment controller) | Ansible, GitHub CLI (`gh`) authenticated with access to the release repository, SSH access to the device |
| Raspberry Pi (release deployment) | Debian 13 (trixie), ARM64, Python 3 for Ansible, sudo access, runtime libraries including `libasound2t64` |
| Raspberry Pi (source build only) | Rust toolchain, `build-essential`, `pkg-config`, `libasound2-dev`, `protobuf-compiler` |

## Deploying to Raspberry Pi

Run deployment commands **on the macOS controller, from the repository root**.
Ansible downloads and verifies a published GitHub Release locally, then transfers
the binary to `rpi3` over SSH. The device does not need Rust, a compiler, or a
source checkout for release deployment.

Install or update a release-based service with:

```sh
# Replace v0.1.0 with the published release you want to install.
make deploy-release VERSION=v0.2.0 RPI_HOST=rpi3
```

Use `make migrate-release VERSION=...` for an existing source-based service and
`make rollback VERSION=...` to activate a previously installed release.
Configuration is preserved during updates and migrations.

The default inventory is `ansible/inventories/homelab/hosts.yaml`. Use
`ANSIBLE_INVENTORY` for another inventory and `ANSIBLE_ARGS=--ask-become-pass`
when sudo requires a password. Direct playbook commands are documented in
[ansible/README.md](ansible/README.md).

See [RELEASE.md](RELEASE.md) for installation layout, publication, migration,
rollback and failure recovery. Publishing a tag does not deploy to the device.

## Build

Initialize the Protobuf submodule before the first build:

```bash
git submodule update --init --recursive
```

### macOS (emulator)

```bash
cargo build --bin led-server
# or
make build
```

Make uses `cargo` from `PATH`. To select a specific installation, pass
`CARGO=/path/to/cargo` to `make build` or `make run`.

### Raspberry Pi (source builds)

**Note:** The `rpi-led-matrix` backend compiles its bundled
`rpi-rgb-led-matrix` C++ library, so building on Raspberry Pi requires a C++
compiler (`build-essential`).

```bash
cargo build --release --bin led-server --features rpi --no-default-features
# or
make build   # run on RPi
```

For a development-only remote source build, run:

```bash
make deploy
```

This uses rsync and builds on the device without installing a GitHub Release.
Do not use the legacy `sudo make install` target on an Ansible-managed service.

## Running

### macOS (emulator)

```bash
cargo run --bin led-server
# or
make run
```

### Raspberry Pi (direct source-built binary)

```bash
sudo make run
```

For a deployed service, check it on the device with:

```bash
sudo systemctl status led-server
sudo journalctl -u led-server -n 50 --no-pager
/opt/led-service2/current/led-server --version
/opt/led-service2/current/led-server --check http://127.0.0.1:50051
```

The probe does not display an image. Startup logs include the package version and
build revision. Server failures return a non-zero exit status for systemd restart.

## Configuration (environment variables)

For a new Ansible installation, edit the environment file **on the device** and
restart the service to apply changes:

```sh
sudoedit /etc/led-service2/environment
sudo systemctl restart led-server
```

The table below lists binary defaults from `src/config.rs`. Make and the Ansible
role can provide deployment overrides, such as brightness, GPIO slowdown and
JSON logging; see [role defaults](ansible/roles/led_service2_release/defaults/main.yaml).
Existing environment files are preserved during release updates. A migrated
service may use another configuration source; inspect
`sudo systemctl cat led-server` before editing it.

| Variable | Default | Description |
|----------|---------|-------------|
| `GRPC_ADDR` | `0.0.0.0:50051` | gRPC listen address |
| `PANEL_ROWS` | `32` | Number of LED panel rows |
| `PANEL_COLS` | `64` | Number of LED panel columns |
| `PANEL_BRIGHTNESS` | `50` | Brightness (0–100, RPi only) |
| `PANEL_REFRESH_RATE` | `120` | Refresh-rate limit in Hz; `0` means no limit (RPi only) |
| `PANEL_SLOWDOWN` | unset | GPIO slowdown factor (RPi only) |
| `PANEL_PWM_BITS` | `11` | PWM bit depth (RPi only) |
| `PANEL_PWM_LSB_NANOSECONDS` | `130` | PWM least-significant-bit pulse duration in nanoseconds (RPi only) |
| `WORKER_TIMEOUT` | `30s` | Maximum total processing time per dequeued request, including eye-catch and decoding (e.g. `60s`) |
| `SCROLL_INTERVAL_MS` | `30` | Scroll speed in milliseconds per pixel |
| `EYECATCH_PATH` | unset | Path to GIF file shown when request processing starts |
| `EYECATCH_DURATION_MS` | `3000` | Eye-catch display duration in milliseconds |
| `MAX_IMAGE_DIMENSION` | `4096` | Maximum input width/height and panel dimensions; must be positive |
| `MAX_ANIMATION_FRAMES` | `256` | Maximum GIF frames; must be positive |
| `MAX_DECODED_BYTES` | `67108864` | Decoded buffer budget per image/GIF (64 MiB); GIF splits this between decoder scratch and retained frames |
| `MAX_RENDER_BYTES` | `16777216` | Limit for render buffer allocations (16 MiB), including checks for scrolling and cached animation frames |
| `JINGLE_PATH` | unset | Path to WAV file played on request received |
| `RUST_LOG` | `info` | Log level (`debug` / `info` / `warn` / `error`) |
| `LOG_FORMAT` | text | Set to `json` for structured JSON logging |

### Optional eye-catch and jingle

Release deployment does not upload media files, and new installations leave
these features disabled. Place the GIF and WAV files on the device, then add
their paths to `/etc/led-service2/environment`, keeping the other settings:

```ini
EYECATCH_PATH="/opt/led-service2/assets/butacowalk2.gif"
EYECATCH_DURATION_MS="5000"
JINGLE_PATH="/opt/led-service2/assets/splanews.wav"
```

Restart `led-server` after editing. Either feature can be enabled independently.
To disable one, remove or comment out its path variable instead of setting it to
an empty string. Local `assets/` files are ignored by Git and absent from fresh
clones; prepare them separately.

## Sending Images

Use the `led-client` command to send images.

```bash
# Display PNG for 10 seconds
cargo run --bin led-client -- --file image.png --duration 10

# Horizontal scroll PPM
cargo run --bin led-client -- --file banner.ppm --duration 20

# Explicitly specify display mode
cargo run --bin led-client -- --file image.png --duration 10 --display-mode scroll

# Send to a different server
cargo run --bin led-client -- --addr http://raspberrypi.local:50051 --file image.png --duration 10
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `--addr` | `http://localhost:50051` | Server address |
| `--file` | required | Path to image file to send |
| `--duration` | `10` | Display duration in seconds |
| `--mime` | auto-detected from extension | MIME type |
| `--display-mode` | inferred from file type | `static` or `scroll` |

### Display Mode Inference

When `--display-mode` is omitted, the mode is inferred from the file type:

| File type | Default behavior |
|-----------|-----------------|
| PPM / PNM | Horizontal scroll |
| PNG / JPEG | Static display |
| GIF | Animated playback (mode flag is ignored) |

## gRPC API

See the Protobuf definition at [`led-image-api/api/proto/image/v1/image_service.proto`](led-image-api/api/proto/image/v1/image_service.proto).

```protobuf
service ImageService {
    rpc SendImage(SendImageRequest) returns (SendImageResponse);
}
```

`SendImage` acknowledges admission to the 10-request queue, not successful decode
or display. A full queue returns `RESOURCE_EXHAUSTED`; shutdown or a stopped worker
returns `UNAVAILABLE`. Invalid images are rejected later by the worker and logged.
The gRPC receive limit is 4 MiB.

Processing is sequential. Its deadline starts at dequeue and is the smaller of
`duration_seconds` and `WORKER_TIMEOUT`; queue waiting is excluded. Shutdown
discards queued requests. See [`src/shutdown.rs`](src/shutdown.rs),
[`src/worker.rs`](src/worker.rs) and [`src/decode.rs`](src/decode.rs) for
cancellation and resource-limit details.

## Cargo Features

| Feature | Description |
|---------|-------------|
| `emulator` (default) | Emulator backend using minifb |
| `rpi` | Hardware backend using `rpi-led-matrix`; select it with `--no-default-features --features rpi` |

## Validation

Run the emulator checks on macOS after initializing the Protobuf submodule:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --no-default-features --features emulator -- -D warnings
cargo test --locked --no-default-features --features emulator
```

On Raspberry Pi, replace `emulator` with `rpi`. Emulator validation does not
exercise the hardware backend or verify physical LED and audio output.

## License

This project is distributed under the [GNU General Public License v3.0](LICENSE).
