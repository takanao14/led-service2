/// Protocol Buffers generated code for `image.v1`.
///
/// Shared by both the `led-server` and `led-client` binaries so that the
/// generated types are defined in exactly one place.
// `tonic::Status` is larger than the `result_large_err` threshold, and the
// service traits returning it are generated, so the lint is allowed here
// rather than worked around in generated code.
#[allow(clippy::result_large_err)]
pub mod proto {
    tonic::include_proto!("image.v1");
}
