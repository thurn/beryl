//! Compiles `tests/c/header.c`, the C program that tests `include/beryl3d.h`, and links it into
//! the test targets only, so that neither the phone libraries nor the desktop binary contain
//! it. A phone build compiles nothing: its tests do not run.

fn main() {
    println!("cargo:rerun-if-changed=tests/c");
    println!("cargo:rerun-if-changed=include/beryl3d.h");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "ios" || target == "android" {
        return;
    }
    let objects = cc::Build::new()
        .file("tests/c/header.c")
        .include("include")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile_intermediates();
    for object in objects {
        println!("cargo:rustc-link-arg-tests={}", object.display());
    }
}
