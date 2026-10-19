//! Toolchain helpers. Module (`mod build;`); a root `build.rs` is a Cargo build script.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `<tool> --version`, falling back to rustup's `bin` directory.
///
/// Installs use `--no-modify-path`, so a tool installed by this process is not on
/// PATH until the user's shell restarts; rustup still put it in `~/.cargo/bin`.
pub fn version_of(program: impl AsRef<OsStr>) -> Option<String> {
    resolve(program.as_ref()).map(|(_, version)| version)
}

// The resolution `version_of` checks is also what has to be spawned to build.
fn resolve(program: &OsStr) -> Option<(PathBuf, String)> {
    let direct = Path::new(program);
    if let Some(version) = version_line(direct) {
        return Some((direct.to_path_buf(), version));
    }

    let path = rustup_bin(program.to_str()?)?;
    let version = version_line(&path)?;
    Some((path, version))
}

fn version_line(program: &Path) -> Option<String> {
    let out = Command::new(program).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Some(stdout.lines().next().unwrap_or("").trim().to_string())
}

// Windows: HOME may be an MSYS path like /c/Users/x, so USERPROFILE comes first.
fn rustup_bin(name: &str) -> Option<PathBuf> {
    let home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".cargo")))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))?;

    let path = home
        .join("bin")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

// A toolchain bin directory ahead of the proxy on PATH pins cargo and rustc to
// that toolchain, ignoring the checkout's rust-toolchain.toml. The pinned channel
// is read from the checkout and both binaries come from the toolchain on disk;
// RUSTC is set because cargo otherwise takes rustc from PATH.
fn cargo_command(directory: &Path) -> io::Result<Command> {
    if let Some(channel) = pinned_channel(directory) {
        let toolchain = toolchain_dir(&channel).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "pinned toolchain `{channel}` is not installed (rustup toolchain install {channel})"
                ),
            )
        })?;
        return Ok(toolchain_command(&toolchain));
    }

    if let Some(rustup) = rustup_exe()
        && let Some(channel) = active_toolchain(&rustup, directory)
        && let Some(cargo) = rustup_which(&rustup, &channel, "cargo")
    {
        let mut command = Command::new(cargo);
        if let Some(rustc) = rustup_which(&rustup, &channel, "rustc") {
            command.env("RUSTC", rustc);
        }
        return Ok(command);
    }

    let cargo = cargo_bin().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "cargo not found; check version_of(\"cargo\") first",
        )
    })?;
    Ok(Command::new(cargo))
}

fn toolchain_command(toolchain: &Path) -> Command {
    let bin = toolchain.join("bin");
    let mut command = Command::new(bin.join(exe("cargo")));
    command.env("RUSTC", bin.join(exe("rustc")));
    command
}

fn exe(name: &str) -> String {
    format!("{name}{}", env::consts::EXE_SUFFIX)
}

// `rust-toolchain.toml` (channel = "...") or the legacy plain `rust-toolchain`.
fn pinned_channel(directory: &Path) -> Option<String> {
    if let Ok(text) = fs::read_to_string(directory.join("rust-toolchain.toml")) {
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("channel") else {
                continue;
            };
            let Some(value) = rest.trim_start().strip_prefix('=') else {
                continue;
            };
            return Some(value.trim().trim_matches('"').to_string());
        }
    }

    let text = fs::read_to_string(directory.join("rust-toolchain")).ok()?;
    let channel = text.trim();
    (!channel.is_empty()).then(|| channel.to_string())
}

fn toolchain_dir(channel: &str) -> Option<PathBuf> {
    let home = env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".rustup")))
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".rustup")))?;

    fs::read_dir(home.join("toolchains"))
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            (name == channel || name.starts_with(&format!("{channel}-")))
                && path.join("bin").join(exe("cargo")).is_file()
        })
}

