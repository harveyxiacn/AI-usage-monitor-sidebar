#!/usr/bin/env bash
# Start-up smoke test (and optional upgrade smoke test): launch the real binary
# on a throw-away X display and wait for the log line that says the sidebar is
# on screen.
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
#   SMOKE_TIMEOUT          seconds to wait for each log line (default 60)
#   SMOKE_UPGRADE_FROM     path of an OLDER release AppImage. When set it is run
#                          first against the private profile, stopped, a
#                          hand-written settings.json (UTF-8 BOM, partial) is
#                          seeded, and only then the new binary is started
#                          against the same profile.
#   SMOKE_EXPECT_MIGRATION old usage.db schema version the new build must log
#                          "usage.db schema <old> -> <new>" for (optional)
set -euo pipefail

APP_ID="io.github.harveyxiacn.ai-usage-sidebar"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BINARY="${1:-$ROOT/src-tauri/target/release/ai-usage-sidebar}"
TIMEOUT="${SMOKE_TIMEOUT:-60}"
UPGRADE_FROM="${SMOKE_UPGRADE_FROM:-}"
EXPECT_MIGRATION="${SMOKE_EXPECT_MIGRATION:-}"

[[ -x "$BINARY" ]] || { echo "No executable at $BINARY" >&2; exit 1; }
[[ -z "$UPGRADE_FROM" || -x "$UPGRADE_FROM" ]] || { echo "No executable at $UPGRADE_FROM" >&2; exit 1; }
for tool in xvfb-run dbus-run-session; do
  command -v "$tool" >/dev/null || { echo "$tool is required (install xvfb / dbus-x11)" >&2; exit 1; }
done

WORK="$(mktemp -d)"
APP_PID=""

stop_app() {
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    # xvfb-run, dbus and the app share the process group we created with setsid.
    kill -TERM -- "-$APP_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do
      kill -0 "$APP_PID" 2>/dev/null || break
      sleep 0.25
    done
    kill -KILL -- "-$APP_PID" 2>/dev/null || true
  fi
  APP_PID=""
}
cleanup() { stop_app; rm -rf "$WORK"; }
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
# AppImages need FUSE otherwise, which CI runners do not have.
export APPIMAGE_EXTRACT_AND_RUN=1

LOG_DIR="$XDG_DATA_HOME/$APP_ID/logs"
SETTINGS="$XDG_CONFIG_HOME/$APP_ID/settings.json"

dump_logs() {
  echo "----- stdout/stderr -----" >&2
  cat "$WORK/stdout.log" >&2 || true
  echo "----- app log -----" >&2
  cat "$LOG_DIR"/* >&2 2>/dev/null || echo "(no log file was written)" >&2
}

# fail <message>: print diagnostics, keep a copy of the logs for the CI artifact.
fail() {
  echo "SMOKE FAILED: $*" >&2
  dump_logs
  if [[ -n "${SMOKE_LOG_COPY:-}" ]]; then
    mkdir -p "$SMOKE_LOG_COPY"
    cp -r "$LOG_DIR"/. "$SMOKE_LOG_COPY"/ 2>/dev/null || true
    cp "$WORK/stdout.log" "$SMOKE_LOG_COPY"/ 2>/dev/null || true
  fi
  exit 1
}

# start_phase <label> <binary>: launch, wait for the marker, check the log.
start_phase() {
  local label="$1" bin="$2"
  echo "== $label: launching $bin (timeout ${TIMEOUT}s)"
  setsid xvfb-run -a dbus-run-session -- "$bin" >"$WORK/stdout.log" 2>&1 &
  APP_PID=$!
  local found=0
  for _ in $(seq 1 $((TIMEOUT * 4))); do
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      fail "$label: the app exited before it revealed the sidebar"
    fi
    if grep -qrs -- "sidebar revealed" "$LOG_DIR" 2>/dev/null; then found=1; break; fi
    sleep 0.25
  done
  [[ "$found" -eq 1 ]] || fail "$label: no \"sidebar revealed\" in $LOG_DIR within ${TIMEOUT}s"
  grep -qrs -- "platform setup:" "$LOG_DIR" || fail "$label: no \"platform setup:\" line"
  if grep -Ehrs -- 'panicked|\[ERROR\]' "$LOG_DIR" "$WORK/stdout.log"; then
    fail "$label: the log holds a panic or an ERROR line (shown above)"
  fi
  echo "$label: start-up looks healthy:"
  grep -hs -e "platform setup" -e "tray menu ready" -e "sidebar revealed" -e "usage.db schema" "$LOG_DIR"/* || true
}

if [[ -n "$UPGRADE_FROM" ]]; then
  start_phase "previous release" "$UPGRADE_FROM"
  stop_app
  # Keep the old run's log apart so the new run's assertions read only its own.
  mv "$LOG_DIR" "$WORK/logs-previous"
  # What a user's hand-edited file looked like in the two v0.6 bugs: a UTF-8
  # BOM in front of a partial key set.
  mkdir -p "$(dirname "$SETTINGS")"
  printf '\xef\xbb\xbf{ "edge": "left", "autoHide": true, "language": "zh-CN" }\n' >"$SETTINGS"
fi

start_phase "this build" "$BINARY"

if [[ -n "$UPGRADE_FROM" ]]; then
  grep -rqs -- "platform setup:.*edge=Left.*autoHide=true" "$LOG_DIR" \
    || fail "the BOM-prefixed settings.json was not applied (expected edge=Left, autoHide=true)"
  echo "BOM + partial settings.json applied."
  if [[ -n "$EXPECT_MIGRATION" ]]; then
    grep -rqs -- "usage.db schema $EXPECT_MIGRATION -> " "$LOG_DIR" \
      || fail "expected the log line \"usage.db schema $EXPECT_MIGRATION -> N\""
    echo "Database migration from schema $EXPECT_MIGRATION logged."
  fi
fi
echo "Smoke test passed."
exit 0
