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
#                did not finish and the result is of a half-pressed script;
#                `keys: failed` (a step the app could not carry out, a file it
#                could not write) and `keys: unsettled` (a key went in before
#                the app had settled) are not results either. With `--keys`
#                any last line other than `keys: done` (or no file) is
#                reported on stderr after shot.png/title.txt/clip.txt are
#                taken, and this script ends non-zero: the app's own exit
#                code if it had exited by itself, else 1
#
# A reused OUTDIR is first emptied of the last run's files (keys.done, `*.txt`,
# `*.png`, `*.json` (run.json too), `*.log`) and of its `shots/` and `state/`
# folders (the shots' PNG and JSON): a `<State:>` or `<Shot:>` this run never
# wrote was read as this run's. A `shots` or `state` that is a link or not a
# folder stops the run (exit 2). A folder this script did not make (no `.keys-run`, no
# keys.done and no shot.png in it) is refused, so nothing of anyone else's is deleted.
#
# The clipboard is armed with XRUN-SENTINEL first, so an unchanged clipboard
# reads as that rather than as the previous run's. <BIN>_BIN (KURA_BIN)
# overrides the binary (default target/debug/<bin>). Needs Xvfb, xdotool,
# xclip, ImageMagick.

set -u
out="${1:?usage: scripts/xrun.sh OUTDIR [app arguments...]}"
shift
if [ -n "$(ls -A -- "$out" 2>/dev/null)" ] && [ ! -e "$out/.keys-run" ] && [ ! -e "$out/keys.done" ] && [ ! -e "$out/shot.png" ]; then
    echo "xrun: $out has files this script did not write; pass a new or empty folder" >&2
    exit 2
fi
mkdir -p -- "$out"
printf "an xrun.sh run folder: each run empties it of the last run's files\n" >"$out/.keys-run"
find "$out" -maxdepth 1 -type f \( -name 'keys.done*' -o -name '*.txt' -o -name '*.png' -o -name '*.json' -o -name '*.log' \) -delete \
    || { echo "xrun: could not empty $out of the last run's files" >&2; exit 2; }
for sub in shots state; do
    if [ -L "$out/$sub" ] || { [ -e "$out/$sub" ] && [ ! -d "$out/$sub" ]; }; then
        echo "xrun: $out/$sub is a link or not a folder; remove it, or pass a new folder" >&2
        exit 2
    fi
    rm -rf -- "$out/$sub" || { echo "xrun: could not empty $out/$sub" >&2; exit 2; }
done
rm -f -- "$out/run.json.part"
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

alive=1
kill -0 "$pid" 2>/dev/null || alive=0
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
code=$?
echo "xrun: $out (title: $(cat "$out/title.txt"))"

# With --keys, only a done file whose last line is `keys: done` is a result.
# `keys: failed`, `keys: unsettled`, `keys: refused`, `keys: stalled` or no file
# at all is not: say why (the shot and the title above are kept for that) and
# end non-zero, so a lane cannot take it for a pressed script.
if [ "$keyed" = 1 ] && [ "$(tail -1 "$out/keys.done" 2>/dev/null)" != "keys: done" ]; then
    if [ -e "$out/keys.done" ]; then
        echo "xrun: --keys did not end with 'keys: done'; this is not a result:" >&2
        grep -E '^(keys|step|why|pressed|left|unsettled):' "$out/keys.done" | sed 's/^/xrun:   /' >&2
    else
        echo "xrun: --keys wrote no keys.done; this is not a result" >&2
    fi
    if [ "$alive" = 0 ]; then
        echo "xrun: the app had exited by itself (exit code $code); see $log" >&2
        [ "$code" -ne 0 ] && exit "$code"
    fi
    exit 1
fi
