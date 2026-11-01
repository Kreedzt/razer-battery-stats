//! Embeds a `VERSIONINFO` resource so the release binary carries its version,
//! product name and the `razer-hid` MIT attribution.
//!
//! Written by hand rather than with `embed-resource`: that crate needs
//! `toml 0.9`/`winnow`, and those tarballs cannot be fetched from crates.io on
//! this machine (index queries succeed, downloads time out), so a build
//! dependency would make the project unbuildable offline. Everything here is
//! std-only: write a `.rc`, hand it to `rc.exe`, link the `.res`.
//!
//! A missing resource compiler is a warning, not an error: the panel runs fine
//! without version metadata, and failing the build over a cosmetic resource
//! would block everyone whose toolchain lacks the Windows SDK.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The upstream `razer-hid` identity, embedded so a bare binary still states
/// where its HID layer came from.
const UPSTREAM: &str = "https://github.com/xzeldon/razer-battery-report @ v1.3.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is always set by cargo"));
    let version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let Some(rc) = find_rc() else {
        println!(
            "cargo:warning=rc.exe not found (Windows SDK missing); \
             building without version metadata. Set RC=/path/to/rc.exe to supply one."
        );
        return;
    };

    let rc_file = out.join("version.rc");
    let res_file = out.join("version.res");
    fs::write(&rc_file, version_rc(&version)).expect("write version.rc");

    let output = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            // A `.res` is not an object file: the linker takes it as a bare
            // argument, which is exactly what a link-arg forwards verbatim.
            println!("cargo:rustc-link-arg-bins={}", res_file.display());
        }
        Ok(o) => {
            // Surface the compiler's own message: a malformed `.rc` here is a
            // build.rs bug, and the text is the only clue.
            println!(
                "cargo:warning=rc.exe failed ({}); building without version metadata: {}",
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Err(e) => {
            println!("cargo:warning=could not run rc.exe ({e}); building without version metadata")
        }
    }
}

/// Builds the `.rc` source for `VERSIONINFO` id 1, lang 0x0409 / cp 1200.
///
/// All four version words come from `CARGO_PKG_VERSION`; a malformed value
/// degrades to 0 rather than failing the build.
fn version_rc(version: &str) -> String {
    let mut parts = version.split('.').map(|p| p.parse().unwrap_or(0u16));
    let (major, minor, patch) = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    format!(
        r#"1 VERSIONINFO
 FILEVERSION {major},{minor},{patch},0
 PRODUCTVERSION {major},{minor},{patch},0
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS 0x40004L
 FILETYPE 0x1L
 FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "razer-battery-stats"
            VALUE "FileDescription", "Razer battery taskbar panel"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "razer-battery-taskbar"
            VALUE "LegalCopyright", "razer-hid (c) 2024 Timofey Gelazoniya, MIT"
            VALUE "OriginalFilename", "razer-battery-taskbar.exe"
            VALUE "ProductName", "Razer Battery Taskbar"
            VALUE "ProductVersion", "{version}"
            VALUE "Comments", "{UPSTREAM}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}

/// Locates the MSVC/Windows-SDK resource compiler.
///
/// `RC` wins when set to an existing file. Otherwise the Windows SDK layout is
/// probed newest-version-first, x64 only: `rc.exe` is a native host tool and the
/// SDK ships one per host architecture.
fn find_rc() -> Option<PathBuf> {
    if let Some(p) = env::var_os("RC").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    [
        r"C:\Program Files (x86)\Windows Kits\10\bin",
        r"C:\Program Files\Windows Kits\10\bin",
    ]
    .iter()
    .filter_map(|root| {
        let mut versions: Vec<PathBuf> = fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|e| e.path().join("x64").join("rc.exe"))
            .filter(|p| p.is_file())
            .collect();
        // Sort by the SDK version directory so the newest toolchain wins.
        versions.sort_by_key(|p| sdk_version_key(p));
        versions.pop()
    })
    .next()
}

/// Orders SDK directories numerically, so `10.0.26100.0` beats `10.0.9.0`
/// (a plain string sort would invert them).
fn sdk_version_key(rc_path: &Path) -> Vec<u64> {
    rc_path
        .parent() // x64
        .and_then(Path::parent) // version dir
        .and_then(|p| p.file_name())
        .map(|n| {
            n.to_string_lossy()
                .split('.')
                .map(|s| s.parse().unwrap_or(0))
                .collect()
        })
        .unwrap_or_default()
}
