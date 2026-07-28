fn main() {
    // The Inflect-Micro-v2 engine statically links a prebuilt ONNX Runtime (via
    // `ort` + `download-binaries`) built against glibc >= 2.38, so it references
    // __isoc23_strtol/strtoll/... . Linking that archive on an older-glibc host
    // (the ubuntu-22.04 CI runners ship glibc 2.35) fails with "undefined
    // symbol". Compile a small shim providing those symbols by forwarding to the
    // classic libc functions, so the result links and runs on older glibc.
    //
    // voxctrl-inference does the same for its Moonshine backend. Both are needed:
    // this crate's own test binary links ONNX Runtime without pulling in
    // voxctrl-inference, so that crate's shim is not in scope here. The
    // definitions in ours are weak, so a binary containing both is fine.
    let inflect = std::env::var_os("CARGO_FEATURE_INFLECT_MICRO").is_some();
    let linux = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");

    println!("cargo:rerun-if-changed=src/isoc23_compat.c");
    println!("cargo:rerun-if-changed=build.rs");

    if inflect && linux {
        cc::Build::new()
            .file("src/isoc23_compat.c")
            .compile("voxctrl_tts_isoc23_compat");
    }
}
