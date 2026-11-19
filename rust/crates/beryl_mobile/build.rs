fn main() {
    // Cargo reruns a build script that prints no `rerun-if-changed` whenever any file in the
    // package changes. This script writes generated permission files into `permissions/`, so
    // that made every build stale. Watch only the hand-written inputs.
    for path in ["build.rs", "Cargo.toml", "android", "ios"] {
        println!("cargo:rerun-if-changed={path}");
    }
    // Only Rust calls these native commands.
    tauri_plugin::Builder::new(&[])
        .android_path("android")
        .ios_path("ios")
        .build();
}
