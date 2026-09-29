#!/bin/sh
# Runs the original DiscordTimes.exe for differential testing
# (docs/superpowers/specs/2026-09-29-differential-testing-design.md): under Wine, on an
# invisible display (Xvfb), from a disposable copy of the install, with the sound thrown away.
# Nothing is written into the install itself or into ~/.wine.
#
#   scripts/run-original.sh <out-dir> <sec>...   screenshots at those seconds, then stops
#
# The work folder is RAZDOR_DIFFTEST_DIR (default ~/.local/share/razdor/difftest/original).
# The first run copies the install there (RAZDOR_DT_DIR, else the folder Razdor remembered)
# and makes a Wine prefix of its own; later runs reuse both. Needs wine, Xvfb and ImageMagick.
#
# What the original needs under Wine:
# - A Russian locale: Wine gives programs the ANSI code page of the locale, and with a
#   Western one the Cyrillic map names cannot be read and the game crashes while loading
#   (page fault at 00402BFD).
# - A sound driver: without one it stops at "No sound driver is available for use". Wine's
#   ALSA driver gets a private config whose default device is ALSA's null device.
set -eu
[ $# -ge 2 ] || { echo "usage: $0 <out-dir> <sec>..." >&2; exit 2; }
OUT=$1; shift
W=${RAZDOR_DIFFTEST_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/razdor/difftest}/original
mkdir -p "$W" "$OUT"

if [ ! -f "$W/game/DiscordTimes.exe" ]; then
    SRC=${RAZDOR_DT_DIR:-$(cat "${XDG_CONFIG_HOME:-$HOME/.config}/razdor/install" 2>/dev/null || true)}
    [ -f "$SRC/DiscordTimes.exe" ] || { echo "no install: set RAZDOR_DT_DIR" >&2; exit 1; }
    echo "copying the install from $SRC"
    cp -a "$SRC" "$W/game"
fi
cat > "$W/alsa-null.conf" <<'EOF'
</usr/share/alsa/alsa.conf>
pcm.!default { type null }
ctl.!default { type hw card 0 }
EOF

export WINEPREFIX="$W/prefix" WINEDEBUG=${RAZDOR_WINEDEBUG:--all}
export WINEDLLOVERRIDES="mscoree,mshtml=;winepulse.drv=d"
export ALSA_CONFIG_PATH="$W/alsa-null.conf"
export LANG=ru_RU.UTF-8 LC_ALL=ru_RU.UTF-8

# A free display number of our own.
N=77
while [ -e "/tmp/.X$N-lock" ]; do N=$((N + 1)); done
export DISPLAY=:$N
Xvfb "$DISPLAY" -screen 0 1024x768x24 -nolisten tcp >/dev/null 2>&1 &
XVFB=$!
stop() {
    wineserver -k 2>/dev/null || true
    kill "$XVFB" 2>/dev/null || true
}
trap stop EXIT INT TERM
sleep 1

if [ ! -f "$WINEPREFIX/system.reg" ]; then
    echo "making the Wine prefix"
    wineboot -i >/dev/null 2>&1
    wineserver -w
fi

(cd "$W/game" && exec wine DiscordTimes.exe) >"$OUT/wine.log" 2>&1 &
t=0
for at in "$@"; do
    sleep $((at - t)); t=$at
    import -window root "$OUT/shot-$at.png"
done
if grep -q "Unhandled" "$OUT/wine.log"; then
    grep "Unhandled" "$OUT/wine.log" >&2
    exit 1
fi
echo "screenshots in $OUT"
