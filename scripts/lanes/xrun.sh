#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
#
# Run the app once on a virtual X display and keep what it showed -- the Linux
# lane's way of pressing keys and reading the result (kura: .claude/linux-role.md).
#
#   scripts/xrun.sh OUTDIR [app arguments...]
#   scripts/xrun.sh --evidence LABEL [--spec F]... [--scenario F]... [--evidence-root DIR] [--] [app arguments...]
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
# `*.png`, `*.json` (run.json too), `*.jsonl` (trace.jsonl), `*.log`) and of its `shots/` and `state/`
# folders (the shots' PNG and JSON): a `<State:>` or `<Shot:>` this run never
# wrote was read as this run's. A `shots` or `state` that is a link or not a
# folder stops the run (exit 2). A folder this script did not make (no `.keys-run`, no
# keys.done and no shot.png in it) is refused, so nothing of anyone else's is deleted.
#
# The app is started with `setsid`, so it leads its own process group. On exit, by
# any way (also a failure, ^C or SIGTERM), one EXIT trap asks the app alone to end
# (TERM, up to ~3 s, then KILL); what is still in the group then is what it left
# running, and gets TERM, up to ~3 s, then KILL. Then Xvfb is stopped. A program
# that made its own session or process group is outside the group: it is neither
# seen nor ended.
#
# --evidence LABEL (an evidence run, docs/visual-test.md; there is no OUTDIR): runs
# `evidence.py new` for the folder, passes the run-id variable (keys_env without
# `_DONE`, plus `_RUN_ID`) and one empty folder per variable in lanes.conf's
# `isolate=` (under OUTDIR/isolate, so the run reads and writes none of the
# machine's settings), and in the EXIT trap removes OUTDIR/isolate and calls
# `evidence.py manifest` with the exit code, --killed (this script had to stop an app
# whose keys had not finished: a time-out, no window, ^C, SIGTERM) and the cleanup
# verdict (PASS only when the app left nothing in its group, Xvfb stopped on TERM and
# isolate was removed). The folder is the last line. --spec
# goes to `new` and `manifest`, --scenario to `manifest`, --evidence-root to `new`
# (else lanes.conf's `evidence=`). The app arguments must include --keys. Exit 5:
# no evidence (a refusal before `new`, `new` failed, `manifest` could not run or
# exited 2); 6: the run's own code was 0 but the manifest lists problems; otherwise
# the run's code. TODO "keys.ps1 / xrun.sh -Evidence", #205.
#
# The clipboard is armed with XRUN-SENTINEL first, so an unchanged clipboard
# reads as that rather than as the previous run's. <BIN>_BIN (KURA_BIN)
# overrides the binary (default target/debug/<bin>). Needs Xvfb, xdotool,
# xclip, ImageMagick, setsid (util-linux) and ps (procps).

set -u
usage='usage: scripts/xrun.sh OUTDIR [app arguments...]'
evidence=0 label="" ev_root=""
specs=() scenarios=()
while [ $# -gt 0 ]; do
    case "$1" in
        --evidence) label="${2:?xrun: --evidence needs a LABEL}"; evidence=1; shift 2 ;;
        --spec) specs+=("${2:?xrun: --spec needs a file}"); shift 2 ;;
        --scenario) scenarios+=("${2:?xrun: --scenario needs a file}"); shift 2 ;;
        --evidence-root) ev_root="${2:?xrun: --evidence-root needs a folder}"; shift 2 ;;
        --) shift; break ;;
        *) break ;;
    esac
done
if [ "$evidence" = 1 ]; then
    usage='usage: scripts/xrun.sh --evidence LABEL [--spec F]... [--scenario F]... [--evidence-root DIR] [--] [app arguments...]'
    out=""
else
    if [ "${#specs[@]}" -gt 0 ] || [ "${#scenarios[@]}" -gt 0 ] || [ -n "$ev_root" ]; then
        echo "xrun: --spec, --scenario and --evidence-root need --evidence" >&2
        exit 2
    fi
    out="${1:?$usage}"
    shift
