//! alsa-lib's error lines, formatted in C (src/alsa_errors.c: its handler
//! is variadic) - Linux only.

fn main() {
    println!("cargo::rerun-if-changed=src/alsa_errors.c");
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        cc::Build::new()
            .file("src/alsa_errors.c")
            .warnings(true)
            .extra_warnings(true)
            .compile("amp_alsa_errors");
    }
}
