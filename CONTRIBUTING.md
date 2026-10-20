# Contributing to steelmc-helper

Thanks for helping! This file is the developer map. If you are here to *use* the tool,
the [README](README.md) has the quickstart and everything user-facing.

```
cargo build
cargo clippy --all-targets -- -D warnings
```

Both should be warning-free. The tool is tested on Linux; the Windows and macOS paths
(asset selection, `chmod` being a no-op, rustup resolution) have not been exercised.

Commits run a [prek](https://github.com/j178/prek) pre-commit chain (`prek.toml`): fmt,
clippy with CI parity flags, tests, typos, and whitespace guards. Install it with
`prek install` after cloning.

## How it is organized

| Module | Job |
| --- | --- |
| [`src/main.rs`](src/main.rs) | every prompt, spinner, progress bar and message; orchestrates the two modes |
| [`src/fetch.rs`](src/fetch.rs) | release listing, OS asset matching, tarball downloads and extraction, PR CI lookup |
| [`src/build.rs`](src/build.rs) | find/install `cargo` and `rustc`, resolve the pinned toolchain, list features, run `cargo build` |

The boundary is: `main.rs` is the only module that talks to the user (`cliclack`), and the
only module that does not do network or toolchain work. New prompts go in `main.rs`; new
HTTP calls go in `fetch.rs`; anything that shells out to `cargo`/`rustc`/`rustup` goes in
`build.rs`.

## Dependencies

HTTP and TLS come from [`self_update`](https://crates.io/crates/self_update) (ureq + rustls);
`flate2`/`tar` unpack source tarballs; `serde_json` parses `cargo metadata` and the check-runs
response; UI is [`cliclack`](https://crates.io/crates/cliclack).

Two things in `self_update` 1.3.1 are worth knowing if you edit `fetch.rs`:

- `ReleaseAsset::download_url()` returns the **API** URL, which serves JSON metadata unless the
  request sends `Accept: application/octet-stream`. `fetch::download` sets that header.
- `asset_for()` matches on a target triple or arch+OS substring, but these assets are named
  `steel-linux` etc., so matching is hand-written.

## Sizing the release binary

Users download the helper on every install, so `Cargo.toml` zeroes in on size:

`opt-level = "z"`, `lto = "fat"` via `lto = true`, `codegen-units = 1`,
`strip = "symbols"`, `panic = "abort"`.

If you add a dependency, check what it does to `target/release/steelmc-helper` before
committing — the v0.1.0 binary is ~2.3 MiB, and it got there by not being careless.
