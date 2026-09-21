#!/usr/bin/env bash
# Start-up smoke test: launch the real binary on a throw-away X display and
# wait for the log line that says the sidebar is on screen.
#
#   scripts/smoke-linux.sh [path/to/ai-usage-sidebar]
#
# This is the one check that catches a start-up panic, a missing runtime
# library or a window that never gets revealed — none of which unit tests or
# browser tests can see. It needs `xvfb-run` and `dbus-run-session`; nothing is
# ever drawn on the developer's desktop, and a private XDG home keeps the real
# settings.json and usage.db untouched.
#
# Environment:
#   SMOKE_TIMEOUT   seconds to wait for the log line (default 60)
set -euo pipefail

APP_ID="io.github.harveyxiacn.ai-usage-sidebar"
MARKER="sidebar revealed"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/src-tauri/target/release/ai-usage-sidebar}"
TIMEOUT="${SMOKE_TIMEOUT:-60}"

[[ -x "$BINARY" ]] || { echo "No executable at $BINARY" >&2; exit 1; }
for tool in xvfb-run dbus-run-session; do
  command -v "$tool" >/dev/null || { echo "$tool is required (install xvfb / dbus-x11)" >&2; exit 1; }
done

WORK="$(mktemp -d)"
APP_PID=""

cleanup() {
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    # xvfb-run, dbus and the app share the process group we created with setsid.
    kill -TERM -- "-$APP_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$APP_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -KILL -- "-$APP_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

# A private XDG home: the app writes its settings, database and logs there.
export XDG_CONFIG_HOME="$WORK/config"
export XDG_DATA_HOME="$WORK/data"
export XDG_CACHE_HOME="$WORK/cache"
export HOME="$WORK/home"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$HOME"

# WebKitGTK's DMABUF renderer paints black or crashes on virtual/NVIDIA GPUs.
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export AI_USAGE_SIDEBAR_LOG=debug
export GDK_BACKEND=x11

LOG_DIR="$XDG_DATA_HOME/$APP_ID/logs"
STDOUT="$WORK/stdout.log"

echo "Launching $BINARY (timeout ${TIMEOUT}s)"
setsid xvfb-run -a dbus-run-session -- "$BINARY" >"$STDOUT" 2>&1 &
APP_PID=$!

found=0
for _ in $(seq 1 $((TIMEOUT * 4))); do
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "The app exited before it revealed the sidebar." >&2
    break
  fi
  if grep -qrs -- "$MARKER" "$LOG_DIR" 2>/dev/null; then
    found=1
    break
  fi
  sleep 0.25
done

if [[ "$found" -ne 1 ]]; then
  echo "Did not find \"$MARKER\" in $LOG_DIR within ${TIMEOUT}s." >&2
  echo "----- stdout/stderr -----" >&2
  cat "$STDOUT" >&2 || true
  echo "----- app log -----" >&2
  cat "$LOG_DIR"/* >&2 2>/dev/null || echo "(no log file was written)" >&2
  exit 1
fi

echo "Start-up looks healthy:"
grep -hs -e "platform setup" -e "tray menu ready" -e "$MARKER" "$LOG_DIR"/* || true
exit 0
