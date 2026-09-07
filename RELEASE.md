# Releases and deployment

Release binaries target Raspberry Pi running Debian 13 (trixie), ARM64, with
glibc 2.41. macOS development continues to use `make run`; no emulator binary
is published. Build jobs run in a Debian trixie container on an ARM64 runner.
Install the runtime ALSA library (`libasound2t64`) on the device. Other runtime
libraries are recorded in `build-info.json`; executing `--version` checks that
the dynamic loader can resolve them before
activation. CI does not verify LED or audio hardware behavior.

## Build artifacts

PR and main builds upload temporary ARM64 artifacts for verification.
The build workflow also supports manual execution.

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
