//! Gives the binaries macOS's 8 MiB main-thread stack on Windows, whose linker defaults to
//! 1 MiB: a search that recurses deep enough to pass on macOS would overflow there. In a build
//! script, not `.cargo/config.toml`, since the recipes' `RUSTFLAGS` would override that.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
