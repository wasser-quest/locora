#!/bin/sh
# Headless native smoke test: Xvfb + private D-Bus + AT-SPI, real Tauri/WebKitGTK.
# Usage (from the repository root, after `cargo build -p locora-app` in apps/app
# and `trunk build` in apps/app/ui):
#   apps/app/tests/run_smoke.sh [native_smoke.py arguments...]
#   apps/app/tests/run_smoke.sh --motion [splinely_motion_check.py arguments...]
# Requires xvfb-run, dbus-run-session, xdotool, at-spi2-core and python3-gi
# (Atspi 2.0, Gtk/Gdk 3.0). Debug builds load the UI from localhost:1420; a
# static server for apps/app/ui/dist is started when nothing listens there.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ "${LOCORA_SMOKE_INNER:-}" = 1 ]; then
    # D-Bus activation of the accessibility bus is not available everywhere
    # (sandboxes, minimal CI images); start both services explicitly.
    for dir in /usr/libexec /usr/lib/at-spi2-core /usr/lib/at-spi2; do
        if [ -x "$dir/at-spi-bus-launcher" ]; then
            "$dir/at-spi-bus-launcher" --launch-immediately &
            sleep 1
            [ -x "$dir/at-spi2-registryd" ] && "$dir/at-spi2-registryd" &
            sleep 1
            break
        fi
    done
    export GTK_MODULES=gail:atk-bridge NO_AT_BRIDGE=
    script=native_smoke.py
    if [ "${1:-}" = --motion ]; then
        script=splinely_motion_check.py
        shift
    fi
    exec python3 "$here/$script" "$@"
fi
server=
if ! python3 -c 'import socket; socket.create_connection(("127.0.0.1", 1420), 1)' 2>/dev/null; then
    python3 -m http.server 1420 --bind 127.0.0.1 --directory "$here/../ui/dist" >/dev/null 2>&1 &
    server=$!
    trap 'kill $server' EXIT
    sleep 1
fi
LOCORA_SMOKE_INNER=1 xvfb-run -a -s "-screen 0 1600x1400x24" dbus-run-session -- "$0" "$@"
