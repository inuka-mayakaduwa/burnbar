#!/usr/bin/env bash
# Headless end-to-end smoke test for the Tauri shell.
#
# Boots the real binary in demo mode on a virtual X server with a D-Bus session
# (WebKitGTK and the tray/single-instance plugins need one), waits, and fails if
# the process exits early. No account, network access or display is required.
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "xvfb-run is required (Ubuntu: apt install xvfb dbus-x11)" >&2
  exit 1
fi

cargo build -p burnbar-desktop

xvfb-run -a dbus-run-session -- bash -c '
  ./target/debug/burnbar --demo > /tmp/burnbar-smoke.log 2>&1 &
  pid=$!
  sleep 12
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "BurnBar exited during the smoke test" >&2
    cat /tmp/burnbar-smoke.log >&2
    exit 1
  fi
  kill "$pid"
  wait "$pid" 2>/dev/null || true
  echo "BurnBar stayed alive for 12 seconds in demo mode"
'
