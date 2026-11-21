#!/usr/bin/env bash
set -euo pipefail

# Build a simple Termux-compatible .deb from the release binary.
# Usage: ./scripts/build-termux-deb.sh [aarch64|arm|x86_64]
# Note: cross-compiling requires the target toolchain installed.

ARCH_INPUT="${1:-aarch64}"
case "$ARCH_INPUT" in
  aarch64|arm64) TERMUX_ARCH="aarch64"; TARGET="aarch64-linux-android" ;;
  arm|armv7) TERMUX_ARCH="arm"; TARGET="armv7-linux-androideabi" ;;
  x86_64) TERMUX_ARCH="x86_64"; TARGET="x86_64-linux-android" ;;
  *) echo "unsupported arch: $ARCH_INPUT"; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(grep '^version' Cargo.toml | head -n1 | cut -d'"' -f2)"
DIST="$ROOT/dist"
PKGDIR="$DIST/pkg"
rm -rf "$PKGDIR"
mkdir -p "$PKGDIR/DEBIAN" "$PKGDIR/data/data/com.termux/files/usr/bin" "$DIST"

# Build: if target installed, try cross build; otherwise build host and warn.
if rustup target list --installed | grep -q "^${TARGET}$"; then
  cargo build --release --target "$TARGET"
  BIN="target/${TARGET}/release/opencode-native"
else
  echo "warning: rust target $TARGET not installed; building host binary" >&2
  cargo build --release
  BIN="target/release/opencode-native"
fi

install -m755 "$BIN" "$PKGDIR/data/data/com.termux/files/usr/bin/opencode-native"
ln -sf opencode-native "$PKGDIR/data/data/com.termux/files/usr/bin/ocn"

cat > "$PKGDIR/DEBIAN/control" <<CTL
Package: opencode-native
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${TERMUX_ARCH}
Maintainer: OpenCode Native Contributors
Description: Lightweight native OpenCode-style coding agent
 A small Rust-based native agent for direct device use (Termux/Android/Linux).
CTL

dpkg-deb --build "$PKGDIR" "$DIST/opencode-native_${VERSION}_${TERMUX_ARCH}.deb"
echo "built: $DIST/opencode-native_${VERSION}_${TERMUX_ARCH}.deb"
