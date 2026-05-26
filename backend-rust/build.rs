use std::path::Path;

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let version_path = manifest_dir.join("../../VERSION");
    let version = std::fs::read_to_string(&version_path)
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let version = version.trim();
    println!("cargo:rustc-env=APP_VERSION={version}");
    println!("cargo:rerun-if-changed={}", version_path.display());
}
