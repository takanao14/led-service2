fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["led-image-api/api/proto/image/v1/image_service.proto"],
            &["led-image-api/api/proto"],
        )?;
    Ok(())
}