fi
# Set before anything can fail, so the EXIT trap is safe under `set -u`.
xvfb="" pid="" killed=0 pgid_bad=0 evpy="" notes=()
scale_name="" scale_val="" scale_src=""
# What a missing tool or setting ends with: with --evidence that is "no evidence" (5).
refuse=2
[ "$evidence" = 1 ] && refuse=5

# Empties the last run's files from the run folder (the marker first, so a folder this script made is known as ours).
prep_folder() {
    mkdir -p -- "$out"
    # Evidence: evidence.py wrote the marker; the folder is new and its pre/ is a folder, which the cleaning below never touches.
    [ -e "$out/.keys-run" ] || printf "an xrun.sh run folder: each run empties it of the last run's files\n" >"$out/.keys-run"
    find "$out" -maxdepth 1 -type f \( -name 'keys.done*' -o -name '*.txt' -o -name '*.png' -o -name '*.json' -o -name '*.jsonl' -o -name '*.log' \) -delete \
        || { echo "xrun: could not empty $out of the last run's files" >&2; exit 2; }
    for sub in shots state; do
        if [ -L "$out/$sub" ] || { [ -e "$out/$sub" ] && [ ! -d "$out/$sub" ]; }; then
            echo "xrun: $out/$sub is a link or not a folder; remove it, or pass a new folder" >&2
            exit 2
        fi
        rm -rf -- "${out:?}/${sub:?}" || { echo "xrun: could not empty $out/$sub" >&2; exit 2; }
    done
    rm -f -- "$out/run.json.part"
}

# Asks the app alone to end (bounded, so an app that ignores TERM cannot hold the script).
# The status is the app's. What is still in its group afterwards is what it left running.
stop_app() {
    kill "$pid" 2>/dev/null
    for _ in $(seq 1 15); do kill -0 "$pid" 2>/dev/null || break; sleep 0.2; done
    kill -0 "$pid" 2>/dev/null && kill -KILL "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
}
# What is in the app's group, as "name (pid), ...". Zombies are not counted: nothing can end them.
group_left() {
    ps -eo pid=,pgid=,stat=,comm= 2>/dev/null | awk -v g="$pid" '$2 == g && $3 !~ /^Z/ { printf "%s%s (%s)", (n++ ? ", " : ""), $4, $1 }'
}
# An explicit code for each signal: the EXIT trap's $? does not then depend on how bash ended.
catch_signals() { trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP; }

# One EXIT trap for every way out. Order matters: the app first, then what it left in its
# group (judged before it is killed), then Xvfb (its clients are gone by then), and the
# manifest last, so it sees the final files.
on_exit() {
    local rc=$? cleanup=PASS left n m mrc final
    trap - EXIT
    trap '' INT TERM HUP # the cleanup and the manifest finish even under a second ^C
    set +e
    if [ -n "$pid" ]; then
        if kill -0 "$pid" 2>/dev/null; then
            killed=1
            stop_app
        fi
        # A helper that ends along with the app gets a moment.
        [ -z "$(group_left)" ] || sleep 0.3
        left=$(group_left)
        if [ -n "$left" ]; then
            notes+=("left running after the run: $left")
            cleanup=FAIL
            kill -TERM -- -"$pid" 2>/dev/null
            for _ in $(seq 1 15); do [ -n "$(group_left)" ] || break; sleep 0.2; done
            if [ -n "$(group_left)" ]; then
                kill -KILL -- -"$pid" 2>/dev/null
                sleep 0.3
            fi
            left=$(group_left)
            [ -z "$left" ] || notes+=("still running after KILL: $left")
        fi
    fi
    [ "$pgid_bad" = 0 ] || cleanup=FAIL
    if [ -n "$xvfb" ]; then
        kill "$xvfb" 2>/dev/null
        for _ in $(seq 1 15); do kill -0 "$xvfb" 2>/dev/null || break; sleep 0.2; done
        if kill -0 "$xvfb" 2>/dev/null; then
            kill -KILL "$xvfb" 2>/dev/null
            notes+=("Xvfb did not stop on TERM")
            cleanup=FAIL
        fi
        wait "$xvfb" 2>/dev/null
    fi
    if [ "$evidence" = 1 ]; then
        if [ -L "$out/isolate" ]; then
            notes+=("isolate is a link; not removed"); cleanup=FAIL
        else
            rm -rf -- "${out:?}/isolate" 2>/dev/null
            [ -e "$out/isolate" ] && { notes+=("isolate was not removed"); cleanup=FAIL; }
        fi
        local margs=(manifest --run "$out" --exit-code "$rc" --cleanup "$cleanup")
        [ "$killed" = 1 ] && margs+=(--killed)
        if [ "${#notes[@]}" -gt 0 ]; then
            n=""
            for m in "${notes[@]}"; do n="${n:+$n; }$m"; done
            margs+=(--note "$n")
        fi
        [ -n "$scale_src" ] && margs+=(--scale-env "$scale_name" "--scale-value=$scale_val" --scale-source "$scale_src")
        for m in ${specs[@]+"${specs[@]}"}; do margs+=(--spec "$m"); done
        for m in ${scenarios[@]+"${scenarios[@]}"}; do margs+=(--scenario "$m"); done
        PYTHONUTF8=1 python3 "$evpy" "${margs[@]}"
        mrc=$?
        final=$rc
        if [ "$mrc" = 0 ]; then :
        elif [ "$mrc" = 1 ]; then [ "$rc" = 0 ] && final=6
        else echo "xrun: evidence.py manifest exited $mrc; there is no manifest" >&2; final=5
        fi
        echo "$out"
        exit "$final"
    fi
    exit "$rc"
}

