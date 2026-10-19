//! SteelMC downloader CLI: prompts and progress only; work lives in `fetch` and `build`.

mod build;
mod fetch;

use std::fmt::Display;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use build::BuildOptions;
use cliclack::{
    ProgressBar, confirm, input, intro, log, multiselect, outro, outro_cancel, progress_bar,
    select, spinner,
};
use self_update::update::{Release, ReleaseAsset};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Fallback bar when the server sends no `Content-Length` (chunked responses).
const BYTES_TEMPLATE: &str = "{msg} [{elapsed_precise}] {spinner} {bytes}";

#[derive(Clone, PartialEq, Eq)]
enum Mode {
    Prebuilt,
    Source,
}

#[derive(Clone, PartialEq, Eq)]
enum SourceChoice {
    Latest,
    Previous,
    PullRequest,
}

enum Target {
    Release(Release),
    PullRequest(u64),
}

fn install_steel() -> Result<()> {
    intro("SteelMC downloader")?;

    let mode = select("How do you want to install?")
        .item(
            Mode::Prebuilt,
            "Prebuilt binary",
            "installs in seconds, slower at runtime, fixed features",
        )
        .item(
            Mode::Source,
            "Build from source",
            "slow install, faster at runtime, pick features",
        )
        .interact()?;

    match mode {
        Mode::Prebuilt => install_prebuilt(),
        Mode::Source => install_from_source(),
    }
}

fn install_prebuilt() -> Result<()> {
    let releases = fetch_releases()?;
    let release = pick_prebuilt_release(&releases)?;
    let asset = pick_asset(&release)?;

    log::info(format!("{} -> {}", release.name(), asset.name()))?;

    let dest = PathBuf::from(asset.name());
    if !overwrite_ok(&dest)? {
        return Ok(outro_cancel("Nothing downloaded")?);
    }

    save(
        asset.download_url(),
        &dest,
        format!("Downloading {}", asset.name()),
    )?;
    fetch::make_executable(&dest)?;

    log::success(format!("Run it with: ./{}", dest.display()))?;
    outro("Done")?;
    Ok(())
}

fn install_from_source() -> Result<()> {
    ensure_toolchain()?;

    let releases = fetch_releases()?;
    let (label, url, tag) = match choose_source(&releases)? {
        Target::Release(release) => (
            release.name().to_string(),
            fetch::tag_tarball(release.version()),
            format!("v{}", release.version()),
        ),
        Target::PullRequest(number) => {
            if !check_ci(number)? {
                return Ok(outro_cancel("Nothing installed")?);
            }
            (
                format!("PR #{number}"),
                fetch::pull_request_url(number),
                format!("pr-{number}"),
            )
        }
    };

    let dest = PathBuf::from(format!("steel{}", std::env::consts::EXE_SUFFIX));
    if !overwrite_ok(&dest)? {
        return Ok(outro_cancel("Nothing installed")?);
    }

    build_source(&label, &url, &tag, &dest)
}

fn build_source(label: &str, url: &str, tag: &str, dest: &Path) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let work = cwd.join(format!(".steel-src-{tag}"));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;

    let archive = work.join("source.tar.gz");
    save(url, &archive, format!("Downloading {label}"))?;
    let root = fetch::extract_tarball(&archive, &work.join("src"))?;

    let (features, custom) = choose_features(&root)?;
    let features: Vec<&str> = features.iter().map(String::as_str).collect();
    let options = BuildOptions {
        features: &features,
        package: Some("steel"),
        release: true,
        no_default_features: custom,
    };

    log::step(format!("Building {label}; this takes a while"))?;
    let out = match build::cargo_build(&root, &options) {
        Ok(out) => out,
        Err(err) => {
            log::warning(format!("Build failed; source kept in {}", root.display()))?;
            return Err(err.into());
        }
    };

    let built = out.join(dest.file_name().unwrap_or_default());
    if !built.is_file() {
        return Err(format!("cargo finished but {} is missing", built.display()).into());
    }

    fs::copy(&built, dest)?;
    fetch::make_executable(dest)?;
    let _ = fs::remove_dir_all(&work);

    log::success(format!("Installed {}", dest.display()))?;
    log::success(format!("Run it with: ./{}", dest.display()))?;
    outro("Done")?;
    Ok(())
}

