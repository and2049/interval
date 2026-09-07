//! Self-update: replacing the installed `interval-desktop` with a newer GitHub release.
//!
//! The install shape is one file — the binary the installers drop in `~/.interval/bin`
//! (or wherever the user moved it) — so an update swaps exactly that file with the one
//! inside the platform archive of the newest release (`interval-desktop-<target>.tar.gz`,
//! `.zip` on Windows). Nothing installed is touched until the archive has downloaded and
//! unpacked completely; the swap itself is two renames inside the install directory.
//!
//! Versions are the release tags, `YY-M-D.N`, ordered as a date and then a same-day index.
//! A binary built without `INTERVAL_VERSION` (anything but the release workflow) reports
//! no version and refuses to update itself, so `cargo run` never overwrites a working
//! tree's binary with a download.
//!
//! The `version` file the installers keep beside the binary is rewritten on a successful
//! update, so `install` / `install.ps1` and this module agree on what is installed.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const REPO: &str = "and2049/interval";
const BIN_STEM: &str = "interval-desktop";
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// The release version stamped by the workflow, or `None` for a working-tree build.
pub fn current_version() -> Option<&'static str> {
    option_env!("INTERVAL_VERSION").filter(|version| !version.is_empty())
}

pub fn releases_url() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("development build — install a release before updating")]
    DevBuild,
    #[error("could not reach GitHub: {0}")]
    Network(String),
    #[error("no release build for {0}")]
    UnsupportedPlatform(String),
    #[error("release v{version} has no asset named {asset}")]
    MissingAsset { version: String, asset: String },
    #[error("{0} is not writable — reinstall interval from {1}")]
    NotWritable(PathBuf, String),
    #[error("download was incomplete or corrupt — try again")]
    CorruptDownload,
    #[error("{0}")]
    Io(String),
}

fn net(error: reqwest::Error) -> UpdateError {
    UpdateError::Network(error.to_string())
}

fn io(error: std::io::Error) -> UpdateError {
    UpdateError::Io(error.to_string())
}

/// A release tag `vYY-M-D.N` reduced to its ordering key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReleaseVersion {
    year: u32,
    month: u32,
    day: u32,
    index: u32,
}

impl ReleaseVersion {
    pub fn parse(version: &str) -> Option<Self> {
        let (date, index) = version.trim_start_matches('v').split_once('.')?;
        let mut parts = date.split('-').map(str::parse::<u32>);
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Ok(year)), Some(Ok(month)), Some(Ok(day)), None) => Some(Self {
                year,
                month,
                day,
                index: index.parse().ok()?,
            }),
            _ => None,
        }
    }
}

/// True when `candidate` supersedes `current`. Versions the tag scheme cannot parse fall
/// back to inequality: offering a no-op update beats stranding someone on an old build.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (
        ReleaseVersion::parse(candidate),
        ReleaseVersion::parse(current),
    ) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => candidate != current,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

impl Release {
    pub fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }

    fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

fn client() -> Result<reqwest::Client, UpdateError> {
    reqwest::Client::builder()
        .user_agent(format!("{BIN_STEM}/{}", current_version().unwrap_or("dev")))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(net)
}

pub async fn latest_release() -> Result<Release, UpdateError> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let response = client()?.get(url).send().await.map_err(net)?;
    if response.status() == reqwest::StatusCode::FORBIDDEN
        && response
            .headers()
            .get("x-ratelimit-remaining")
            .is_some_and(|value| value == "0")
    {
        return Err(UpdateError::Network(
            "GitHub API rate limit reached — try again later".into(),
        ));
    }
    response
        .error_for_status()
        .map_err(net)?
        .json::<Release>()
        .await
        .map_err(net)
}

/// The `<os>-<arch>` fragment in archive names, or `None` on a platform CI does not build.
pub fn platform_target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("windows", "x86_64") => "windows-x64",
        _ => return None,
    })
}

pub fn asset_name(target: &str) -> String {
    let ext = if target.starts_with("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("{BIN_STEM}-{target}.{ext}")
}

pub enum Check {
    UpToDate,
    Available(Release),
}

/// Ask GitHub whether anything newer exists. Does not touch the filesystem.
pub async fn check() -> Result<Check, UpdateError> {
    let current = current_version().ok_or(UpdateError::DevBuild)?;
    let release = latest_release().await?;
    if is_newer(release.version(), current) {
        Ok(Check::Available(release))
    } else {
        Ok(Check::UpToDate)
    }
}

/// The running binary and the directory the swap happens in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub dir: PathBuf,
    pub binary: PathBuf,
}

