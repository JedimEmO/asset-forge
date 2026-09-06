#!/bin/sh
set -eu
cd -- "$(dirname -- "$0")"
# Route through the desktop mixer, rather than ALSA hardware enumeration.
# This configuration is private to this process; no system setting is changed.
if [ "$(uname -s)" = Linux ] && command -v pactl >/dev/null 2>&1; then
    if relay_sink=$(pactl get-default-sink 2>/dev/null); then
        export PULSE_SINK="$relay_sink"
        export PULSE_SERVER="${PULSE_SERVER:-unix:${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pulse/native}"
        export ALSA_CONFIG_PATH="$PWD/desktop-audio.conf"
        printf 'Relay Run audio: desktop default %s\n' "$relay_sink" >&2
    fi
fi
exec ./relay-runner "$@"
