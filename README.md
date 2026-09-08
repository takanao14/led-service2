# led-service2

An LED panel display service for Raspberry Pi. Receives images via gRPC and renders them on an RGB LED matrix panel. On macOS, a minifb-based emulator is available for development.

## Features

- Display PNG / JPEG / GIF / PPM (PNM) images
- Animated GIF playback
- Horizontal scroll display (for wide images)
- Eye-catch GIF display on request received
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

For a new installation or an update of an existing release-based service:

```sh
# Replace v0.1.0 with the published release you want to install.
make deploy-release VERSION=v0.1.0 RPI_HOST=rpi3
```

| Device state | Command |
|--------------|---------|
| No `led-server.service` registered | `make deploy-release VERSION=v0.1.0 RPI_HOST=rpi3` creates and starts the service |
| Service already uses the release layout | The same `deploy-release` command updates it; configuration is preserved |
| Existing service runs a binary from a source checkout | `make migrate-release VERSION=v0.1.0 RPI_HOST=rpi3` preserves its configuration and switches to the release layout |
| Return to a previously installed tagged release | `make rollback VERSION=v0.1.0 RPI_HOST=rpi3` |

Initial deployment creates `/etc/led-service2/environment` and a systemd unit
running `/opt/led-service2/current/led-server`. Versioned binaries are retained
under `/opt/led-service2/releases/`. The service is enabled at boot. An already
active version is checked without restarting it.

The default inventory is `ansible/inventories/homelab/hosts.yaml`. Use
`ANSIBLE_INVENTORY` for another inventory and `ANSIBLE_ARGS=--ask-become-pass`
when sudo requires a password. Direct playbook commands are documented in
[ansible/README.md](ansible/README.md).

See [RELEASE.md](RELEASE.md) for tag publication, runtime requirements,
initial installation, migration and failure recovery. Publishing a tag does
not deploy to the device.

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

**Note:** Because the `rpi-led-matrix` backend is used, building on Raspberry Pi requires a C++ compiler (`build-essential`) and the `rpi-rgb-led-matrix` C++ library.

```bash
cargo build --release --bin led-server --features rpi --no-default-features
# or
make build   # run on RPi
```

For development that specifically needs a remote source build, `make deploy`
still uses rsync and builds on the device. It does not install or activate a
GitHub Release:

```bash
make deploy
```

The legacy `sudo make install` target installs the source-built binary's service
and copies local assets. It is not part of release deployment; do not run it to
update an Ansible-managed service.

## Running

Tagged Raspberry Pi binaries and versioned deployment are described in
[RELEASE.md](RELEASE.md). Use `led-server --version` to inspect the package
version, or `led-server --check http://127.0.0.1:50051` to probe a running service
without displaying an image. Startup logs include the version and build revision.

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

### Raspberry Pi (deployed systemd service)

Run these commands **on the device** after deployment:

```bash
sudo systemctl status led-server
sudo journalctl -u led-server -n 50 --no-pager
/opt/led-service2/current/led-server --version
/opt/led-service2/current/led-server --check http://127.0.0.1:50051
```

The server exits with a non-zero status if gRPC startup or serving fails (for example, when the listen port is already in use), allowing systemd `Restart=on-failure` to restart it.

## Configuration (environment variables)

For a new Ansible installation, edit the environment file **on the device** and
restart the service to apply changes:

```sh
sudoedit /etc/led-service2/environment
sudo systemctl restart led-server
```

For example, set `PANEL_REFRESH_RATE="120"` in the file. This limits panel refresh
to 120 Hz; it does not guarantee the hardware can achieve that rate. Existing
environment files are preserved by `deploy`, so changes to role defaults do not
update an installed device. A migrated legacy service retains its original
configuration source; inspect `sudo systemctl cat led-server` before editing it.

The table below lists binary defaults from `src/config.rs`. Make and the Ansible
role can provide deployment overrides, such as brightness, GPIO slowdown and
JSON logging; see [role defaults](ansible/roles/led_service2_release/defaults/main.yaml).

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
| `EYECATCH_PATH` | unset | Path to GIF file shown on request received |
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

# Play animated GIF (30 seconds)
cargo run --bin led-client -- --file anim.gif --duration 30

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

The request queue capacity is 10. When full, `RESOURCE_EXHAUSTED` is returned. During shutdown or after the worker stops, requests return `UNAVAILABLE`.

`SendImage` acknowledges queue admission, not successful decoding or display. Invalid or oversized images are rejected by the worker and logged; subsequent requests continue. The gRPC receive limit remains 4 MiB per message.

Each request receives a deadline when dequeued: the smaller of `duration_seconds` and `WORKER_TIMEOUT`. This budget includes eye-catch loading/playback, decoding, resizing, and main image display; queue waiting is excluded. The eye-catch is loaded on the first request and cached after successful decoding, with the same image limits and a 4 MiB file limit. If the budget expires during the eye-catch, the main image is skipped. Jingle audio plays independently of the request deadline.

Cancellation and deadlines are checked at decoder reads, between GIF frames, during resizing, and between display refreshes. These are cooperative checks: a codec operation already using buffered data or a blocking backend call must return before cancellation takes effect. Image limits bound application-owned buffers; decoder allocation limits are best-effort, and these settings are not a total process RSS limit. Cached eye-catch frames, the current image, render buffers, and queued compressed payloads can coexist.

SIGINT, SIGTERM, closing the emulator window, or pressing Escape stops the worker and discards queued requests. Existing emulator windows continue processing events while idle. gRPC connections get up to two seconds for graceful shutdown. No emulator window is opened until the first frame is displayed.

## Cargo Features

| Feature | Description |
|---------|-------------|
| `emulator` (default) | Emulator backend using minifb |
| `rpi` | Hardware backend using rpi-led-panel (mutually exclusive with `emulator`) |

## Key Dependencies

| Crate | Purpose | Platform | License |
|-------|---------|----------|---------|
| [rpi-led-matrix](https://crates.io/crates/rpi-led-matrix) | RPi LED matrix panel control | RPi | GPL-3.0 |
| [tonic](https://crates.io/crates/tonic) | gRPC server and client | all | MIT |
| [prost](https://crates.io/crates/prost) | Protocol Buffers code generation | all | Apache-2.0 |
| [tokio](https://crates.io/crates/tokio) | Async runtime | all | MIT |
| [image](https://crates.io/crates/image) | Image decoding (PNG / JPEG / GIF / PPM) | all | Apache-2.0 OR MIT |
| [alsa](https://crates.io/crates/alsa) | WAV audio playback via direct ALSA access | Linux | MIT OR Apache-2.0 |
| [hound](https://crates.io/crates/hound) | WAV file decoding | Linux | Apache-2.0 |
| [rodio](https://crates.io/crates/rodio) | WAV audio playback | non-Linux | Apache-2.0 OR MIT |
| [minifb](https://crates.io/crates/minifb) | Window rendering for emulator | non-Linux | Apache-2.0 OR MIT |
| [clap](https://crates.io/crates/clap) | CLI argument parsing | all | Apache-2.0 OR MIT |

## License

This project is distributed under the [GNU General Public License v3.0](LICENSE).
