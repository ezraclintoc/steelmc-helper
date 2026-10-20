#!/bin/sh
# steelmc-helper installer.
#
#   curl -fsSL https://raw.githubusercontent.com/ezraclintoc/steelmc-helper/main/install.sh | sh
#
# Flags: --to DIR (install prefix, default ~/.local/bin), --nightly (nightly
# release instead of the latest stable one).
# Override STEELMC_HELPER_BASE to install from a non-default location.
set -eu

REPO="ezraclintoc/steelmc-helper"
BIN="steelmc-helper"
TO="$HOME/.local/bin"
REF="latest"

help() {
    sed -n '2,8p' "$0"
    exit 0
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --to) TO="$2"; shift 2 ;;
        --to=*) TO="${1#*=}"; shift ;;
        --nightly) REF="nightly"; shift ;;
        -h|--help) help ;;
        *) printf >&2 'unknown flag: %s\n' "$1"; exit 64 ;;
    esac
done

# --- what build do we need? --------------------------------------------------

case "$(uname -s)" in
    Linux | Darwin) ;;
    *) # shellcheck disable=SC2016
        printf >&2 'install.sh supports Linux and macOS; on Windows run `cargo install --git https://github.com/%s` instead.\n' "$REPO"; exit 1 ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64 | amd64) ARCH="x86_64" ;;
    aarch64 | arm64) ARCH="aarch64" ;;
    *) printf >&2 'unsupported architecture: %s\n' "$ARCH"; exit 1 ;;
esac

ASSET="$BIN-$ARCH-unknown-linux-gnu"
case "$(uname -s)" in
    Darwin)
        # Releases ship an Apple-silicon build only; Intel macs use cargo.
        if [ "$ARCH" != "aarch64" ]; then
            # shellcheck disable=SC2016
            printf >&2 'no release asset for Intel macs; run `cargo install --git https://github.com/%s` instead.\n' "$REPO"; exit 1
        fi
        ASSET="$BIN-aarch64-apple-darwin"
        ;;
esac

# --- fetch -------------------------------------------------------------------

BASE="${STEELMC_HELPER_BASE:-https://github.com/$REPO/releases}"
URL="$BASE/$REF/download/$ASSET.tar.gz"

# The default source is HTTPS-only. A STEELMC_HELPER_BASE override exists for
# mirrors and locally staged test layouts, which may be plain http. POSIX sh
# has no arrays, so the two curl flavors become one helper used twice.
fetch() {
    # fetch <url> <dest>
    if [ -n "${STEELMC_HELPER_BASE:-}" ]; then
        curl -fsSL -o "$2" "$1" || { printf >&2 'download failed: %s\n' "$1"; exit 1; }
    else
        curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1" \
            || { printf >&2 'download failed: %s\n' "$1"; exit 1; }
    fi
}

TMPDIR_INSTALL="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_INSTALL"' EXIT

printf 'Downloading %s -> %s/steelmc-helper\n' "$URL" "$TO"
mkdir -p "$TO"
fetch "$URL" "$TMPDIR_INSTALL/$ASSET.tar.gz"
fetch "$URL.sha256" "$TMPDIR_INSTALL/$ASSET.tar.gz.sha256"

(cd "$TMPDIR_INSTALL" &&
 {
     if command -v sha256sum >/dev/null 2>&1; then
         SUM="$(cut -d' ' -f1 < "$ASSET.tar.gz.sha256")"
         [ "$(sha256sum < "$ASSET.tar.gz" | cut -d' ' -f1)" = "$SUM" ]
     elif command -v shasum >/dev/null 2>&1; then
         SUM="$(cut -d' ' -f1 < "$ASSET.tar.gz.sha256")"
         [ "$(shasum -a 256 < "$ASSET.tar.gz" | cut -d' ' -f1)" = "$SUM" ]
     else
         echo >&2 'neither sha256sum nor shasum found; skipping checksum verification'
         exit 0
     fi
 } ) || { printf >&2 'checksum mismatch; aborting\n'; exit 65; }

tar -xzf "$TMPDIR_INSTALL/$ASSET.tar.gz" -C "$TMPDIR_INSTALL"
[ -f "$TMPDIR_INSTALL/$BIN" ] || { printf >&2 'archive did not contain %s\n' "$BIN"; exit 1; }

if [ -f "$TO/$BIN" ] && [ -t 0 ]; then
    printf '%s/steelmc-helper exists; overwrite? [y/N] ' "$TO"
    read -r reply
    case "$reply" in
        [yY]*) ;;
        *) printf >&2 'not overwritten; exiting\n'; exit 1 ;;
    esac
fi

cat "$TMPDIR_INSTALL/$BIN" > "$TO/$BIN"
chmod +x "$TO/$BIN"

# --- what's next? ------------------------------------------------------------

case ":$PATH:" in
    *":$TO:"*) ;;
    *) # shellcheck disable=SC2016
        printf '\nWARNING: %s is not on PATH. Add this to your shell profile:\n\n  export PATH="%s:$PATH"\n\n' "$TO" "$TO" ;;
esac

printf '\nInstalled %s/%s.\nRun it from any directory:\n\n  %s\n' "$TO" "$BIN" "$BIN"
