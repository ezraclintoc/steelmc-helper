//! GitHub: release listing, platform assets, and downloads.
//!
//! The HTTP work comes from [`self_update`]; no prompts or progress output live here.

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use self_update::backends::github::ReleaseList;
use self_update::update::{Release, ReleaseAsset};
use self_update::ureq;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const OWNER: &str = "Steel-Foundation";
pub const REPO: &str = "SteelMC";

/// Refuse to stream more than this into one file (defends against a bad server).
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

pub fn releases() -> Result<Vec<Release>> {
    let list = ReleaseList::configure()
        .repo_owner(OWNER)
        .repo_name(REPO)
        .build()?
        .fetch()?;
    Ok(list.into_vec())
}

/// Platforms SteelMC publishes assets for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
    Windows,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        match env::consts::OS {
            "linux" => Self::Linux,
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            _ => Self::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Other => env::consts::OS,
        }
    }

    /// Prefix of this platform's assets; `None` means there is no build for it.
    fn asset_prefix(self) -> Option<&'static str> {
        match self {
            Self::Linux => Some("steel-linux"),
            Self::Macos => Some("steel-macos"),
            Self::Windows => Some("steel-windows"),
            Self::Other => None,
        }
    }
}

/// Assets that look like a build for the machine we're running on.
pub fn platform_assets(release: &Release) -> Vec<ReleaseAsset> {
    let Some(prefix) = Platform::current().asset_prefix() else {
        return Vec::new();
    };

    release
        .assets()
        .iter()
        .filter(|asset| asset.name().to_ascii_lowercase().starts_with(prefix))
        .cloned()
        .collect()
}

/// SteelMC's PR CI publishes no binaries, so the only PR artifact is the source tarball.
pub fn pull_request_url(number: u64) -> String {
    format!("https://codeload.github.com/{OWNER}/{REPO}/tar.gz/refs/pull/{number}/head")
}

/// Source tarball for a release. SteelMC tags are `v<version>`, and `Release::version()`
/// has that prefix stripped, so it is added back here.
pub fn tag_tarball(version: &str) -> String {
    let version = version.trim_start_matches('v');
    format!("https://codeload.github.com/{OWNER}/{REPO}/tar.gz/refs/tags/v{version}")
}

/// Unpack a `.tar.gz` and return its single top-level directory.
pub fn extract_tarball(archive: &Path, into: &Path) -> Result<PathBuf> {
    fs::create_dir_all(into)?;
    let file = File::open(archive)?;
    tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(into)?;

    let mut dirs = fs::read_dir(into)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir());

    match (dirs.next(), dirs.next()) {
        (Some(root), None) => Ok(root),
        _ => Ok(into.to_path_buf()),
    }
}

/// Result of looking up a PR's GitHub checks.
pub enum Ci {
    Passed,
    Failed(Vec<String>),
    Running,
    /// No checks, or the lookup itself failed (rate limit, 404).
    Unknown(String),
}

pub fn pull_request_ci(number: u64) -> Ci {
    match fetch_ci(number) {
        Ok(ci) => ci,
        Err(err) => Ci::Unknown(err.to_string()),
    }
}

fn fetch_ci(number: u64) -> Result<Ci> {
    let pull = api(&format!(
        "https://api.github.com/repos/{OWNER}/{REPO}/pulls/{number}"
    ))?;
    let sha = pull["head"]["sha"]
        .as_str()
        .ok_or("pull request response has no head SHA")?;

    let checks = api(&format!(
        "https://api.github.com/repos/{OWNER}/{REPO}/commits/{sha}/check-runs?per_page=100"
    ))?;
    let runs = checks["check_runs"]
        .as_array()
        .ok_or("check-runs response is malformed")?;

    if runs.is_empty() {
        return Ok(Ci::Unknown("no CI checks reported for this commit".into()));
    }

    let mut failed = Vec::new();
    let mut running = false;

    for run in runs {
        let name = run["name"].as_str().unwrap_or("check");
        if run["status"].as_str() != Some("completed") {
            running = true;
            continue;
        }
        match run["conclusion"].as_str().unwrap_or("unknown") {
            "success" | "neutral" | "skipped" => {}
            other => failed.push(format!("{name} ({other})")),
        }
    }

    Ok(if !failed.is_empty() {
        Ci::Failed(failed)
    } else if running {
        Ci::Running
    } else {
        Ci::Passed
    })
}

fn api(url: &str) -> Result<serde_json::Value> {
    let mut response = ureq::get(url)
        .header("Accept", "application/vnd.github+json")
        .call()?;
    Ok(response.body_mut().read_json()?)
}

/// Stream `url` into `dest`, calling `on_progress(done, total)` as bytes arrive.
///
/// `total` is `None` when the response has no `Content-Length` (codeload uses chunked
/// encoding). `self_update` requires the callback to be `'static`, so it cannot borrow
/// caller state; `Send + Sync` because it may run on the transport's thread.
pub fn download(
    url: &str,
    dest: &Path,
    on_progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
) -> Result<()> {
    let mut dl = self_update::Download::from_url(url);
    dl.show_download_progress(false)
        .timeout(Duration::from_secs(300))
        .max_download_size(MAX_DOWNLOAD);

    // GitHub release assets are handed out as `api.github.com/repos/.../releases/assets/ID`.
    // Without this Accept header that endpoint returns the asset's JSON metadata instead
    // of the bytes (the API redirects to the signed object URL when asked for octet-stream).
    dl.request_header(
        self_update::http::header::ACCEPT,
        "application/octet-stream",
    );

    dl.progress_callback(on_progress);

    let result = (|| {
        let mut file = File::create(dest)?;
        dl.download_to(&mut file)?;
        file.flush()?;
        Ok(())
    })();

    if result.is_err() {
        // Don't leave a truncated binary behind.
        let _ = fs::remove_file(dest);
    }
    result
}

#[cfg(unix)]
pub fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
pub fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}
