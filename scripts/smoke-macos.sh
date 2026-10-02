#!/usr/bin/env bash
# macOS start-up smoke test (and optional upgrade smoke test).
#
#   scripts/smoke-macos.sh path/to/"AI Usage Sidebar.app"
#
# Launches the bundle's executable and waits for `sidebar revealed` in
# ~/Library/Logs/io.github.harveyxiacn.ai-usage-sidebar/. Meant for a CI runner
# (it uses the real home directory); on a developer machine set
# SMOKE_ALLOW_REAL_PROFILE=1 to acknowledge that your own settings.json and
# logs are replaced in upgrade mode.
#
# Environment:
#   SMOKE_TIMEOUT          seconds to wait for each log line (default 90)
#   SMOKE_UPGRADE_FROM     path of an OLDER release .dmg: copied out, run
#                          first, stopped, a hand-written BOM settings.json is
#                          seeded, then the new bundle starts on the same profile
#   SMOKE_EXPECT_MIGRATION old usage.db schema the new build must migrate from
#   SMOKE_LOG_COPY         directory the logs are copied to on failure
#
# On CI there is no `Claude Code-credentials` Keychain item, so providers show
# "not signed in" and macOS never opens a Keychain prompt.
set -euo pipefail

APP_ID="io.github.harveyxiacn.ai-usage-sidebar"
APP="${1:?usage: smoke-macos.sh path/to/App.app}"
TIMEOUT="${SMOKE_TIMEOUT:-90}"
UPGRADE_FROM="${SMOKE_UPGRADE_FROM:-}"
EXPECT_MIGRATION="${SMOKE_EXPECT_MIGRATION:-}"
LOG_DIR="$HOME/Library/Logs/$APP_ID"
SETTINGS="$HOME/Library/Application Support/$APP_ID/settings.json"
WORK="$(mktemp -d)"
APP_PID=""
MOUNT=""

[[ -d "$APP" ]] || { echo "No app bundle at $APP" >&2; exit 1; }
if [[ -z "${CI:-}" && -z "${SMOKE_ALLOW_REAL_PROFILE:-}" ]]; then
  echo "This test replaces your real settings.json and logs; set SMOKE_ALLOW_REAL_PROFILE=1" >&2
  exit 1
fi

stop_app() {
  if [[ -n "$APP_PID" ]]; then
    kill -TERM "$APP_PID" 2>/dev/null || true
    for _ in $(seq 1 20); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 0.25; done
    kill -KILL "$APP_PID" 2>/dev/null || true
    APP_PID=""
  fi
}
cleanup() {
  stop_app
  if [[ -n "$MOUNT" ]]; then hdiutil detach "$MOUNT" -quiet -force 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() {
  echo "SMOKE FAILED: $*" >&2
  echo "----- stdout/stderr -----" >&2; cat "$WORK/stdout.log" >&2 || true
  echo "----- app log -----" >&2; cat "$LOG_DIR"/* >&2 2>/dev/null || echo "(no log file was written)" >&2
  if [[ -n "${SMOKE_LOG_COPY:-}" ]]; then
    mkdir -p "$SMOKE_LOG_COPY"
    cp -R "$LOG_DIR"/. "$SMOKE_LOG_COPY"/ 2>/dev/null || true
    cp "$WORK/stdout.log" "$SMOKE_LOG_COPY"/ 2>/dev/null || true
  fi
  exit 1
}

# start_phase <label> <bundle>
start_phase() {
  local label="$1" exe
  exe="$(ls "$2"/Contents/MacOS/* | head -n1)"
  [[ -x "$exe" ]] || fail "no executable inside $2/Contents/MacOS"
  echo "== $label: launching $exe (timeout ${TIMEOUT}s)"
  "$exe" >"$WORK/stdout.log" 2>&1 &
  APP_PID=$!
  local found=0
  for _ in $(seq 1 $((TIMEOUT * 2))); do
    kill -0 "$APP_PID" 2>/dev/null || fail "$label: the app exited before it revealed the sidebar"
    if grep -qrs -- "sidebar revealed" "$LOG_DIR" 2>/dev/null; then found=1; break; fi
    sleep 0.5
  done
  [[ "$found" -eq 1 ]] || fail "$label: no \"sidebar revealed\" in $LOG_DIR within ${TIMEOUT}s"
  grep -qrs -- "platform setup:" "$LOG_DIR" || fail "$label: no \"platform setup:\" line"
  grep -qrs -- "tray menu ready" "$LOG_DIR" || fail "$label: no \"tray menu ready\" line"
  if grep -Ehrs -- 'panicked|\[ERROR\]' "$LOG_DIR" "$WORK/stdout.log"; then
    fail "$label: the log holds a panic or an ERROR line (shown above)"
  fi
  echo "$label: start-up looks healthy:"
  grep -hs -e "platform setup" -e "tray menu ready" -e "sidebar revealed" -e "usage.db schema" "$LOG_DIR"/* || true
}

rm -rf "$LOG_DIR"

if [[ -n "$UPGRADE_FROM" ]]; then
  MOUNT="$WORK/mnt"; mkdir -p "$MOUNT"
  hdiutil attach "$UPGRADE_FROM" -nobrowse -readonly -mountpoint "$MOUNT" -quiet
  OLD_APP="$(ls -d "$MOUNT"/*.app | head -n1)"
  cp -R "$OLD_APP" "$WORK/previous.app"
  hdiutil detach "$MOUNT" -quiet -force; MOUNT=""
  start_phase "previous release" "$WORK/previous.app"
  stop_app
  # Keep the old run's log apart so the new run's assertions read only its own.
  mv "$LOG_DIR" "$WORK/logs-previous"
  # A UTF-8 BOM in front of a partial key set: the shape of both v0.6 bugs.
  mkdir -p "$(dirname "$SETTINGS")"
  printf '\xef\xbb\xbf{ "edge": "left", "autoHide": true, "language": "zh-CN" }\n' >"$SETTINGS"
fi

start_phase "this build" "$APP"

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
