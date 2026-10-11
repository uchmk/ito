#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
#
# Run the app once on a virtual X display and keep what it showed -- the Linux
# lane's way of pressing keys and reading the result (kura: .claude/linux-role.md).
#
#   scripts/xrun.sh OUTDIR [app arguments...]
#
# The app is scripts/lanes.conf's `bin` (kura: `kura`), and the variable it
# writes when the keys are done is its `keys_env` (kura: KURA_KEYS_DONE).
#
# Starts its own Xvfb, launches the app with the arguments (`--keys` included),
# waits for the window, and writes into OUTDIR what it shows. With `--keys` it
# waits until the app says the last key has been pressed and has settled
# (keys_env, at most XRUN_KEYS_TIMEOUT seconds, default 120), then
# XRUN_WAIT seconds more (default 1) for anything a key started in the
# background. Without `--keys` it waits XRUN_WAIT seconds (default 3). Until
# kura v0.60.1 it only ever waited XRUN_WAIT, and a script with more `<Wait:N>`
# in it than that was read half-pressed (kura#134). Writes into OUTDIR:
#
#   shot.png     the whole screen
#   title.txt    the window's title, which names the folder the list is in
#   clip.txt     the clipboard, read while the app still owns it (an X clipboard
#                dies with its owner, so reading it afterwards gets nothing)
#   <bin>.log    what the app printed (kura.log)
#   keys.done    there when every `--keys` key went in (last line `keys: done`);
#                missing, or `keys: stalled` (kura v0.67.12), means the script
#                did not finish and the result is of a half-pressed script
#
# The clipboard is armed with XRUN-SENTINEL first, so an unchanged clipboard
# reads as that rather than as the previous run's. <BIN>_BIN (KURA_BIN)
# overrides the binary (default target/debug/<bin>). Needs Xvfb, xdotool,
# xclip, ImageMagick.

set -u
out="${1:?usage: scripts/xrun.sh OUTDIR [app arguments...]}"
shift
mkdir -p -- "$out"
conf() { sed -n "s/^$1=//p" "$(dirname "$0")/lanes.conf"; }
name=$(conf bin)
keys_env=$(conf keys_env)
[ -n "$name" ] && [ -n "$keys_env" ] || { echo "xrun: scripts/lanes.conf needs bin= and keys_env=" >&2; exit 2; }
over="${name^^}_BIN"
bin="${!over:-target/debug/$name}"
log="$out/$name.log"
keyed=0
for a in "$@"; do [ "$a" = "--keys" ] && keyed=1; done
if [ "$keyed" = 1 ]; then wait_s="${XRUN_WAIT:-1}"; else wait_s="${XRUN_WAIT:-3}"; fi
keys_timeout="${XRUN_KEYS_TIMEOUT:-120}"
rm -f -- "$out/keys.done"

for tool in Xvfb xdotool xclip import; do
    command -v "$tool" >/dev/null || { echo "xrun: $tool is missing (apt-get install -y xvfb xdotool xclip imagemagick)" >&2; exit 2; }
done
# What winit and the renderer load at start. Missing, the app panics with a
# backtrace and this script could only say "no window appeared" (kura#131).
for lib in libxkbcommon-x11.so libvulkan.so; do
    ldconfig -p | grep -q "$lib" || { echo "xrun: $lib is missing (apt-get install -y libxkbcommon-x11-0 mesa-vulkan-drivers libvulkan1)" >&2; exit 2; }
done

# A display number nobody else holds.
n=90
while [ -e "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ]; do n=$((n + 1)); done
Xvfb ":$n" -screen 0 1400x900x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null' EXIT
export DISPLAY=":$n"
for _ in $(seq 1 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.1; done

printf 'XRUN-SENTINEL' | xclip -selection clipboard -i -loops 1 &
sleep 0.2

env "$keys_env=$out/keys.done" "$bin" "$@" >"$log" 2>&1 &
pid=$!
win=""
for _ in $(seq 1 100); do
    win=$(xdotool search --pid "$pid" --name . 2>/dev/null | head -1)
    [ -n "$win" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.2
done
if [ -z "$win" ]; then
    echo "xrun: no window appeared; see $log" >&2
    kill "$pid" 2>/dev/null
    exit 1
fi
if [ "$keyed" = 1 ]; then
    for _ in $(seq 1 $((keys_timeout * 5))); do
        [ -e "$out/keys.done" ] && break
        kill -0 "$pid" 2>/dev/null || break
        sleep 0.2
    done
    [ -e "$out/keys.done" ] || echo "xrun: --keys did not finish within ${keys_timeout}s; this is a half-pressed result" >&2
    # The app writes the file for a script that stalled too (kura v0.67.12), and
    # replaces it if the keys go on after all: wait that out once more.
    if head -1 "$out/keys.done" 2>/dev/null | grep -q '^keys: stalled'; then
        for _ in $(seq 1 $((keys_timeout * 5))); do
            tail -1 "$out/keys.done" | grep -q '^keys: done' && break
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.2
        done
        tail -1 "$out/keys.done" | grep -q '^keys: done' \
            || echo "xrun: --keys stalled ($(grep -E '^(pressed|left):' "$out/keys.done" | tr '\n' ' ')); this is a half-pressed result" >&2
    fi
fi
sleep "$wait_s"

import -window root "$out/shot.png"
xdotool getwindowname "$win" >"$out/title.txt" 2>/dev/null
timeout 2 xclip -selection clipboard -o >"$out/clip.txt" 2>/dev/null || true

kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
echo "xrun: $out (title: $(cat "$out/title.txt"))"
