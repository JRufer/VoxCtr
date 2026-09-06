//! How this copy of VoxCtrl was installed, and which release asset replaces it.
//!
//! Self-updating only makes sense for the builds that are a single file the app
//! owns: the Linux AppImage, and the Windows installer that can be re-run over
//! the top of itself. A `.deb`, a distro package or a `cargo run` build belongs
//! to something else — the package manager, or the developer — and rewriting it
//! behind that owner's back is how a package database ends up lying about what
//! is on disk. Those cases are detected and reported, not updated.

use std::path::{Path, PathBuf};

use crate::release::ReleaseAsset;

/// The shape of this installation, as far as updating is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// A Linux AppImage at this path, which can be replaced in place.
    AppImage {
        path: PathBuf,
        /// True when the running file is the Vulkan (GPU) build, so the update
        /// keeps the flavour the user chose instead of quietly downgrading
        /// them to the CPU build.
        vulkan: bool,
    },
    /// A Windows install, updated by running the new installer.
    /// A Windows NSIS installation, replaced by re-running the installer.
    ///
    /// `webgpu` marks the GPU variant. Unlike the AppImage there is no filename
    /// to read it off, so it comes from the build's own features.
    WindowsInstaller { webgpu: bool },
    /// Installed by something that owns the files — a `.deb`, a distro package.
    ManagedPackage,
    /// A development build, or anything else we cannot safely replace.
    Unmanaged,
}

impl InstallKind {
    /// Whether VoxCtrl can replace itself, as opposed to pointing the user at
    /// the download page.
    pub fn can_self_update(&self) -> bool {
        matches!(self, Self::AppImage { .. } | Self::WindowsInstaller { .. })
    }

    /// Why self-updating is unavailable, phrased for the user. `None` when it
    /// is available.
    pub fn unsupported_reason(&self) -> Option<&'static str> {
        match self {
            Self::AppImage { .. } | Self::WindowsInstaller { .. } => None,
            Self::ManagedPackage => Some(
                "This copy of VoxCtrl was installed by your package manager, which owns \
                 the files. Update it the same way you installed it, or download the new \
                 release from GitHub.",
            ),
            Self::Unmanaged => Some(
                "VoxCtrl is running from a build directory rather than a packaged release, \
                 so there is nothing for it to replace. Download the new release from GitHub, \
                 or rebuild from source.",
            ),
        }
    }
}

/// Classify the running process.
///
/// An AppImage sets `APPIMAGE` to the absolute path of the image file — that is
/// the whole detection, and it is the runtime's own contract. Everything else
/// is decided from where the executable sits.
///
/// `gpu_build` says whether this binary is the GPU variant of its platform's
/// release artifacts. An AppImage can read that off its own filename; a Windows
/// installation cannot, because the installer writes the same paths either way.
/// So the caller — compiled from the same features as the binary it is asking
/// about — has to say. Getting it wrong means a GPU install quietly updating
/// itself onto the CPU build.
pub fn detect(gpu_build: bool) -> InstallKind {
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    if let Some(path) = appimage {
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let vulkan = is_vulkan_build(&path);
        return InstallKind::AppImage { path, vulkan };
    }

    let exe = std::env::current_exe().unwrap_or_default();
    classify_exe_path_with(&exe, gpu_build)
}

/// The part of [`detect`] that depends only on the executable path, split out so
/// it can be tested without a real installation.
///
/// Assumes a CPU build; [`classify_exe_path_with`] is the form that takes the
/// variant. Kept because most callers and tests do not care.
pub fn classify_exe_path(exe: &Path) -> InstallKind {
    classify_exe_path_with(exe, false)
}

/// [`classify_exe_path`], told whether this is the GPU build.
pub fn classify_exe_path_with(exe: &Path, gpu_build: bool) -> InstallKind {
    if cfg!(target_os = "windows") {
        // The NSIS bundle installs under Program Files or the user's local
        // app data; either way re-running the installer is the update path.
        // A `target\debug` build is a developer's, not ours to overwrite.
        return if is_build_dir(exe) {
            InstallKind::Unmanaged
        } else {
            InstallKind::WindowsInstaller { webgpu: gpu_build }
        };
    }

    let _ = gpu_build;
    if is_build_dir(exe) {
        return InstallKind::Unmanaged;
    }

    // Where a `.deb` (or any distro package) puts it.
    let s = exe.to_string_lossy();
    if s.starts_with("/usr/") || s.starts_with("/opt/") || s.starts_with("/nix/store/") {
        return InstallKind::ManagedPackage;
    }

    InstallKind::Unmanaged
}

