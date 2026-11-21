#!/usr/bin/env sh
set -eu

# OpenCode Native one-file installer/bootstrap.
# Usage:
#   sh install.sh
#   curl -fsSL https://raw.githubusercontent.com/PeyzaKuprey/opencode-native/master/install.sh | sh

REPO="${OCN_REPO:-PeyzaKuprey/opencode-native}"
VERSION="${OCN_VERSION:-latest}"
INSTALL_DIR="${OCN_INSTALL_DIR:-}"

log() { printf '%s\n' "$*"; }
err() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() { command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"; }

download() {
  url="$1"; out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --connect-timeout 15 --max-time 300 --retry 3 --retry-delay 2 -o "$out" "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget --timeout=30 --tries=3 -O "$out" "$url"
  else
    err "need curl or wget"
  fi
}

uname_s=$(uname -s 2>/dev/null || echo unknown)
uname_m=$(uname -m 2>/dev/null || echo unknown)
uname_o=$(uname -o 2>/dev/null || echo unknown)

is_termux=0
case "${PREFIX:-}" in
  *com.termux*) is_termux=1 ;;
esac
case "$uname_o" in
  Android) is_termux=1 ;;
esac

case "$uname_s" in
  Linux) os="linux" ;;
  Android) os="android"; is_termux=1 ;;
  *) err "unsupported OS: $uname_s (supported: Linux/Termux Android)" ;;
esac

case "$uname_m" in
  x86_64|amd64) arch="x86_64" ;;
  aarch64|arm64) arch="aarch64" ;;
  *) err "unsupported arch: $uname_m (supported: x86_64, aarch64)" ;;
esac

if [ "$is_termux" = "1" ]; then
  asset="opencode-native-android-${arch}"
  default_dir="${PREFIX:-/data/data/com.termux/files/usr}/bin"
else
  if [ "$os" != "linux" ] || [ "$arch" != "x86_64" ]; then
    err "host Linux build currently provided for x86_64; for Termux use Android build"
  fi
  asset="opencode-native-linux-x86_64"
  default_dir="${HOME}/.local/bin"
fi

INSTALL_DIR="${INSTALL_DIR:-$default_dir}"
mkdir -p "$INSTALL_DIR"

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${VERSION}"
fi
url="${base}/${asset}"

tmp="${TMPDIR:-/tmp}/opencode-native.$$"
trap 'rm -f "$tmp"' EXIT INT TERM

log "OpenCode Native installer"
log "  repo:    $REPO"
log "  version: $VERSION"
log "  target:  $os/$arch termux=$is_termux"
log "  asset:   $asset"
log "  install: $INSTALL_DIR/opencode-native"
log "  url:     $url"

download "$url" "$tmp"
chmod 755 "$tmp"
# install
dest="$INSTALL_DIR/opencode-native"
mv "$tmp" "$dest"
trap - EXIT INT TERM
ln -sf "opencode-native" "$INSTALL_DIR/ocn" 2>/dev/null || true

# config template
cfg_home="${XDG_CONFIG_HOME:-$HOME/.config}"
cfg="$cfg_home/opencode-native/config.toml"
if [ ! -f "$cfg" ]; then
  mkdir -p "$(dirname "$cfg")"
  cat > "$cfg" <<'CFG'
[model]
provider = "openai-compatible"
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4.1-mini"

[agent]
system_prompt = "You are OpenCode Native, a lightweight coding agent running directly on the user's device. Be concise, safe, and helpful. Use tools when needed."
max_steps = 20
CFG
  log "  config:  created $cfg"
else
  log "  config:  exists $cfg"
fi

log ""
log "Installed: $dest"
"$dest" --version || true
log ""
log "Next:"
log "  export OPENAI_API_KEY=...   # or set api_key_env in config"
log "  opencode-native run \"hello\""
case ":$PATH:" in
  *:"$INSTALL_DIR":*) ;;
  *) log "  add to PATH: export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
