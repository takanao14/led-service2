# AGENTS.md

## Project Constraints
- Communicate in Japanese. Use English for code, identifiers, and comments. Keep filenames in ASCII.
- This Rust gRPC service drives a Raspberry Pi RGB LED matrix; macOS development uses a minifb emulator.
- Keep emulator window operations on the main thread for macOS. Preserve non-blocking gRPC handlers, bounded queue backpressure, and sequential display processing.
- Keep shared image processing and display loops in `src/display/mod.rs`; hardware-specific behavior belongs in backends behind `LedDisplay`.
- Centralize runtime configuration in `src/config.rs`; use environment variables and Make variables for deployment overrides.
- Bound image dimensions, animation frame counts, and decoded memory for Raspberry Pi resource limits. Include decoding and eye-catch playback when considering request deadlines.
- Preserve documented hardware workarounds, especially disabled hardware pulsing to prevent USB interference on Raspberry Pi 3.
- Protobuf definitions live in the `led-image-api/` submodule. Preserve field numbers, regenerate bindings rather than editing generated files, and update the parent repository's submodule reference deliberately.
- `assets/` contains local image/audio files, is ignored by Git, and is absent from fresh clones.

## Build and Validation
- Initialize API sources when needed: `git submodule update --init --recursive`.
- Protobuf generation requires `protoc`. Raspberry Pi builds also require a C++ toolchain and ALSA development headers (`build-essential`, `libasound2-dev`, and `protobuf-compiler` on Debian-based systems).
- macOS emulator: `cargo build --bin led-server` or `make build`.
- Raspberry Pi: `cargo build --release --bin led-server --features rpi --no-default-features` or `make build` on the device.
- For Rust changes, run `cargo fmt --all -- --check`. Run `cargo clippy --all-targets` and `cargo test` with `--no-default-features --features emulator` for the emulator or `--no-default-features --features rpi` on Raspberry Pi.
- Use a fake `LedDisplay` for display regression tests where practical. Emulator validation does not verify the hardware backend.
- Documentation-only changes require path, command, and consistency checks; Rust builds are not required.

## Deployment and Operations
- `make deploy` runs `deploy.sh`, which uses rsync with `--delete` and builds remotely. Check the destination and deletion scope, and exclude local secrets from syncs.
- `sudo make install` copies assets, writes a systemd unit and an audio udev rule, and enables/starts the service. Build the binary and prepare required assets first.
- Do not deploy, install/restart services, or execute on hardware merely to validate source or documentation changes.