fn is_build_dir(exe: &Path) -> bool {
    exe.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "debug" || s == "release"
    }) && exe.components().any(|c| c.as_os_str() == "target")
}

/// Whether a file name belongs to the Vulkan (GPU) AppImage build.
pub fn is_vulkan_build(path: &Path) -> bool {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase().contains("vulkan"))
        .unwrap_or(false)
}

/// Pick the release asset that replaces this installation.
///
/// The release workflow labels every artifact with its platform — see
/// `.github/workflows/release.yml`, which appends `-linux-x86_64`,
/// `-linux-x86_64-vulkan` or `-windows-x86_64` to each file's stem — so the
/// match is on that suffix rather than on the version-bearing prefix, which
/// changes every release.
pub fn select_asset<'a>(kind: &InstallKind, assets: &'a [ReleaseAsset]) -> Option<&'a ReleaseAsset> {
    match kind {
        InstallKind::AppImage { vulkan: true, .. } => {
            // Keep the GPU build if the release has one; a release that shipped
            // only the CPU variant is still an upgrade worth taking, and the
            // CPU build runs on the same machine.
            appimage_named(assets, "-linux-x86_64-vulkan.appimage")
                .or_else(|| appimage_named(assets, "-linux-x86_64.appimage"))
        }
        InstallKind::AppImage { vulkan: false, .. } => {
            appimage_named(assets, "-linux-x86_64.appimage")
        }
        // Same rule as the AppImage above: keep the GPU build when the release
        // has one, and take the CPU build rather than skip an update when it
        // does not — it runs on the same machine.
        InstallKind::WindowsInstaller { webgpu: true } => windows_named(assets, "-windows-x86_64-webgpu.exe")
            .or_else(|| windows_named(assets, "-windows-x86_64.exe")),
        InstallKind::WindowsInstaller { webgpu: false } => {
            windows_named(assets, "-windows-x86_64.exe")
        }
        InstallKind::ManagedPackage | InstallKind::Unmanaged => None,
    }
}

fn windows_named<'a>(assets: &'a [ReleaseAsset], suffix: &str) -> Option<&'a ReleaseAsset> {
    assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with(suffix))
}

