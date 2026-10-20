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

# Offer to run the helper right away when the user is at a terminal. The
# interactive check must use /dev/tty, not [ -t 0 ]: under `curl | sh` fd 0 is
# the pipe carrying this script even though the user's terminal is open.
# Test interactivity without an exec in a compound command (a failing
# redirect-only exec is fatal in some shells, and an if-condition redirect is
# scoped to the condition): open a probe fd in a subshell, then the real fd 3
# inside the body.
CAN_TTY=0
# fd 3 exists only inside the subshell, so the is-a-tty check must happen
# there: the subshell exits true only when /dev/tty opens AND is a tty.
if (exec 3</dev/tty && [ -t 3 ]) 2>/dev/null; then
    CAN_TTY=1
fi
if [ "$CAN_TTY" = 1 ]; then
    exec 3</dev/tty
    # prompt <text> <reply-var>: read from the user's terminal, not the pipe
    prompt() {
        printf '%s' "$1"
        # shellcheck disable=SC2229
        read -r "$2" <&3
    }

    prompt 'Start it now? [Y/n] ' REPLY_START || exit 0
    case "$REPLY_START" in
        [nN]*) exit 0 ;;
    esac
    case ":$PATH:" in
        *":$TO:"*) ;;
        *) PATH="$TO:$PATH" ;;
    esac
    # fd 0 belongs to curl, so hand the helper the terminal directly.
    # The helper may exit non-zero (Esc cancels as exit 1); capture without
    # tripping set -e.
    HELPER_STATUS=0
    "$TO/$BIN" <&3 >&2 2>&2 || HELPER_STATUS=$?

    prompt 'Keep steelmc-helper installed? [Y/n] ' REPLY_KEEP || exit 0
    case "$REPLY_KEEP" in
        [nN]*) rm -f "$TO/$BIN" && printf 'Removed %s/%s.\n' "$TO" "$BIN" ;;
        *) printf 'Keeping %s/%s.\n' "$TO" "$BIN" ;;
    esac
    exit "$HELPER_STATUS"
fi
