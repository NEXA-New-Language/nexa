#!/bin/sh
# Install the NEXA compiler.
#
#   ./install.sh                 install from this checkout (if it is the
#                                NEXA source tree) or from GitHub (main)
#   ./install.sh --branch NAME   install from a branch, e.g. feature/ui-1.0
#   NEXA_BRANCH=NAME ./install.sh
#
# Requires a Rust toolchain (cargo) and a C compiler (cc). The binary is
# installed with `cargo install` into ~/.cargo/bin (or $CARGO_HOME/bin).
set -eu

REPO="https://github.com/NEXA-New-Language/nexa"
BRANCH="${NEXA_BRANCH:-main}"

while [ $# -gt 0 ]; do
    case "$1" in
        --branch)
            BRANCH="$2"
            shift 2
            ;;
        --branch=*)
            BRANCH="${1#--branch=}"
            shift
            ;;
        -h|--help)
            echo "Usage: ./install.sh [--branch NAME]"
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            exit 1
            ;;
    esac
done

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
    echo "Installing NEXA from this checkout..."
    cargo install --path . --locked
else
    echo "Installing NEXA from $REPO ($BRANCH)..."
    cargo install --git "$REPO" --branch "$BRANCH" --locked nexa
fi

echo ""
nexa --version
echo ""
echo "Installed. If 'nexa' is not on your PATH, add this to your shell profile:"
echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
echo ""
echo "Try it: nexa new hello && cd hello && nexa run"