/// Inspect the running install. Proves the directory is writable before anything is
/// downloaded, so a system-owned path fails immediately rather than after the transfer.
pub fn plan() -> Result<Plan, UpdateError> {
    let binary = std::env::current_exe().map_err(io)?;
    let dir = binary
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| UpdateError::Io(format!("{} has no parent", binary.display())))?;
    let probe = dir.join(format!(".interval-write-probe-{}", std::process::id()));
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
        }
        Err(_) => return Err(UpdateError::NotWritable(dir, releases_url())),
    }
    Ok(Plan { dir, binary })
}

/// Delete the `.old` backup a previous update left behind. Windows cannot unlink the image
/// of a running process, so [`apply`] leaves it and startup, when nothing holds it, clears it.
pub fn sweep_backups() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(backup_path(&exe));
    }
}

/// A downloaded release, unpacked and waiting to be renamed into place.
#[derive(Debug)]
pub struct Staged {
    staging: PathBuf,
    source: PathBuf,
    dest: PathBuf,
    pub version: String,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.staging);
    }
}

/// Download this platform's asset and unpack the binary into a staging directory next to
/// the install. `on_progress` receives 0-100 as the transfer proceeds.
pub async fn download(
    plan: Plan,
    release: &Release,
    mut on_progress: impl FnMut(u8),
) -> Result<Staged, UpdateError> {
    let target = platform_target().ok_or_else(|| {
        UpdateError::UnsupportedPlatform(format!(
            "{}-{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ))
    })?;
    let name = asset_name(target);
    let asset = release
        .asset(&name)
        .ok_or_else(|| UpdateError::MissingAsset {
            version: release.version().to_string(),
            asset: name,
        })?;
    let bytes = fetch(asset, &mut on_progress).await?;
    stage(&bytes, &plan, release.version())
}

fn stage(archive: &[u8], plan: &Plan, version: &str) -> Result<Staged, UpdateError> {
    let staging = plan
        .dir
        .join(format!(".interval-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).map_err(io)?;
    let wanted = plan
        .binary
        .file_name()
        .ok_or_else(|| UpdateError::Io("install path has no file name".into()))?;
    let staged = Staged {
        source: staging.join(wanted),
        staging,
        dest: plan.binary.clone(),
        version: version.to_string(),
    };
    unpack(archive, &staged.source, wanted)?;
    let usable =
        std::fs::metadata(&staged.source).is_ok_and(|meta| meta.is_file() && meta.len() > 0);
    if !usable {
        return Err(UpdateError::Io(format!(
            "release archive is missing {}",
            wanted.to_string_lossy()
        )));
    }
    set_executable(&staged.source);
    Ok(staged)
}

async fn fetch(asset: &Asset, on_progress: &mut impl FnMut(u8)) -> Result<Vec<u8>, UpdateError> {
    let mut response = client()?
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(net)?
        .error_for_status()
        .map_err(net)?;
    let expected = response.content_length().unwrap_or(asset.size);
    let total = expected.max(1);
    let mut bytes: Vec<u8> = Vec::with_capacity(total as usize);
    let mut last_reported = u8::MAX;
    while let Some(chunk) = response.chunk().await.map_err(net)? {
        bytes.extend_from_slice(&chunk);
        let percent = ((bytes.len() as u64 * 100) / total).min(100) as u8;
        if percent != last_reported {
            last_reported = percent;
            on_progress(percent);
        }
    }
    if expected > 0 && bytes.len() as u64 != expected {
        return Err(UpdateError::CorruptDownload);
    }
    Ok(bytes)
}

/// Extract the one member named `wanted` to `destination`. Archive paths are untrusted, so
/// only the member's file name is consulted and the destination is always ours. The format
/// is read from the magic bytes, so both archive shapes are testable on every platform.
fn unpack(archive: &[u8], destination: &Path, wanted: &OsStr) -> Result<(), UpdateError> {
    if archive.starts_with(b"PK") {
        unpack_zip(archive, destination, wanted)
    } else {
        unpack_tar_gz(archive, destination, wanted)
    }
}

fn unpack_tar_gz(archive: &[u8], destination: &Path, wanted: &OsStr) -> Result<(), UpdateError> {
    let mut decoder = flate2::read::GzDecoder::new(archive);
    {
        let mut tar = tar::Archive::new(&mut decoder);
        for entry in tar.entries().map_err(io)? {
            let mut entry = entry.map_err(io)?;
            let is_wanted = entry
                .path()
                .ok()
                .and_then(|path| path.file_name().map(|name| name == wanted))
                .unwrap_or(false);
            if is_wanted && entry.header().entry_type().is_file() {
                entry.unpack(destination).map_err(io)?;
            }
        }
    }
    // `tar` stops at the end-of-archive marker and never reads the gzip trailer, so the
    // CRC32 is only checked once the rest of the stream is drained.
    std::io::copy(&mut decoder, &mut std::io::sink()).map_err(|_| UpdateError::CorruptDownload)?;
    Ok(())
}

fn unpack_zip(archive: &[u8], destination: &Path, wanted: &OsStr) -> Result<(), UpdateError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .map_err(|_| UpdateError::CorruptDownload)?;
    for index in 0..zip.len() {
        let mut file = zip
            .by_index(index)
            .map_err(|_| UpdateError::CorruptDownload)?;
        let is_wanted = file
            .enclosed_name()
            .and_then(|path| path.file_name().map(|name| name == wanted))
            .unwrap_or(false);
        if is_wanted && file.is_file() {
            let mut out = std::fs::File::create(destination).map_err(io)?;
            std::io::copy(&mut file, &mut out).map_err(|_| UpdateError::CorruptDownload)?;
        }
    }
    Ok(())
}

/// Rename the staged binary over the installed one and return the version now installed.
///
/// The old binary is moved aside rather than overwritten: on Windows that is the only way
/// to replace a running `.exe`, and everywhere it is what a failed second rename restores.
pub fn apply(staged: Staged) -> Result<String, UpdateError> {
    let backup = backup_path(&staged.dest);
    let _ = std::fs::remove_file(&backup);
    let saved = staged.dest.exists();
    if saved {
        rename_with_retry(&staged.dest, &backup).map_err(io)?;
    }
    if let Err(error) = rename_with_retry(&staged.source, &staged.dest) {
        if saved {
            let _ = std::fs::rename(&backup, &staged.dest);
        }
        return Err(io(error));
    }
    set_executable(&staged.dest);
    if saved && !cfg!(windows) {
        let _ = std::fs::remove_file(&backup);
    }
    if let Some(dir) = staged.dest.parent() {
        let _ = std::fs::write(dir.join("version"), &staged.version);
    }
    Ok(staged.version.clone())
}

/// Windows briefly reports a sharing violation while a virus scanner holds a freshly
/// written file; a few short retries turn that into a slightly slower update.
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(_) if cfg!(windows) && attempt < 4 => {
                attempt += 1;
                std::thread::sleep(std::time::Duration::from_millis(150 * attempt));
            }
            Err(error) => return Err(error),
        }
    }
}

