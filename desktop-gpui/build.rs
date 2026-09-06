//! Two jobs:
//!
//! 1. Embeds the Windows app icon as resource ID 1. GPUI's Windows backend loads the
//!    window/taskbar icon from the executable's own resources via `LoadImageW(module, 1)`
//!    — there is no runtime icon API (`WindowOptions::icon` is X11-only). Deliberately
//!    does NOT embed an RT_MANIFEST: gpui's default `windows-manifest` feature already
//!    embeds one, and a second copy is a duplicate-resource link error.
//!    The VERSIONINFO block takes `INTERVAL_VERSION` when set (the release workflow passes
//!    the date-based `YY-M-D.N` release version, see the Releases section of `README.md`) and falls back to
//!    the crate version.
//!
//! 2. Downloads and embeds a pinned `uv` binary for the build target, so FastF1 ingest
//!    needs no Python on the user's machine: at runtime uv provisions a managed CPython
//!    and the fastf1 venv itself (see `embed.rs` / `fastf1_historical.rs`). The archive
//!    is sha256-pinned here — never trust the server's own checksum files, they travel
//!    over the same channel. Escape hatches for offline builds:
//!    `INTERVAL_UV_BINARY=<path>` embeds a local uv, `INTERVAL_SKIP_UV_EMBED=1` embeds
//!    nothing (the app then falls back to the system-python bootstrap).

use std::path::PathBuf;

fn main() {
    #[cfg(target_os = "windows")]
    windows_resources();
    embed_uv();
}

const UV_VERSION: &str = "0.12.6";

/// (archive name, sha256 of the archive, path of the uv binary inside the archive)
fn uv_artifact(target: &str) -> Option<(String, &'static str, String)> {
    let (archive, sha256, inner) = match target {
        "x86_64-unknown-linux-gnu" => (
            format!("uv-{target}.tar.gz"),
            "8681d8921e7d520fb368991dcf5f9c1905b80f5bf2a265a0ed085c8d8e342477",
            format!("uv-{target}/uv"),
        ),
        "aarch64-unknown-linux-gnu" => (
            format!("uv-{target}.tar.gz"),
            "d58030acd26159499ac82f32da12d1b3c12a3a1bfc414232d9082070c03e128d",
            format!("uv-{target}/uv"),
        ),
        "x86_64-apple-darwin" => (
            format!("uv-{target}.tar.gz"),
            "2a26ea71bbeff1c7e12c2cc40245c96a041deff276bc921e7038e304d5d3e04c",
            format!("uv-{target}/uv"),
        ),
        "aarch64-apple-darwin" => (
            format!("uv-{target}.tar.gz"),
            "14b459d51ea2e71eeba28c45a268c922bdf8607fc6455e3f40b4e082895d160d",
            format!("uv-{target}/uv"),
        ),
        "x86_64-pc-windows-msvc" => (
            format!("uv-{target}.zip"),
            "df7cb9f243eae1621400d4fcf5b1b3d90f20e264ece91b64deb3b0078abca6ef",
            "uv.exe".to_string(),
        ),
        "aarch64-pc-windows-msvc" => (
            format!("uv-{target}.zip"),
            "6dda514fbbe3152d980758e0f6347116060114d7d24932fc0ea5d8063f8b253a",
            "uv.exe".to_string(),
        ),
        _ => return None,
    };
    Some((archive, sha256, inner))
}