type Installer = fn() -> std::io::Result<()>;

fn ensure_toolchain() -> Result<()> {
    let tools: [(&str, Installer); 2] = [
        ("rustc", build::install_rust),
        ("cargo", build::install_cargo),
    ];

    for (tool, install) in tools {
        if build::version_of(tool).is_some() {
            continue;
        }

        if !confirm(format!("{tool} not found. Install it now?"))
            .initial_value(true)
            .interact()?
        {
            return Err(format!("{tool} is required to build from source").into());
        }

        install()?;

        if build::version_of(tool).is_none() {
            return Err(format!("{tool} still not found after install; restart your shell").into());
        }
    }

    if let Some(hint) = build::linker_hint() {
        log::warning(format!(
            "Linker prerequisites look incomplete. {hint} The build is likely to fail at link time until that is fixed."
        ))?;
    }
    Ok(())
}

fn fetch_releases() -> Result<Vec<Release>> {
    let spin = spinner();
    spin.start("Fetching releases");

    match fetch::releases() {
        Ok(releases) => {
            spin.stop(format!("Found {} releases", releases.len()));
            Ok(releases)
        }
        Err(err) => {
            spin.error("Could not fetch releases");
            Err(err)
        }
    }
}

fn pick_prebuilt_release(releases: &[Release]) -> Result<Release> {
    let latest = releases
        .first()
        .ok_or("this repository has no published releases")?;
    let Some(previous) = releases.get(1) else {
        return Ok(latest.clone());
    };

    let index = select("Which release?")
        .item(0usize, "Latest", release_hint(latest))
        .item(1usize, "Previous", release_hint(previous))
        .interact()?;

    Ok(if index == 0 {
        latest.clone()
    } else {
        previous.clone()
    })
}

fn choose_source(releases: &[Release]) -> Result<Target> {
    let mut prompt = select("What do you want to build?");

    if let Some(latest) = releases.first() {
        prompt = prompt.item(SourceChoice::Latest, "Latest release", release_hint(latest));
    }
    if let Some(previous) = releases.get(1) {
        prompt = prompt.item(
            SourceChoice::Previous,
            "Previous release",
            release_hint(previous),
        );
    }
    prompt = prompt.item(
        SourceChoice::PullRequest,
        "Pull request",
        "PR head, checked against its CI",
    );

    let choice = prompt.interact()?;
    match choice {
        SourceChoice::Latest => Ok(Target::Release(
            releases
                .first()
                .cloned()
                .ok_or("this repository has no published releases")?,
        )),
        SourceChoice::Previous => Ok(Target::Release(
            releases
                .get(1)
                .cloned()
                .ok_or("there is no previous release")?,
        )),
        SourceChoice::PullRequest => Ok(Target::PullRequest(ask_pr_number()?)),
    }
}

fn ask_pr_number() -> Result<u64> {
    let raw: String = input("Pull request number")
        .placeholder("1234")
        .interact()?;
    raw.trim()
        .parse()
        .map_err(|_| format!("`{raw}` is not a PR number").into())
}

fn release_hint(release: &Release) -> String {
    let date: String = release.date().chars().take(10).collect();
    format!("{} ({date})", release.name())
}

fn pick_asset(release: &Release) -> Result<ReleaseAsset> {
    let candidates = fetch::platform_assets(release);

    match candidates.len() {
        0 => {
            let available: Vec<&str> = release.assets().iter().map(|a| a.name()).collect();
            Err(format!(
                "{} has no build for {} {} (available: {})",
                release.name(),
                fetch::Platform::current().name(),
                std::env::consts::ARCH,
                available.join(", ")
            )
            .into())
        }
        1 => Ok(candidates.into_iter().next().unwrap()),
        _ => {
            // e.g. on x86_64 Linux: steel-linux (glibc) vs steel-linux-musl (static)
            let mut prompt = select("Which build?");
            for (i, asset) in candidates.iter().enumerate() {
                let hint = if asset.name().ends_with("musl") {
                    "static"
                } else {
                    "glibc"
                };
                prompt = prompt.item(i, asset.name(), hint);
            }
            let choice = prompt.interact()?;
            Ok(candidates.into_iter().nth(choice).unwrap())
        }
    }
}

