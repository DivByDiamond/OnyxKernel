use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=linker.ld");

    // `linker.ld` is a bare-metal RISC-V layout (ENTRY(_start), base 0x10000,
    // .note/.eh_frame discarded). Passing it to an x86_64 build produces a
    // binary the host loader cannot start — historically this made
    // `cargo test -p onyx_init` die with SIGSEGV inside ld-linux before
    // main(). Only apply it to the real target.
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("riscv64") {
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let linker_ld = manifest.join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", linker_ld.display());
}
