fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=LED_BUILD_REVISION");
    // Track tracked source edits and Git refs, including worktree metadata.
    for path in [
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "led-image-api",
        ".git",
    ] {
        if std::path::Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    for reference in ["HEAD", "index", "packed-refs", "refs"] {
        if let Some(path) = git_output(&["rev-parse", "--git-path", reference]) {
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    let revision = std::env::var("LED_BUILD_REVISION")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| git_output(&["describe", "--always", "--dirty", "--abbrev=12"]))
        .unwrap_or_else(|| "unknown".to_owned());
    if !revision
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        return Err("LED_BUILD_REVISION contains invalid characters".into());
    }
    println!("cargo:rustc-env=LED_BUILD_REVISION={revision}");
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &["led-image-api/api/proto/image/v1/image_service.proto"],
            &["led-image-api/api/proto"],
        )?;
    Ok(())
}

fn git_output(args: &[&str]) -> Option<String> {
    // A source archive nested inside another checkout must not use its Git data.
    if !std::path::Path::new(".git").exists() {
        return None;
    }
    let output = std::process::Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
