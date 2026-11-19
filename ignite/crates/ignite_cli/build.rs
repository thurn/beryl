//! Gives the `ignite` executable's main thread an 8 MiB stack on Windows, as macOS gives it.
//!
//! The MSVC linker reserves 1 MiB for the main thread unless told otherwise, and a debug build of
//! `ignite` overflowed that before it printed `--help` (`STATUS_STACK_OVERFLOW`, exit code
//! 0xC00000FD), which failed every test that runs the executable.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
