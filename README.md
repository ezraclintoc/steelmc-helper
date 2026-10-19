# SteelMC helper

An interactive CLI that installs [SteelMC](https://github.com/Steel-Foundation/SteelMC).

It has two install paths, both offered in the first prompt: download a release binary (seconds,
no Rust toolchain needed) or build from source for a faster runtime, a custom feature set, or a
pull request that has no release yet.

```console
$ cargo run --release
┌  SteelMC downloader
│
◆  How do you want to install?
│  ● Prebuilt binary (installs in seconds, slower at runtime, fixed features)
│  ○ Build from source (slow install, faster at runtime, pick features)
└
```

There are no arguments or config files; every choice is a prompt, and `Esc`/`Ctrl-C` cancels
cleanly. Everything is written into the directory you run it from.

## Quickstart

Install the tool itself, then run it — it prompts for everything else:

```console
$ curl -fsSL https://raw.githubusercontent.com/ezraclintoc/steelmc-helper/main/install.sh | sh
Installed ~/.local/bin/steelmc-helper.
Run it from any directory:

  steelmc-helper
$ steelmc-helper
```

The installer detects your OS and CPU, downloads the matching release asset, verifies its
SHA256 against the published checksum file, and installs to `~/.local/bin`. Flags: `--to <dir>`
(pick a different prefix), `--musl` (force the static Linux build), `--nightly` (rolling nightly
instead of the latest release). On Windows build from source with
`cargo install --git https://github.com/ezraclintoc/steelmc-helper` instead.

You already have Rust? `cargo install --git https://github.com/ezraclintoc/steelmc-helper` works
from any platform.

## Installing SteelMC

| Path | Pick it when |
| --- | --- |
| **Prebuilt binary** (preselected) | you just want to run SteelMC: seconds to install, no toolchain |
| Build from source | you want a build tuned to this machine, custom features, or an unreleased PR |

### Prebuilt binary

1. Choose the latest or previous release.
2. On x86_64 Linux, choose the glibc or the static musl build; other platforms have one asset.
3. The file keeps its release name (`steel-linux`, `steel-linux-musl`, `steel-macos`,
   `steel-windows.exe`), is marked executable on Unix, and the tool prints how to run it.

```console
$ ./steel-linux
```

This is a generic build produced by SteelMC's release workflow, so its features are the package
defaults and it is not tuned for this CPU. Building from source trades a long compile for exactly
that.

Before downloading or building over an existing file the tool asks; the default answer is **No**.

### Build from source

Use this when you want `-C target-cpu=native` style tuning, a custom feature set, or a PR.
Choosing it means:

1. `rustc` and `cargo` are checked first; a missing one gets one "Install it now?" prompt.
2. The source for the chosen tag or PR head is downloaded and unpacked into `.steel-src-<tag>/`.
3. Features are offered (see below), then `cargo build --release --package steel` runs.
4. The binary is copied to `./steel` and the scratch directory is removed. On failure it is
   kept so the cargo output can be inspected.

#### Pull requests

SteelMC's PR workflows only run `cargo test` and `clippy`, so there is no compiled artifact
attached to a PR — the tool builds the PR head itself and installs `./steel`. First it reads the
head SHA and looks up that commit's check runs:

| Checks | Behaviour |
| --- | --- |
| all completed and successful | builds |
| any failed | lists them, asks "Build anyway?" (default **No**) |
| still running | asks "Build anyway?" (default **Yes**) |
| none, 404, rate limit | warns that the build probably won't work, default **Yes** |

#### Feature flags

After the source is unpacked, the tool asks whether to customize features. The list comes from
`cargo metadata --no-deps` for the `steel` package, so it always matches the source being built;
for `v0.16.0+mc26.3` it is:

`deadlock_detection`, `dhat`, `dhat-heap`, `jaeger`, `mimalloc`, `slow_chunk_gen`, `stand-alone`

The package's `default` features are preselected. Accepting the defaults builds normally;
customizing builds with `--no-default-features --features <picked>`, so unchecking `mimalloc`
really does turn it off. `dhat-heap` is a profiling build and `deadlock_detection` starts a
watchdog thread, so selecting everything is rarely what you want.

## Requirements

- **Network access** to `github.com`, `api.github.com` and `codeload.github.com`.
- **Rust** only for source builds. If `rustc` or `cargo` is missing you get one "Install it now?"
  prompt per tool, which uses `rustup component add` and falls back to the official rustup
  installer with `--no-modify-path`. Because a fresh install cannot update the current process's
  `PATH`, the tool also looks in rustup's `bin` directory when checking whether the tools exist.
- No GitHub token: every API call is unauthenticated, so the 60 requests/hour per-IP limit is
  shared by the release listing and the PR check. Downloading tarballs and assets is not
  rate-limited the same way.

## Toolchain handling

The build does not just call `cargo` from `PATH`. SteelMC pins a channel in
`rust-toolchain.toml` (currently `nightly-2026-08-21`) and passes nightly-only `-Z` flags, so using
a stable compiler fails with an error saying those options are nightly-only.

For each source build the tool:

1. reads `channel` from the checkout's `rust-toolchain.toml` (or a legacy plain `rust-toolchain`);
2. finds the matching directory under `$RUSTUP_HOME/toolchains`;
3. runs that toolchain's `bin/cargo` with `RUSTC` pointed at its `bin/rustc`.

Pinning `RUSTC` matters: Cargo otherwise resolves `rustc` from `PATH`, so a stable toolchain
directory in front of rustup's proxy would still win. With both pinned, the build is unaffected by
`PATH` order or an exported `RUSTUP_TOOLCHAIN`.

If the pinned channel is not installed, the error says so instead of failing confusingly:

```
pinned toolchain `nightly-2026-08-21` is not installed (rustup toolchain install nightly-2026-08-21)
```

Before downloading anything the tool also probes what the linker needs: `cc`/`gcc`/`clang` on
Unix, MSVC's `link.exe` on Windows, and — additionally on Linux — `lld`, because SteelMC's config
links with `-Clink-arg=-fuse-ld=lld`. A missing piece is only a warning with a platform hint
(`nix-shell -p gcc lld` on NixOS), not a hard error.

## Files it writes

| Path | When |
| --- | --- |
| `./steel-linux`, `./steel-linux-musl`, `./steel-macos`, `./steel-windows.exe` | prebuilt download |
| `./steel` | source build |
| `.steel-src-<tag>/` | source build scratch space, removed on success and kept on failure |

Both `.steel-src-*/` and `target/` are gitignored. A failed build keeps its tree so you can
inspect `cargo`'s output or rerun there; the next attempt deletes and recreates it.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `... has no build for linux x86_64` | the release has no asset whose name starts with `steel-linux`; check the release page |
| PR build fails immediately with 404 | wrong PR number, or the PR is from a fork whose head is gone |
| ``the option `Z` is only accepted on the nightly compiler`` | a pinned toolchain was selected but its `rustc` was not used; reinstall the helper so `RUSTC` is pinned |
| `linker 'cc' not found` (or `'lld' not found`) | no C compiler/linker on `PATH`; on NixOS run the helper from `nix-shell -p gcc lld`, elsewhere install `build-essential` + `lld` (Debian/Ubuntu) or `gcc` + `lld` (Fedora/Alpine); the helper warns about all of these up front now |
| `Could not fetch releases` | offline, or the unauthenticated GitHub rate limit was hit |
| source build says `cargo finished but ... is missing` | the workspace layout changed and no binary named `steel` is produced |

## How it is organized

| Module | Job |
| --- | --- |
| [`src/main.rs`](src/main.rs) | every prompt, spinner, progress bar and message; orchestrates the two modes |
| [`src/fetch.rs`](src/fetch.rs) | release listing, OS asset matching, tarball downloads and extraction, PR CI lookup |
| [`src/build.rs`](src/build.rs) | find/install `cargo` and `rustc`, resolve the pinned toolchain, list features, run `cargo build` |

HTTP and TLS come from [`self_update`](https://crates.io/crates/self_update) (ureq + rustls);
`flate2`/`tar` unpack source tarballs; `serde_json` parses `cargo metadata` and the check-runs
response; UI is [`cliclack`](https://crates.io/crates/cliclack).

Two things in `self_update` 1.3.1 are worth knowing if you edit `fetch.rs`:

- `ReleaseAsset::download_url()` returns the **API** URL, which serves JSON metadata unless the
  request sends `Accept: application/octet-stream`. `fetch::download` sets that header.
- `asset_for()` matches on a target triple or arch+OS substring, but these assets are named
  `steel-linux` etc., so matching is hand-written.

## Development

```console
cargo build
cargo clippy --all-targets -- -D warnings
```

Both should be warning-free. The tool is tested on Linux; the Windows and macOS paths
(asset selection, `chmod` being a no-op, rustup resolution) have not been exercised.
