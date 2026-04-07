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
| macOS (development) | Rust toolchain |
| Raspberry Pi (production) | Rust toolchain, root privileges (LED panel control) |

## Build

### macOS (emulator)

```bash
cargo build --bin led-server
# or
make build
```

### Raspberry Pi

**Note:** Because the `rpi-led-matrix` backend is used, building on Raspberry Pi requires a C++ compiler (`build-essential`) and the `rpi-rgb-led-matrix` C++ library.

```bash
cargo build --release --bin led-server --features rpi --no-default-features
# or
make build   # run on RPi
```

To rsync from macOS and build remotely on RPi:

```bash
make deploy
```

## Running

### macOS (emulator)

```bash
cargo run --bin led-server
# or
make run
```

### Raspberry Pi (direct)

```bash
sudo make run
```

### Raspberry Pi (systemd service)

```bash
sudo make install    # install and enable autostart
sudo make status     # check status
sudo make restart    # restart
sudo make uninstall  # uninstall
```

## Configuration (environment variables)

| Variable | Default | Description |
|----------|---------|-------------|
| `GRPC_ADDR` | `0.0.0.0:50051` | gRPC listen address |
| `PANEL_ROWS` | `32` | Number of LED panel rows |
| `PANEL_COLS` | `64` | Number of LED panel columns |
| `PANEL_BRIGHTNESS` | `50` | Brightness (0–100, RPi only) |
| `PANEL_REFRESH_RATE` | `120` | Refresh rate in Hz (RPi only) |
| `PANEL_SLOWDOWN` | unset | GPIO slowdown factor (RPi only) |
| `WORKER_TIMEOUT` | `30s` | Maximum display time per request (e.g. `60s`) |
| `SCROLL_INTERVAL_MS` | `30` | Scroll speed in milliseconds per pixel |
| `EYECATCH_PATH` | unset | Path to GIF file shown on request received |
| `EYECATCH_DURATION_MS` | `5000` | Eye-catch display duration in milliseconds |
| `JINGLE_PATH` | unset | Path to WAV file played on request received |
| `RUST_LOG` | `info` | Log level (`debug` / `info` / `warn` / `error`) |
| `LOG_FORMAT` | text | Set to `json` for structured JSON logging |

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

The request queue capacity is 10. When full, `RESOURCE_EXHAUSTED` is returned.

## Cargo Features

| Feature | Description |
|---------|-------------|
| `emulator` (default) | Emulator backend using minifb |
| `rpi` | Hardware backend using rpi-led-panel (mutually exclusive with `emulator`) |

## Key Dependencies

| Crate | Purpose | License |
|-------|---------|---------|
| [rpi-led-matrix](https://crates.io/crates/rpi-led-matrix) | RPi LED matrix panel control | GPL-3.0 |
| [tonic](https://crates.io/crates/tonic) | gRPC server and client | MIT |
| [prost](https://crates.io/crates/prost) | Protocol Buffers code generation | Apache-2.0 |
| [tokio](https://crates.io/crates/tokio) | Async runtime | MIT |
| [image](https://crates.io/crates/image) | Image decoding (PNG / JPEG / GIF / PPM) | Apache-2.0 OR MIT |
| [rodio](https://crates.io/crates/rodio) | WAV audio playback | Apache-2.0 OR MIT |
| [minifb](https://crates.io/crates/minifb) | Window rendering for emulator | Apache-2.0 OR MIT |
| [clap](https://crates.io/crates/clap) | CLI argument parsing | Apache-2.0 OR MIT |

## License

This project is distributed under the [GNU General Public License v3.0](LICENSE).
