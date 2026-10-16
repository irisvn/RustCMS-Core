#!/bin/sh
# Installs the `rustcms` command (macOS, Linux).
#
#   curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh
#   curl -fsSL https://irisvn.github.io/RustCMS/install.sh | sh -s -- --modify-path
#
# Environment:
#   RUSTCMS_VERSION        a release tag (v0.4.0, v0.4.0-rc.1); default: the latest release
#   RUSTCMS_INSTALL        install root (default ~/.rustcms; the command goes in its bin/)
#
# The archive is checked against the release's SHA256SUMS before it is opened.
# That detects a corrupted download, not a compromised release: read this
# script and the release page if that matters to you.
set -eu

REPO_URL="https://github.com/irisvn/RustCMS"
PAGES_URL="https://irisvn.github.io/RustCMS"
# Tests only: a local mirror of the release assets (file://…).
DOWNLOAD_BASE="${RUSTCMS_DOWNLOAD_BASE:-$REPO_URL/releases/download}"
LATEST_URL="${RUSTCMS_LATEST_URL:-$PAGES_URL/latest.txt}"
ROOT="${RUSTCMS_INSTALL:-$HOME/.rustcms}"
MODIFY_PATH=0

say() { printf 'rustcms-install: %s\n' "$*"; }
fail() { printf 'rustcms-install: error: %s\n' "$*" >&2; exit 1; }

for arg in "$@"; do
    case "$arg" in
        --modify-path) MODIFY_PATH=1 ;;
        -h|--help) sed -n '2,13p' "$0" 2>/dev/null || true; exit 0 ;;
        *) fail "unknown option: $arg" ;;
    esac
done

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

# HTTPS only (redirects too), unless a test mirror was given explicitly.
if [ -n "${RUSTCMS_DOWNLOAD_BASE:-}" ] || [ -n "${RUSTCMS_LATEST_URL:-}" ]; then
    PROTO='=https,file'
else
    PROTO='=https'
fi
fetch() {
    curl --proto "$PROTO" --proto-redir "$PROTO" --tlsv1.2 -fsSL --retry 3 --connect-timeout 10 --max-time 300 \
        -o "$2" "$1" || fail "download failed: $1"
}

case "$(uname -s)" in
    Darwin) os=apple-darwin ;;
    Linux) os=unknown-linux-musl ;;
    *) fail "unsupported system $(uname -s): RustCMS supports macOS and Linux (on Windows, use WSL)" ;;
esac
case "$(uname -m)" in
    arm64|aarch64) arch=aarch64 ;;
    x86_64|amd64) arch=x86_64 ;;
    *) fail "unsupported processor $(uname -m)" ;;
esac
# An x86_64 shell under Rosetta on Apple silicon: install the native binary.
if [ "$os" = apple-darwin ] && [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
    arch=aarch64
fi
target="$arch-$os"

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t rustcms)
trap 'rm -rf "$tmp"' EXIT INT TERM

tag="${RUSTCMS_VERSION:-}"
if [ -z "$tag" ]; then
    fetch "$LATEST_URL" "$tmp/latest.txt"
    tag=$(tr -d ' \r\n' < "$tmp/latest.txt")
fi
printf '%s' "$tag" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.(0|[1-9][0-9]*))?$' \
    || fail "not a release tag: '$tag' (no stable release yet? set RUSTCMS_VERSION=vX.Y.Z-rc.N)"

archive="rustcms-$tag-$target.tar.gz"
say "installing rustcms $tag ($target)"
fetch "$DOWNLOAD_BASE/$tag/$archive" "$tmp/$archive"
fetch "$DOWNLOAD_BASE/$tag/SHA256SUMS" "$tmp/SHA256SUMS"

# Exactly one line names the archive.
expected=$(awk -v f="$archive" '$2 == f || $2 == "*" f { print $1 }' "$tmp/SHA256SUMS")
if [ -z "$expected" ] || [ "$(printf '%s\n' "$expected" | wc -l | tr -d ' ')" != 1 ]; then
    fail "SHA256SUMS has no single entry for $archive"
fi
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$archive" | awk '{ print $1 }')
else
    actual=$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')
fi
[ "$actual" = "$expected" ] || fail "checksum mismatch for $archive: the download is corrupted"

# Only the command is taken from the archive.
mkdir "$tmp/out"
tar -xzf "$tmp/$archive" -C "$tmp/out" rustcms || fail "$archive has no rustcms executable"
chmod 755 "$tmp/out/rustcms"
"$tmp/out/rustcms" --version >/dev/null 2>&1 || fail "the downloaded rustcms does not run on this system"

# Next to the destination, then renamed: an existing install is kept if anything fails.
mkdir -p "$ROOT/bin"
staged="$ROOT/bin/.rustcms.$$"
cp "$tmp/out/rustcms" "$staged"
mv -f "$staged" "$ROOT/bin/rustcms"
printf '{"version":"%s","target":"%s","installer":"install.sh"}\n' "$tag" "$target" > "$ROOT/install.json"
say "installed $ROOT/bin/rustcms ($("$ROOT/bin/rustcms" --version))"

case ":$PATH:" in
    *":$ROOT/bin:"*) ;;
    *)
        line="export PATH=\"$ROOT/bin:\$PATH\""
        if [ "$MODIFY_PATH" = 1 ]; then
            case "${SHELL:-}" in
                */zsh) rc="$HOME/.zshrc" ;;
                */bash) rc="$HOME/.bashrc" ;;
                *) rc="$HOME/.profile" ;;
            esac
            grep -qsF "$line" "$rc" || printf '\n%s\n' "$line" >> "$rc"
            say "added $ROOT/bin to PATH in $rc (open a new terminal)"
        else
            say "add it to your PATH:  $line"
        fi
        ;;
esac

command -v cargo >/dev/null 2>&1 \
    || say "Rust is not installed: install it from https://rustup.rs, then run \`rustcms doctor\`"
say "next: rustcms new blog"