/// `false` means the user chose not to overwrite an existing file.
fn overwrite_ok(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    Ok(confirm(format!("{} exists. Overwrite?", path.display())).interact()?)
}

/// Feature names from `cargo metadata`, defaults preselected.
///
/// `true` means the selection is exhaustive, so the build uses
/// `--no-default-features` and enables exactly what was picked.
fn choose_features(directory: &Path) -> Result<(Vec<String>, bool)> {
    let available = build::features(directory, "steel")?;
    if available.names.is_empty() {
        return Ok((Vec::new(), false));
    }

    if !confirm("Customize feature flags?")
        .initial_value(false)
        .interact()?
    {
        return Ok((Vec::new(), false));
    }

    let mut prompt = multiselect("Features");
    for name in &available.names {
        let hint = if available.defaults.contains(name) {
            "default"
        } else {
            ""
        };
        prompt = prompt.item(name.clone(), name, hint);
    }

    let selected = prompt
        .initial_values(available.defaults.clone())
        .required(false)
        .interact()?;

    Ok((selected, true))
}

/// Check a PR's GitHub checks. `false` means the user backed out after a warning.
fn check_ci(number: u64) -> Result<bool> {
    let spin = spinner();
    spin.start(format!("Checking CI for PR #{number}"));

    let (passed, message) = match fetch::pull_request_ci(number) {
        fetch::Ci::Passed => {
            spin.stop("CI passed for this PR");
            return Ok(true);
        }
        fetch::Ci::Failed(checks) => {
            spin.error("CI is failing for this PR");
            (false, format!("Failing checks: {}", checks.join(", ")))
        }
        fetch::Ci::Running => {
            spin.stop("CI is still running for this PR");
            (true, "Checks have not finished yet".to_string())
        }
        fetch::Ci::Unknown(why) => {
            spin.stop("No CI result for this PR");
            (true, format!("{why}; this build probably won't work"))
        }
    };

    log::warning(message)?;
    Ok(confirm("Build anyway?").initial_value(passed).interact()?)
}

/// Download `url` to `dest`, with a bar built on the first progress callback.
fn save(url: &str, dest: &Path, label: impl Display) -> Result<()> {
    let progress = Progress::new(label);

    match fetch::download(url, dest, progress.callback()) {
        Ok(()) => {
            match progress.take() {
                Some(bar) => bar.stop(format!("Saved {}", dest.display())),
                None => log::success(format!("Saved {}", dest.display()))?,
            }
            Ok(())
        }
        Err(err) => {
            if let Some(bar) = progress.take() {
                bar.error("Download failed");
            }
            Err(err)
        }
    }
}

/// cliclack bar, created lazily because the size is only known on the first callback.
struct Progress {
    label: String,
    bar: Arc<Mutex<Option<ProgressBar>>>,
}

impl Progress {
    fn new(label: impl Display) -> Self {
        Self {
            label: label.to_string(),
            bar: Arc::default(),
        }
    }

    fn callback(&self) -> impl Fn(u64, Option<u64>) + Send + Sync + 'static {
        let label = self.label.clone();
        let slot = Arc::clone(&self.bar);

        move |done, total| {
            let mut slot = slot.lock().unwrap();
            let bar = slot.get_or_insert_with(|| {
                let bar = match total {
                    Some(total) => progress_bar(total).with_download_template(),
                    None => progress_bar(0).with_template(BYTES_TEMPLATE),
                };
                bar.start(&label);
                bar
            });
            if let Some(total) = total {
                bar.set_length(total);
            }
            bar.set_position(done);
        }
    }

    fn take(&self) -> Option<ProgressBar> {
        self.bar.lock().unwrap().take()
    }
}

fn main() {
    if let Err(err) = install_steel() {
        // cliclack reports Esc / Ctrl-C as an Interrupted io::Error.
        let cancelled = err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == ErrorKind::Interrupted);

        if !cancelled {
            let _ = log::error(err.to_string());
        } // else: the prompt already printed its own "Operation cancelled." line
        std::process::exit(1);
    }
}
