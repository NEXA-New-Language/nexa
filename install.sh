#!/bin/sh
# Install the NEXA compiler.
#
#   ./install.sh                 download the latest prebuilt binary
#                                (Linux x86-64) and install it into
#                                ~/.cargo/bin; builds from source if no
#                                prebuilt binary is available yet
#   ./install.sh --from-source   build and install from source instead
#   ./install.sh --branch NAME   install from a branch (source build)
#   ./install.sh --version 1.2.0 download that release instead of latest
#   NEXA_BRANCH=NAME ./install.sh
#   NEXA_VERSION=1.2.0 ./install.sh
#   NEXA_RELEASE_URL=<url> ./install.sh   (override the download URL)
#
# The prebuilt path needs curl and tar; the source path needs cargo and
# a C compiler (cc). Checksums of prebuilt binaries are always verified.
set -eu

REPO="https://github.com/NEXA-New-Language/nexa"
API="https://api.github.com/repos/NEXA-New-Language/nexa"
BRANCH="${NEXA_BRANCH:-main}"
VERSION="${NEXA_VERSION:-}"
FROM_SOURCE=0
RELEASE_URL="${NEXA_RELEASE_URL:-}"

while [ $# -gt 0 ]; do
    case "$1" in
        --from-source)
            FROM_SOURCE=1
            shift
            ;;
        --branch)
            BRANCH="$2"
            FROM_SOURCE=1
            shift 2
            ;;
        --branch=*)
            BRANCH="${1#--branch=}"
            FROM_SOURCE=1
            shift
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#--version=}"
            shift
            ;;
        -h|--help)
            echo "Usage: ./install.sh [--from-source] [--branch NAME] [--version X.Y.Z]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

BIN_DIR="${CARGO_HOME:-$HOME/.cargo}/bin"

install_from_source() {
    if ! command -v cargo >/dev/null 2>&1; then
        echo "cargo not found. Install Rust first: https://rustup.rs" >&2
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
        exit 1
    fi
    if ! command -v cc >/dev/null 2>&1; then
        echo "A C compiler (cc) is required to link NEXA programs." >&2
        echo "  Ubuntu/Debian: sudo apt install build-essential" >&2
        exit 1
    fi
    if [ -f ./Cargo.toml ] && grep -q '^name = "nexa"$' ./Cargo.toml; then
        echo "Building NEXA from this checkout..."
        cargo install --path . --locked
    else
        echo "Building NEXA from $REPO ($BRANCH)..."
        cargo install --git "$REPO" --branch "$BRANCH" --locked nexa
    fi
}

install_from_binary() {
    if ! command -v curl >/dev/null 2>&1; then
        echo "curl is required to download the prebuilt binary." >&2
        return 1
    fi
    if ! command -v tar >/dev/null 2>&1; then
        echo "tar is required to unpack the prebuilt binary." >&2
        return 1
    fi
    if [ -z "$VERSION" ]; then
        echo "Looking up the latest NEXA release..."
        TAG=$(curl -fsSL "$API/releases/latest" | grep -m 1 '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
        if [ -z "$TAG" ]; then
            echo "Could not find a published release." >&2
            return 1
        fi
        VERSION="${TAG#v}"
    fi
    TARBALL="nexa-${VERSION}-x86_64-linux.tar.gz"
    if [ -n "$RELEASE_URL" ]; then
        URL="$RELEASE_URL"
    else
        URL="$REPO/releases/download/v${VERSION}/${TARBALL}"
    fi
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT
    echo "Downloading NEXA $VERSION (prebuilt binary)..."
    if ! curl -fsSL -o "$TMPDIR/$TARBALL" "$URL"; then
        echo "Download failed: $URL" >&2
        return 1
    fi
    if ! curl -fsSL -o "$TMPDIR/$TARBALL.sha256" "$URL.sha256"; then
        echo "Checksum file is missing for this release." >&2
        return 1
    fi
    echo "Verifying checksum..."
    (cd "$TMPDIR" && sha256sum -c "$TARBALL.sha256") || {
        echo "Checksum verification failed. Aborting." >&2
        return 1
    }
    tar xzf "$TMPDIR/$TARBALL" -C "$TMPDIR"
    mkdir -p "$BIN_DIR"
    cp "$TMPDIR/nexa-${VERSION}-x86_64-linux/nexa" "$BIN_DIR/nexa"
    chmod +x "$BIN_DIR/nexa"
    echo "Installed prebuilt NEXA $VERSION to $BIN_DIR/nexa"
}

if [ "$FROM_SOURCE" -eq 1 ]; then
    install_from_source
elif ! install_from_binary; then
    echo ""
    echo "No prebuilt binary available; building from source instead..."
    install_from_source
fi

echo ""
"$BIN_DIR/nexa" --version
echo ""
echo "Installed. If 'nexa' is not on your PATH, add this to your shell profile:"
echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
echo ""
echo "Try it: nexa new hello && cd hello && nexa run"