fn appimage_named<'a>(assets: &'a [ReleaseAsset], suffix: &str) -> Option<&'a ReleaseAsset> {
    assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: 1,
            digest: None,
        }
    }

    /// The assets a real release carries, in the order GitHub returns them.
    fn release_assets() -> Vec<ReleaseAsset> {
        vec![
            asset("VoxCtrl_0.4.0_amd64-linux-x86_64-vulkan.AppImage"),
            asset("VoxCtrl_0.4.0_amd64-linux-x86_64-vulkan.deb"),
            asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage"),
            asset("VoxCtrl_0.4.0_amd64-linux-x86_64.deb"),
            asset("VoxCtrl_0.4.0_x64-setup-windows-x86_64.exe"),
        ]
    }

    #[test]
    fn a_cpu_appimage_is_replaced_by_the_cpu_appimage() {
        let assets = release_assets();
        let kind = InstallKind::AppImage { path: "/home/u/VoxCtrl.AppImage".into(), vulkan: false };
        let picked = select_asset(&kind, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage");
    }

    /// A GPU user who is silently moved onto the CPU build loses the
    /// acceleration they installed VoxCtrl for, and nothing tells them why.
    #[test]
    fn a_vulkan_appimage_stays_on_the_vulkan_build() {
        let assets = release_assets();
        let kind = InstallKind::AppImage { path: "/home/u/vk.AppImage".into(), vulkan: true };
        let picked = select_asset(&kind, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.4.0_amd64-linux-x86_64-vulkan.AppImage");
    }

    #[test]
    fn a_vulkan_install_falls_back_when_the_release_has_no_gpu_build() {
        let assets = vec![asset("VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage")];
        let kind = InstallKind::AppImage { path: "/home/u/vk.AppImage".into(), vulkan: true };
        assert_eq!(
            select_asset(&kind, &assets).unwrap().name,
            "VoxCtrl_0.4.0_amd64-linux-x86_64.AppImage"
        );
    }

    #[test]
    fn a_deb_is_never_offered_as_an_appimage_update() {
        let assets = vec![asset("VoxCtrl_0.4.0_amd64-linux-x86_64.deb")];
        let kind = InstallKind::AppImage { path: "/home/u/VoxCtrl.AppImage".into(), vulkan: false };
        assert!(select_asset(&kind, &assets).is_none());
    }

    #[test]
    fn windows_takes_the_installer() {
        let assets = release_assets();
        let picked =
            select_asset(&InstallKind::WindowsInstaller { webgpu: false }, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.4.0_x64-setup-windows-x86_64.exe");
    }

    #[test]
    fn a_webgpu_windows_install_stays_on_the_gpu_build() {
        let mut assets = release_assets();
        assets.push(asset("VoxCtrl_0.5.0_x64-setup-windows-x86_64-webgpu.exe"));

        let picked =
            select_asset(&InstallKind::WindowsInstaller { webgpu: true }, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.5.0_x64-setup-windows-x86_64-webgpu.exe");
    }

    #[test]
    fn a_webgpu_windows_install_takes_the_cpu_build_rather_than_skip_an_update() {
        // Same rule the Vulkan AppImage follows: a release that shipped only
        // the CPU installer is still an upgrade worth taking.
        let assets = release_assets();
        let picked =
            select_asset(&InstallKind::WindowsInstaller { webgpu: true }, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.4.0_x64-setup-windows-x86_64.exe");
    }

    #[test]
    fn a_cpu_windows_install_is_never_upgraded_onto_the_gpu_build() {
        // The GPU installer needs a GPU; handing it to a machine that asked for
        // the CPU build would be a downgrade dressed as an update.
        let mut assets = release_assets();
        assets.push(asset("VoxCtrl_0.5.0_x64-setup-windows-x86_64-webgpu.exe"));

        let picked =
            select_asset(&InstallKind::WindowsInstaller { webgpu: false }, &assets).unwrap();
        assert_eq!(picked.name, "VoxCtrl_0.4.0_x64-setup-windows-x86_64.exe");
    }

    #[test]
    fn an_empty_release_selects_nothing() {
        let kind = InstallKind::AppImage { path: "/x.AppImage".into(), vulkan: false };
        assert!(select_asset(&kind, &[]).is_none());
    }

    #[test]
    fn package_installs_have_no_asset_and_say_why() {
        assert!(select_asset(&InstallKind::ManagedPackage, &release_assets()).is_none());
        assert!(!InstallKind::ManagedPackage.can_self_update());
        assert!(InstallKind::ManagedPackage
            .unsupported_reason()
            .unwrap()
            .contains("package manager"));
    }

    #[test]
    fn an_appimage_can_update_itself() {
        let kind = InstallKind::AppImage { path: "/x.AppImage".into(), vulkan: false };
        assert!(kind.can_self_update());
        assert!(kind.unsupported_reason().is_none());
    }

    #[test]
    fn the_vulkan_flavour_is_read_off_the_file_name() {
        assert!(is_vulkan_build(Path::new("/a/VoxCtrl_0.3.10_amd64-linux-x86_64-vulkan.AppImage")));
        assert!(!is_vulkan_build(Path::new("/a/VoxCtrl_0.3.10_amd64-linux-x86_64.AppImage")));
    }

    #[test]
    fn a_cargo_build_is_not_something_to_overwrite() {
        assert_eq!(
            classify_exe_path(Path::new("/home/u/VoxCtrl/target/debug/voxctrl")),
            InstallKind::Unmanaged
        );
        assert_eq!(
            classify_exe_path(Path::new("/home/u/VoxCtrl/target/release/voxctrl")),
            InstallKind::Unmanaged
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_system_path_is_a_package_managers_business() {
        assert_eq!(classify_exe_path(Path::new("/usr/bin/voxctrl")), InstallKind::ManagedPackage);
        assert_eq!(classify_exe_path(Path::new("/opt/voxctrl/voxctrl")), InstallKind::ManagedPackage);
    }
}
