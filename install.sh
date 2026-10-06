#!/usr/bin/env bash
# Neothesia (juice) installer: builds the FX fork from source and installs it
# user-locally as `neothesia-juice`, side by side with any stock Neothesia.
#
#   ./install.sh              install or update
#   ./install.sh --uninstall  remove binary, desktop entry, icon (keeps source)
#
# One-liner on a fresh box:
#   curl -fsSL https://raw.githubusercontent.com/bosman-solutions/Neothesia/juice/install.sh | bash
#
# Env overrides: NEOTHESIA_REPO, NEOTHESIA_BRANCH, NEOTHESIA_SRC
set -euo pipefail

REPO="${NEOTHESIA_REPO:-https://github.com/bosman-solutions/Neothesia.git}"
BRANCH="${NEOTHESIA_BRANCH:-juice}"
SRC="${NEOTHESIA_SRC:-$HOME/.local/src/neothesia-juice}"

NAME="neothesia-juice"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"

say() { printf '\033[1;33m🎹 %s\033[0m\n' "$*"; }
die() { printf '\033[1;31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

uninstall() {
    rm -fv "$BIN_DIR/$NAME" "$APP_DIR/$NAME.desktop" "$ICON_DIR/$NAME.png"
    say "Removed. Source left at $SRC (rm -rf it if you want it gone)."
    exit 0
}
[[ "${1:-}" == "--uninstall" ]] && uninstall

# ---- deps -------------------------------------------------------------------
if command -v pacman >/dev/null; then
    pkgs=(git pkgconf alsa-lib wayland libxkbcommon vulkan-icd-loader)
    command -v cargo >/dev/null || pkgs+=(rust)
    missing=()
    for p in "${pkgs[@]}"; do pacman -Qq "$p" >/dev/null 2>&1 || missing+=("$p"); done
    if ((${#missing[@]})); then
        say "Installing deps: ${missing[*]}"
        sudo pacman -S --needed --noconfirm "${missing[@]}"
    fi
else
    say "Not Arch: make sure git, cargo (1.85+), pkg-config, alsa, wayland,"
    say "libxkbcommon and a Vulkan loader are installed."
fi
command -v cargo >/dev/null || die "cargo not found"

# ---- source -----------------------------------------------------------------
if [[ -d "$SRC/.git" ]]; then
    say "Updating $SRC ($BRANCH)"
    git -C "$SRC" fetch --depth 1 origin "$BRANCH"
    git -C "$SRC" checkout -q -B "$BRANCH" FETCH_HEAD
else
    say "Cloning $REPO ($BRANCH) -> $SRC"
    mkdir -p "$(dirname "$SRC")"
    git clone --depth 1 --branch "$BRANCH" "$REPO" "$SRC"
fi

# ---- build ------------------------------------------------------------------
say "Building (first build takes a few minutes)"
cargo build --release --locked -p neothesia --manifest-path "$SRC/Cargo.toml"

# ---- install ----------------------------------------------------------------
mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"
install -m755 "$SRC/target/release/neothesia" "$BIN_DIR/$NAME"
install -m644 "$SRC/flatpak/com.github.polymeilex.neothesia.png" "$ICON_DIR/$NAME.png"
cat > "$APP_DIR/$NAME.desktop" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Neothesia (juice)
Comment=MIDI visualizer with glitter, embers and glass notes
Categories=Game;Music;
Icon=$NAME
Exec=$BIN_DIR/$NAME %f
MimeType=audio/midi;audio/x-midi;
Terminal=false
StartupNotify=false
EOF
command -v update-desktop-database >/dev/null && update-desktop-database -q "$APP_DIR" || true

rev=$(git -C "$SRC" rev-parse --short HEAD)
say "Installed $NAME @ $rev"
[[ ":$PATH:" == *":$BIN_DIR:"* ]] || say "Note: $BIN_DIR is not on your PATH"
say "Run: $NAME song.mid   (or launch 'Neothesia (juice)' from your app menu)"
