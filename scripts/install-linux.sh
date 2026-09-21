#!/usr/bin/env bash
# Per-user install on any Linux desktop (no root, no package manager):
#   scripts/install-linux.sh              build a release binary and install it
#   scripts/install-linux.sh --no-build   install the binary that is already built
#   scripts/install-linux.sh --uninstall  remove everything this script installed
# Settings and the usage database are never touched.
set -euo pipefail

APP_ID="ai-usage-sidebar"
APP_NAME="AI Usage Sidebar"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
BIN_DIR="$HOME/.local/bin"
DESKTOP_FILE="$DATA_HOME/applications/$APP_ID.desktop"
ICON_SIZES=(32 64 128 256 512)

refresh_caches() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA_HOME/applications" || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t -f "$DATA_HOME/icons/hicolor" 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$BIN_DIR/$APP_ID" "$DESKTOP_FILE"
  for size in "${ICON_SIZES[@]}"; do
    rm -f "$DATA_HOME/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
  done
  refresh_caches
  echo "Removed $APP_NAME. Settings and history were kept."
  exit 0
fi

if [[ "${1:-}" != "--no-build" ]]; then
  (cd "$ROOT" && pnpm install --frozen-lockfile && pnpm tauri build --no-bundle)
fi

BINARY="$ROOT/src-tauri/target/release/$APP_ID"
[[ -x "$BINARY" ]] || { echo "Release binary not found: $BINARY" >&2; exit 1; }

# Replace atomically so a running instance keeps its (unlinked) executable.
install -Dm755 "$BINARY" "$BIN_DIR/$APP_ID.new"
mv -f "$BIN_DIR/$APP_ID.new" "$BIN_DIR/$APP_ID"

icon_source() {
  case "$1" in
    32) echo "32x32.png" ;;
    64) echo "64x64.png" ;;
    128) echo "128x128.png" ;;
    256) echo "128x128@2x.png" ;;
    512) echo "icon.png" ;;
  esac
}
for size in "${ICON_SIZES[@]}"; do
  install -Dm644 "$ROOT/src-tauri/icons/$(icon_source "$size")" \
    "$DATA_HOME/icons/hicolor/${size}x${size}/apps/$APP_ID.png"
done

# StartupWMClass ties the dashboard window to this launcher, so the dock shows
# one icon instead of a second generic one. Launching again while the app runs
# focuses the dashboard (single-instance).
mkdir -p "$(dirname "$DESKTOP_FILE")"
cat >"$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Name[zh_CN]=AI 使用量侧边栏
Comment=Claude Code / Codex quota sidebar
Comment[zh_CN]=Claude Code / Codex 额度侧边栏
Exec=$BIN_DIR/$APP_ID
Icon=$APP_ID
Terminal=false
Categories=Utility;
StartupNotify=true
StartupWMClass=$APP_ID
EOF
chmod 644 "$DESKTOP_FILE"
command -v desktop-file-validate >/dev/null && desktop-file-validate "$DESKTOP_FILE"
refresh_caches

echo "Installed $APP_NAME:"
echo "  $BIN_DIR/$APP_ID"
echo "  $DESKTOP_FILE"
case ":$PATH:" in *":$BIN_DIR:"*) ;; *) echo "Note: $BIN_DIR is not on your PATH (the launcher still works)." ;; esac