fn rustup_which(rustup: &Path, channel: &str, tool: &str) -> Option<PathBuf> {
    let output = Command::new(rustup)
        .args(["which", "--toolchain", channel, tool])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn rustup_exe() -> Option<PathBuf> {
    rustup_bin("rustup").or_else(|| resolve(OsStr::new("rustup")).map(|(path, _)| path))
}

fn active_toolchain(rustup: &Path, directory: &Path) -> Option<String> {
    let output = Command::new(rustup)
        .args(["show", "active-toolchain"])
        .env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(directory)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.split_whitespace().next().map(str::to_string)
}

fn cargo_bin() -> Option<PathBuf> {
    rustup_bin("cargo").or_else(|| resolve(OsStr::new("cargo")).map(|(path, _)| path))
}

pub fn install_rust() -> io::Result<()> {
    install("rustc")
}

pub fn install_cargo() -> io::Result<()> {
    install("cargo")
}

// rustup can add one component to an existing toolchain; a full install is the fallback.
fn install(component: &str) -> io::Result<()> {
    if version_of("rustup").is_some() {
        let status = Command::new("rustup")
            .args(["component", "add", component])
            .status()?;
        if status.success() && version_of(component).is_some() {
            return Ok(());
        }
    }
    rustup_init()
}

fn rustup_init() -> io::Result<()> {
    let status = if cfg!(windows) {
        install_windows()?
    } else {
        install_unix()?
    };

    if !status.success() {
        return Err(io::Error::other(format!(
            "rustup installer failed ({status})"
        )));
    }
    Ok(())
}

fn install_unix() -> io::Result<std::process::ExitStatus> {
    const SCRIPT: &str = r#"
set -e
if command -v curl >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile default
else
    wget -qO- https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile default
fi
"#;

    Command::new("sh").arg("-c").arg(SCRIPT).status()
}

fn install_windows() -> io::Result<std::process::ExitStatus> {
    let url = if cfg!(target_arch = "aarch64") {
        "https://win.rustup.rs/aarch64"
    } else {
        "https://win.rustup.rs/x86_64"
    };

    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'; \
         Invoke-WebRequest -Uri '{url}' -OutFile \"$env:TEMP\\rustup-init.exe\"; \
         & \"$env:TEMP\\rustup-init.exe\" -y --no-modify-path --profile default"
    );

    Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
}

/// Feature flags of one workspace member, as Cargo sees them.
pub struct Features {
    pub names: Vec<String>,
    pub defaults: Vec<String>,
}

pub fn features(directory: impl AsRef<Path>, package: &str) -> io::Result<Features> {
    let directory = directory.as_ref();
    let manifest = directory.join("Cargo.toml");
    let output = cargo_command(directory)?
        .env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(directory)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(&manifest)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "cargo metadata failed in {} ({})",
            directory.display(),
            output.status
        )));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|err| io::Error::other(format!("cargo metadata returned invalid JSON: {err}")))?;

    let features = json["packages"]
        .as_array()
        .and_then(|packages| {
            packages
                .iter()
                .find(|candidate| candidate["name"].as_str() == Some(package))
        })
        .and_then(|package| package["features"].as_object())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "package `{package}` has no features in {}",
                    manifest.display()
                ),
            )
        })?;

    let mut names: Vec<String> = features
        .keys()
        .filter(|name| *name != "default")
        .cloned()
        .collect();
    names.sort();

    let defaults = features
        .get("default")
        .and_then(|value| value.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(Features { names, defaults })
}

#[derive(Debug, Clone, Default)]
pub struct BuildOptions<'a> {
    pub features: &'a [&'a str],
    pub no_default_features: bool,
    pub package: Option<&'a str>,
    pub release: bool,
}

pub fn cargo_build(directory: impl AsRef<Path>, options: &BuildOptions<'_>) -> io::Result<PathBuf> {
    let directory = directory.as_ref();
    let manifest = directory.join("Cargo.toml");
    if !manifest.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no Cargo.toml in {}", directory.display()),
        ));
    }

    let mut command = cargo_command(directory)?;
    command
        .env_remove("RUSTUP_TOOLCHAIN")
        .current_dir(directory)
        .arg("build-native")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--release");
    if let Some(package) = options.package {
        command.args(["--package", package]);
    }
    if options.no_default_features {
        command.arg("--no-default-features");
    }
    if !options.features.is_empty() {
        command.arg("--features").arg(options.features.join(","));
    }

    let status = command.status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "cargo build failed in {} ({status})",
            directory.display()
        )));
    }

    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|dir| {
            if dir.is_relative() {
                directory.join(dir)
            } else {
                dir
            }
        })
        .unwrap_or_else(|| directory.join("target"));
    Ok(target.join(if options.release { "release" } else { "debug" }))
}
