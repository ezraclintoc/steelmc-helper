#!/bin/sh
# steelmc-helper one-shot launcher.
#
#   curl -fsSL https://raw.githubusercontent.com/ezraclintoc/steelmc-helper/main/install.sh | sh
#
# Downloads the latest release binary into a temporary directory, runs it
# once, then deletes it. Nothing is installed. For a persistent install:
#   cargo install --git https://github.com/ezraclintoc/steelmc-helper
#
# Flag: --nightly (rolling nightly release instead of the latest stable one).
# Override STEELMC_HELPER_BASE to download from a non-default location.
set -eu

REPO="ezraclintoc/steelmc-helper"
BIN="steelmc-helper"
REF="latest"

help() {
    sed -n '2,9p' "$0"
    exit 0
}

while [ "$#" -gt 0 ]; do
    case "$1" in
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

# --- download, verify, unpack ------------------------------------------------

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

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

printf 'Downloading steelmc-helper (%s)\n' "$URL"
fetch "$URL" "$WORK/$ASSET.tar.gz"
fetch "$URL.sha256" "$WORK/$ASSET.tar.gz.sha256"

(cd "$WORK" &&
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

tar -xzf "$WORK/$ASSET.tar.gz" -C "$WORK"
[ -f "$WORK/$BIN" ] || { printf >&2 'archive did not contain %s\n' "$BIN"; exit 1; }
chmod +x "$WORK/$BIN"

# --- run it ------------------------------------------------------------------
#
# The helper talks to a terminal. Detect one with /dev/tty, not fd 0: under
# `curl | sh` fd 0 is the pipe carrying this script even though the user's
# terminal is open. The probe must be a subshell (a failing redirect-only exec
# is fatal in some shells) and the is-a-tty check must happen inside it, where
# fd 3 actually exists.
if (exec 3</dev/tty && [ -t 3 ]) 2>/dev/null; then
    exec 3</dev/tty
    # fd 0 belongs to curl, so hand the helper the terminal directly. The
    # helper may exit non-zero (Esc cancels as exit 1); `||` keeps the status.
    STATUS=0
    "$WORK/$BIN" <&3 >&2 2>&2 || STATUS=$?
    exec 3<&-
    exit "$STATUS"
fi

# No terminal (CI, curl | sh in a bare shell): the helper's UI needs one.
printf >&2 'No terminal detected; steelmc-helper needs an interactive shell.\n'
printf >&2 'Install it permanently instead:\n'
# shellcheck disable=SC2016
printf >&2 '  cargo install --git https://github.com/%s\n' "$REPO"
exit 1
