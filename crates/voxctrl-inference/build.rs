fn main() {
    // The Moonshine backend statically links a prebuilt ONNX Runtime (via
    // `ort` + `download-binaries`) that is built against glibc >= 2.38 and so
    // references __isoc23_strtol/strtoll/... symbols. Linking that archive on an
    // older-glibc host (our ubuntu-22.04 release runners ship glibc 2.35) fails
    // with "undefined symbol". Compile and link a tiny shim that provides those
    // symbols by forwarding to the classic libc functions, keeping the binary
    // linkable and runnable on older glibc. Linux + `moonshine` feature only.
    let moonshine = std::env::var_os("CARGO_FEATURE_MOONSHINE").is_some();
    let linux = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");

    println!("cargo:rerun-if-changed=src/isoc23_compat.c");
    println!("cargo:rerun-if-changed=build.rs");

    if moonshine && linux {
        cc::Build::new()
            .file("src/isoc23_compat.c")
            .compile("voxctrl_isoc23_compat");
    }
}