fn embed_uv() {
    println!("cargo:rerun-if-env-changed=INTERVAL_UV_BINARY");
    println!("cargo:rerun-if-env-changed=INTERVAL_SKIP_UV_EMBED");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // Version in the file name: OUT_DIR survives build-script edits, so a bare name
    // would keep serving a stale binary across UV_VERSION bumps.
    let embed_path = out_dir.join(format!("uv-embed-{UV_VERSION}"));

    let embedded_version = if std::env::var_os("INTERVAL_SKIP_UV_EMBED").is_some() {
        std::fs::write(&embed_path, []).unwrap();
        println!("cargo:warning=INTERVAL_SKIP_UV_EMBED set: no uv embedded; FastF1 ingest will need a system Python");
        ""
    } else if let Some(local) = std::env::var_os("INTERVAL_UV_BINARY") {
        std::fs::copy(&local, &embed_path)
            .unwrap_or_else(|e| panic!("INTERVAL_UV_BINARY {local:?} unreadable: {e}"));
        UV_VERSION
    } else {
        let target = std::env::var("TARGET").unwrap();
        match uv_artifact(&target) {
            Some((archive, sha256, inner)) => {
                if !embed_path.exists() {
                    download_and_extract_uv(&out_dir, &embed_path, &archive, sha256, &inner);
                }
                UV_VERSION
            }
            None => {
                std::fs::write(&embed_path, []).unwrap();
                println!("cargo:warning=no pinned uv artifact for target {target}; FastF1 ingest will need a system Python");
                ""
            }
        }
    };

    println!("cargo:rustc-env=INTERVAL_UV_EMBED_PATH={}", embed_path.display());
    println!("cargo:rustc-env=INTERVAL_UV_EMBED_VERSION={embedded_version}");
}

fn download_and_extract_uv(
    out_dir: &std::path::Path,
    embed_path: &std::path::Path,
    archive: &str,
    expected_sha256: &str,
    inner: &str,
) {
    let url =
        format!("https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{archive}");
    let archive_path = out_dir.join(archive);
    // curl exists out of the box on all three CI OSes (Windows 10+ ships curl.exe).
    let status = std::process::Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&archive_path)
        .arg(&url)
        .status()
        .expect("curl must be installed to download the pinned uv binary (or set INTERVAL_UV_BINARY / INTERVAL_SKIP_UV_EMBED=1)");
    assert!(status.success(), "downloading {url} failed");

    let bytes = std::fs::read(&archive_path).unwrap();
    let digest = {
        use sha2::Digest;
        sha2::Sha256::digest(&bytes)
            .iter()
            .fold(String::new(), |mut acc, byte| {
                use std::fmt::Write;
                let _ = write!(acc, "{byte:02x}");
                acc
            })
    };
    assert_eq!(
        digest, expected_sha256,
        "sha256 mismatch for {archive}: refusing to embed an unverified uv"
    );

    let extract_dir = out_dir.join("uv-extract");
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir).unwrap();
    // GNU tar handles the .tar.gz targets; on Windows tar.exe is bsdtar, which also
    // reads the .zip artifact.
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&extract_dir)
        .status()
        .expect("tar must be installed to unpack the pinned uv archive");
    assert!(status.success(), "extracting {archive} failed");

    // Rename last so an interrupted build never leaves a half-written embed file
    // behind for the existence check to trust.
    let staged = out_dir.join("uv-embed.partial");
    std::fs::copy(extract_dir.join(inner), &staged).unwrap();
    std::fs::rename(&staged, embed_path).unwrap();
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_file(&archive_path);
}

#[cfg(target_os = "windows")]
fn windows_resources() {
    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("app-icon/icon.ico");
    let icon = icon
        .canonicalize()
        .expect("app-icon/icon.ico must exist in desktop-gpui");
    // canonicalize() yields a \\?\-prefixed path, which rc.exe rejects.
    let icon_escaped = icon
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "\\\\");

    println!("cargo:rerun-if-env-changed=INTERVAL_VERSION");
    let pkg_version = std::env::var("INTERVAL_VERSION")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    let mut parts = pkg_version
        .split(['.', '-'])
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .chain(std::iter::repeat(0));
    let file_version = format!(
        "{},{},{},{}",
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );

    let rc_content = format!(
        r#"1 ICON "{icon_escaped}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
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
            VALUE "FileDescription", "interval\0"
            VALUE "FileVersion", "{pkg_version}\0"
            VALUE "ProductName", "interval\0"
            VALUE "ProductVersion", "{pkg_version}\0"
            VALUE "CompanyName", "interval\0"
            VALUE "OriginalFilename", "interval-desktop.exe\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let rc_path = out_dir.join("interval_resources.rc");
    std::fs::write(&rc_path, rc_content).unwrap();

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .unwrap();

    println!(
        "cargo:rerun-if-changed={}",
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("app-icon/icon.ico")
            .display()
    );
}