# Not evidence: the foreign-folder check runs before anything is read, as it always did.
if [ "$evidence" = 0 ] && [ -n "$(ls -A -- "$out" 2>/dev/null)" ] && [ ! -e "$out/.keys-run" ] && [ ! -e "$out/keys.done" ] && [ ! -e "$out/shot.png" ]; then
    echo "xrun: $out has files this script did not write; pass a new or empty folder" >&2
    exit 2
fi
conf() { sed -n "s/^$1=//p" "$(dirname "$0")/lanes.conf"; }
name=$(conf bin)
keys_env=$(conf keys_env)
isolate=$(conf isolate)
keyed=0
for a in "$@"; do [ "$a" = "--keys" ] && keyed=1; done
if [ "$evidence" = 0 ]; then
    prep_folder
    trap on_exit EXIT
    catch_signals
fi
[ -n "$name" ] && [ -n "$keys_env" ] || { echo "xrun: scripts/lanes.conf needs bin= and keys_env=" >&2; exit "$refuse"; }
over="${name^^}_BIN"
bin="${!over:-target/debug/$name}"
if [ "$keyed" = 1 ]; then wait_s="${XRUN_WAIT:-1}"; else wait_s="${XRUN_WAIT:-3}"; fi
keys_timeout="${XRUN_KEYS_TIMEOUT:-120}"

for tool in Xvfb xdotool xclip import setsid ps; do
    command -v "$tool" >/dev/null || { echo "xrun: $tool is missing (apt-get install -y xvfb xdotool xclip imagemagick util-linux procps)" >&2; exit "$refuse"; }
done
# What winit and the renderer load at start. Missing, the app panics with a
# backtrace and this script could only say "no window appeared" (kura#131).
for lib in libxkbcommon-x11.so libvulkan.so; do
    ldconfig -p | grep -q "$lib" || { echo "xrun: $lib is missing (apt-get install -y libxkbcommon-x11-0 mesa-vulkan-drivers libvulkan1)" >&2; exit "$refuse"; }
done