fn backup_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    dest.with_file_name(name)
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if path.is_file() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn bin_name() -> String {
        if cfg!(windows) {
            format!("{BIN_STEM}.exe")
        } else {
            BIN_STEM.to_string()
        }
    }

    #[test]
    fn release_versions_order_by_date_then_same_day_index() {
        let parse = |tag: &str| ReleaseVersion::parse(tag).unwrap();
        assert!(parse("26-9-6.0") < parse("26-9-6.1"));
        assert!(parse("26-9-6.9") < parse("26-10-1.0"));
        assert!(parse("26-12-31.5") < parse("27-1-1.0"));
        assert_eq!(parse("v26-9-6.0"), parse("26-9-6.0"));
        assert!(ReleaseVersion::parse("0.1.0").is_none());
        assert!(ReleaseVersion::parse("26-9.0").is_none());
        assert!(ReleaseVersion::parse("26-9-6").is_none());
    }

    #[test]
    fn is_newer_compares_tags_and_falls_back_to_inequality() {
        assert!(is_newer("26-9-7.0", "26-9-6.3"));
        assert!(is_newer("26-9-6.10", "26-9-6.9"));
        assert!(!is_newer("26-9-6.0", "26-9-6.0"));
        assert!(!is_newer("26-9-5.0", "26-9-6.0"));
        assert!(is_newer("nightly", "26-9-6.0"));
        assert!(!is_newer("nightly", "nightly"));
    }

    #[test]
    fn asset_names_match_the_release_workflow() {
        assert_eq!(
            asset_name("windows-x64"),
            "interval-desktop-windows-x64.zip"
        );
        assert_eq!(asset_name("linux-x64"), "interval-desktop-linux-x64.tar.gz");
        assert_eq!(
            asset_name("macos-arm64"),
            "interval-desktop-macos-arm64.tar.gz"
        );
    }

    #[test]
    fn backups_keep_the_original_extension() {
        assert_eq!(
            backup_path(Path::new("/opt/interval/interval-desktop.exe")),
            PathBuf::from("/opt/interval/interval-desktop.exe.old")
        );
        assert_eq!(
            backup_path(Path::new("/opt/interval/interval-desktop")),
            PathBuf::from("/opt/interval/interval-desktop.old")
        );
    }

    #[test]
    fn a_release_payload_deserializes_and_looks_assets_up_by_name() {
        let release: Release = serde_json::from_str(
            r#"{"tag_name":"v26-9-6.0","assets":[
                {"name":"a.tar.gz","browser_download_url":"https://x/a","size":12},
                {"name":"b.zip","browser_download_url":"https://x/b"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(release.version(), "26-9-6.0");
        assert_eq!(release.asset("a.tar.gz").unwrap().size, 12);
        assert_eq!(release.asset("b.zip").unwrap().size, 0);
        assert!(release.asset("missing").is_none());
    }

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *contents).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn zip_archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    struct Install {
        dir: PathBuf,
        plan: Plan,
    }

    impl Install {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("interval-update-test-{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let binary = dir.join(bin_name());
            std::fs::write(&binary, b"old binary").unwrap();
            let plan = Plan {
                dir: dir.clone(),
                binary,
            };
            Self { dir, plan }
        }

        fn binary(&self) -> Vec<u8> {
            std::fs::read(&self.plan.binary).unwrap()
        }
    }

    impl Drop for Install {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn a_tar_gz_release_is_staged_then_swapped_in() {
        let install = Install::new("tar");
        let archive = tar_gz(&[(bin_name().as_str(), b"new binary")]);

        let staged = stage(&archive, &install.plan, "26-9-7.0").unwrap();
        assert_eq!(install.binary(), b"old binary");
        let staging = staged.staging.clone();

        assert_eq!(apply(staged).unwrap(), "26-9-7.0");
        assert_eq!(install.binary(), b"new binary");
        assert_eq!(
            std::fs::read_to_string(install.dir.join("version")).unwrap(),
            "26-9-7.0"
        );
        assert!(!staging.exists());
        assert_eq!(backup_path(&install.plan.binary).exists(), cfg!(windows));
    }

    #[test]
    fn a_zip_release_is_staged_then_swapped_in() {
        let install = Install::new("zip");
        let archive = zip_archive(&[(bin_name().as_str(), b"new binary")]);

        let staged = stage(&archive, &install.plan, "26-9-7.0").unwrap();
        assert_eq!(install.binary(), b"old binary");
        apply(staged).unwrap();
        assert_eq!(install.binary(), b"new binary");
    }

    #[test]
    fn members_with_a_leading_dot_slash_still_match() {
        let install = Install::new("dot-slash");
        let name = format!("./{}", bin_name());
        let archive = tar_gz(&[("./", b""), (name.as_str(), b"new binary")]);
        let staged = stage(&archive, &install.plan, "26-9-7.0").unwrap();
        apply(staged).unwrap();
        assert_eq!(install.binary(), b"new binary");
    }

    #[test]
    fn a_truncated_archive_is_rejected_before_anything_is_touched() {
        let install = Install::new("truncated");
        let archive = tar_gz(&[(bin_name().as_str(), b"new binary")]);
        let truncated = &archive[..archive.len() - 8];
        assert!(matches!(
            stage(truncated, &install.plan, "26-9-7.0"),
            Err(UpdateError::CorruptDownload)
        ));
        assert_eq!(install.binary(), b"old binary");
        assert!(!install.dir.join("version").exists());
    }

    #[test]
    fn an_archive_without_the_binary_is_rejected() {
        let install = Install::new("missing");
        let archive = zip_archive(&[("README.md", b"not a binary")]);
        let error = stage(&archive, &install.plan, "26-9-7.0").unwrap_err();
        assert!(matches!(error, UpdateError::Io(_)), "got {error:?}");
        assert_eq!(install.binary(), b"old binary");
    }

    fn serve_once(body: Vec<u8>, declared_len: usize) -> String {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut discard = [0u8; 1024];
            let _ = stream.read(&mut discard);
            let _ = stream.write_all(
                format!("HTTP/1.1 200 OK\r\nContent-Length: {declared_len}\r\n\r\n").as_bytes(),
            );
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        });
        format!("http://127.0.0.1:{port}/asset.tar.gz")
    }

    #[tokio::test]
    async fn a_download_streams_to_completion_and_reports_progress() {
        let body: Vec<u8> = (0..64_000u32).map(|byte| byte as u8).collect();
        let asset = Asset {
            name: "asset.tar.gz".into(),
            browser_download_url: serve_once(body.clone(), body.len()),
            size: body.len() as u64,
        };
        let mut seen = Vec::new();
        let fetched = fetch(&asset, &mut |percent| seen.push(percent))
            .await
            .unwrap();
        assert_eq!(fetched, body);
        assert_eq!(seen.last(), Some(&100));
        assert!(seen.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[tokio::test]
    async fn a_short_body_is_not_accepted_as_a_complete_download() {
        let body = vec![7u8; 1_000];
        let asset = Asset {
            name: "asset.tar.gz".into(),
            browser_download_url: serve_once(body, 10_000),
            size: 10_000,
        };
        assert!(fetch(&asset, &mut |_| {}).await.is_err());
    }
}
