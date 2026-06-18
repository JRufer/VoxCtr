use std::path::PathBuf;

fn main() {
    // The `voxctrl-overlay` helper is shipped as a Tauri sidecar (see
    // `externalBin` in tauri.conf.json) so it lands next to the main binary in
    // packaged builds. Tauri validates that the sidecar file exists on *every*
    // compile of this crate, including a bare `cargo build`/`cargo check` or
    // rust-analyzer — none of which run `beforeBuildCommand`. Guarantee a
    // placeholder exists here, before tauri_build::build() runs its validation,
    // so ordinary development builds never fail. Packaging overwrites this with
    // the real binary via scripts/prepare-sidecar.mjs before Tauri bundles.
    ensure_overlay_sidecar_placeholder();

    tauri_build::build()
}

fn ensure_overlay_sidecar_placeholder() {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };
    // Tauri names sidecars with the target triple it is compiling for.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.is_empty() {
        return;
    }
    let exe_suffix = if target.contains("windows") { ".exe" } else { "" };

    let dir = manifest_dir.join("binaries");
    let sidecar = dir.join(format!("voxctrl-overlay-{target}{exe_suffix}"));
    if sidecar.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("cargo:warning=could not create {}: {e}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(&sidecar, []) {
        println!("cargo:warning=could not write placeholder sidecar {}: {e}", sidecar.display());
    }
}