envargs=("$keys_env=$out/keys.done")
if [ "$evidence" = 1 ]; then
    # Nothing is made until every refusal has been checked: a refused run leaves no folder.
    why=""
    if [ "$keyed" != 1 ]; then why="an evidence run is a keys run: the app arguments need --keys"
    elif [ -z "$isolate" ]; then why="an evidence run is isolated, and scripts/lanes.conf has no isolate="
    elif [ "${keys_env%_DONE}" = "$keys_env" ]; then why="keys_env in scripts/lanes.conf does not end in _DONE, so there is no run-id variable name"
    elif ! command -v python3 >/dev/null; then why="python3 is missing"
    fi
    if [ -n "$why" ]; then echo "xrun: --evidence refused: $why" >&2; exit 5; fi
    evpy="$(dirname "$0")/evidence.py"
    nargs=(new --label "$label")
    for m in ${specs[@]+"${specs[@]}"}; do nargs+=(--spec "$m"); done
    [ -n "$ev_root" ] && nargs+=(--evidence "$ev_root")
    # A signal between `new` and the EXIT trap would leave a folder with no manifest: it is held until the trap is in place.
    held=""
    trap 'held=130' INT; trap 'held=143' TERM; trap 'held=129' HUP
    nout=$(PYTHONUTF8=1 python3 "$evpy" "${nargs[@]}" 2>&1) && nrc=0 || nrc=$?
    rid=$(printf '%s\n' "$nout" | sed -n 's/^run_id=//p')
    out=$(printf '%s\n' "$nout" | sed -n 's/^dir=//p')
    run_env=$(printf '%s\n' "$nout" | sed -n 's/^env=//p')
    new_scale_env=$(printf '%s\n' "$nout" | sed -n 's/^scale_env=//p')
    new_scale=$(printf '%s\n' "$nout" | sed -n 's/^scale=//p')
    if [ "$nrc" != 0 ] || [ -z "$rid" ] || [ -z "$out" ] || [ -z "$run_env" ]; then
        printf '%s\n' "$nout" | sed 's/^/xrun: evidence.py: /' >&2
        echo "xrun: evidence.py new did not make a run folder; nothing was run" >&2
        out=""
        exit 5
    fi
    trap on_exit EXIT
    catch_signals
    [ -z "$held" ] || exit "$held"
    prep_folder
    envargs=("$keys_env=$out/keys.done" "$run_env=$rid")
    # The app's scale override (scale_env= in lanes.conf): the first spec's pixels_per_point when `new` handed one over (source spec),
    # else a value already set by hand (source environment). Either way it goes to the manifest.
    conf_scale_env=$(conf scale_env)
    if [ -n "$new_scale_env" ] && [ -n "$new_scale" ]; then
        scale_name=$new_scale_env scale_val=$new_scale scale_src=spec
        envargs+=("$scale_name=$scale_val")
    elif [ -n "$conf_scale_env" ] && [ -n "${!conf_scale_env:-}" ]; then
        scale_name=$conf_scale_env scale_val=${!conf_scale_env} scale_src=environment
    fi
    for v in $isolate; do
        mkdir -p -- "$out/isolate/$v"
        envargs+=("$v=$out/isolate/$v")
    done
fi
log="$out/$name.log"

# A display number nobody else holds.
n=90
while [ -e "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ]; do n=$((n + 1)); done
Xvfb ":$n" -screen 0 1400x900x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
export DISPLAY=":$n"
for _ in $(seq 1 50); do xdotool getdisplaygeometry >/dev/null 2>&1 && break; sleep 0.1; done

printf 'XRUN-SENTINEL' | xclip -selection clipboard -i -loops 1 &
sleep 0.2

# setsid: the app leads its own process group, so one kill reaches what its keys started. Under `&` in a
# script (no job control) this shell is not a group leader, so setsid does not fork and $! is the app.
setsid env "${envargs[@]}" "$bin" "$@" >"$log" 2>&1 &
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
    exit 1 # the EXIT trap stops the app if it is still running (--killed)
fi
# The group kills and the cleanup check only mean something if the app leads the group.
# An app that has already ended has no group to check.
pg=$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')
if [ -n "$pg" ] && [ "$pg" != "$pid" ]; then
    pgid_bad=1
    notes+=("the app does not lead its own process group (setsid); cleanup cannot be judged")
    echo "xrun: the app does not lead its own process group" >&2
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
if [ "$alive" = 1 ] && [ "$keyed" = 1 ] && [ "$(tail -1 "$out/keys.done" 2>/dev/null)" != "keys: done" ]; then killed=1; fi
stop_app
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
