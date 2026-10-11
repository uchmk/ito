#!/usr/bin/env bash
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its
# scripts/lanes.sh sync, so edit ito's.
# Push this checkout's own commits to main, renumbered onto whatever main has
# become, verified, without a merge commit and without --force.
#
#   scripts/push-main.sh            push
#   scripts/push-main.sh --status   say whether main's CI is red, job by job
#
# Commit as usual first: one commit per change, its version bumped in
# Cargo.toml / Cargo.lock, its CHANGELOG.md section, a `vX.Y.Z: ` subject.
# Then run this instead of `git merge origin/main` + `git push`.
#
# Why (2026-10-05): the development routine, the merge routine and an
# interactive session pushed to main within ten minutes, each merged the
# others in and renumbered in the merge commit. The version came out right,
# but three commits kept the subject `v0.78.31:` and two `v0.78.29:`, and the
# release notes are built from those subjects (release.yml skips merges).
#
# What it does, up to five times until a push lands:
#   1. fetch main; if main is not under HEAD (or HEAD holds a merge of it),
#      replay each of HEAD's own non-merge commits onto main, one by one:
#      the version gets the same kind of bump on top of main's, and the
#      commit's `vOLD` becomes `vNEW` in its message, its CHANGELOG heading
#      and section, and in the lines it adds (TODO.md's `（vX.Y.Z。…）` ticks);
#   2. scripts/verify.sh on the result, and stop if it changed a tracked
#      file (v0.78.42 to v0.78.44 went out without the Cargo.lock their own
#      checks had rewritten);
#   3. wait while main's newest CI run is still going, up to ten minutes:
#      a push to main cancels it, and with a push every three to five
#      minutes the Windows test job never finished (v0.78.54 went red there
#      and nobody saw it for six versions). Then say if it ended red;
#   4. fetch again and start over if main moved meanwhile; else push.
#
# Run it in the background and wait for it: the checks take about three
# minutes and the wait up to ten (a whole Windows test job), past the usual
# ten-minute limit of a foreground command.
#
# The commits replayed have never been pushed, so this rewrites nothing
# anyone else has seen; CLAUDE.md's merge-commit rule is about branches
# merged into main, which keep their merge.
#
# It stops, and changes nothing, when a commit's change does not apply on
# the new main (a real conflict, usually in TODO.md) or touches Cargo.toml /
# Cargo.lock beyond the version. Then: `git rebase origin/main`, resolve by
# hand, fix the version, the CHANGELOG heading and the subject, run it again.

set -euo pipefail
cd "$(dirname "$0")/.."

die() { printf 'push-main: %s\n' "$*" >&2; exit 1; }

# owner/repo from the remote, whatever form its URL takes.
repo=$(git remote get-url origin | sed -E 's#\.git$##; s#.*[/:]([^/:]+/[^/]+)$#\1#')
api="https://api.github.com/repos/$repo/actions"

# Main's CI runs, newest first, as JSON on stdout; nothing when unreadable.
runs() {
    curl -fsS --max-time 20 -H 'Accept: application/vnd.github+json' \
        "$api/workflows/ci.yml/runs?branch=main&event=push&per_page=${1:-1}" 2>/dev/null || true
}

# "status conclusion sha url" of main's newest CI run.
newest() {
    runs 1 | python3 -c 'import json, sys
r = json.load(sys.stdin)["workflow_runs"]
if r: print(r[0]["status"], r[0]["conclusion"] or "-", r[0]["head_sha"][:7], r[0]["html_url"])' 2>/dev/null || true
}

# The newest run that finished (a cancelled one says nothing), job by job.
status() {
    local json
    json=$(runs 15)
    [ -n "$json" ] || { echo "push-main: could not read main's CI"; return 1; }
    printf '%s' "$json" | python3 -c '
import json, sys, urllib.request
done = [r for r in json.load(sys.stdin)["workflow_runs"] if r["conclusion"] not in (None, "cancelled", "skipped")]
if not done:
    sys.exit("push-main: no finished CI run on main among the last 15")
r = done[0]
how, sha, title, url = r["conclusion"], r["head_sha"][:7], r["display_title"][:70], r["html_url"]
print(f"main CI: {how} on {sha} {title}")
print(f"  {url}")
req = urllib.request.Request(r["jobs_url"], headers={"Accept": "application/vnd.github+json"})
for j in json.load(urllib.request.urlopen(req, timeout=20))["jobs"]:
    name, how = j["name"], j["conclusion"]
    print(f"  {name:<12} {how}")
if r["conclusion"] != "success":
    print("RED: fix this before taking a new item (CLAUDE.md, 自動実行モード). The job log needs a")
    print("token this session does not have; reproduce it from the test name in TODO.md, or read")
    print("which tests are #[cfg(windows)]-sensitive (paths, separators) in the commits since green.")
    sys.exit(2)
'
}

# Wait while main's newest CI run is still going (step 3 above).
wait_for_ci() {
    local state waited=0
    while :; do
        state=$(newest)
        if [ -z "$state" ]; then
            echo "push-main: could not read main's CI; pushing without waiting"
            return
        fi
        set -- $state
        if [ "$1" = completed ]; then
            case $2 in
                success|cancelled|skipped) ;;
                *) echo "push-main: WARNING: main's CI on $3 ended $2 ($4); a red main comes before new items (CLAUDE.md)" ;;
            esac
            return
        fi
        if [ $waited -ge 600 ]; then
            echo "push-main: main's CI on $3 is still $1 after ten minutes; pushing"
            return
        fi
        [ $waited -gt 0 ] || echo "push-main: main's CI on $3 is $1; waiting for it, so this push does not cancel it"
        sleep 20
        waited=$((waited + 20))
    done
}

if [ "${1:-}" = --status ]; then
    status
    exit
fi

[ -z "$(git status --porcelain --untracked-files=no)" ] || die "uncommitted changes; commit or restore them first"

replay() {
    python3 - <<'PY'
import os, re, subprocess, sys, tempfile

def git(*args, cwd=None, input=None, check=True):
    r = subprocess.run(["git", *args], cwd=cwd, input=input, capture_output=True)
    if check and r.returncode != 0:
        sys.exit(f"push-main: git {' '.join(args)} failed:\n{r.stderr.decode(errors='replace')}")
    return r

def out(*args, cwd=None):
    return git(*args, cwd=cwd).stdout.decode()

def show(rev, path):
    r = git("show", f"{rev}:{path}", check=False)
    return r.stdout.decode() if r.returncode == 0 else None

# The app's crate, whose version Cargo.lock carries, from scripts/lanes.conf.
CRATE = dict(l.split("=", 1) for l in open("scripts/lanes.conf", encoding="utf-8").read().splitlines()
             if "=" in l and not l.startswith("#"))["crate"]

VERSION = re.compile(r'^version = "(\d+)\.(\d+)\.(\d+)"', re.M)

def version(text):
    m = VERSION.search(text)
    return tuple(int(x) for x in m.groups())

def vs(v):
    return ".".join(map(str, v))

def bump(tip, base, own):
    if own == base:
        return None
    if own[0] != base[0]:
        return (tip[0] + 1, 0, 0)
    if own[1] != base[1]:
        return (tip[0], tip[1] + 1, 0)
    return (tip[0], tip[1], tip[2] + 1)

def renamed(text, old, new):
    return re.sub(r"(?<![\w.])v" + re.escape(old) + r"(?![\d])", "v" + new, text)

commits = out("rev-list", "--reverse", "--no-merges", "origin/main..HEAD").split()
if not commits:
    sys.exit("push-main: nothing of this checkout's own is missing from main")

wt = tempfile.mkdtemp(prefix="push-main-")
git("worktree", "add", "-q", "--detach", wt, "origin/main")
try:
    for c in commits:
        subject = out("log", "-1", "--format=%s", c).strip()
        base = version(show(f"{c}^", "Cargo.toml"))
        own = version(show(c, "Cargo.toml"))
        tip = version(open(os.path.join(wt, "Cargo.toml"), encoding="utf-8", newline="").read())
        new = bump(tip, base, own)

        # Cargo.toml and Cargo.lock may differ only in the app's own version.
        for f in ("Cargo.toml", "Cargo.lock"):
            for line in out("diff", "-U0", f"{c}^", c, "--", f).splitlines():
                if line.startswith(("+++", "---", "@@", "diff ", "index ")):
                    continue
                if not re.fullmatch(r'[+-]version = "\d+\.\d+\.\d+"', line):
                    sys.exit(f"push-main: {c[:7]} ({subject}) changes {f} beyond the version; replay it by hand")

        skip = [":(exclude)Cargo.toml", ":(exclude)Cargo.lock"]
        if new:
            skip.append(":(exclude)CHANGELOG.md")
        patch = git("diff", "--binary", f"{c}^", c, "--", ".", *skip).stdout
        if new and new != own:
            lines = patch.split(b"\n")
            for i, line in enumerate(lines):
                if line.startswith(b"+") and not line.startswith(b"+++"):
                    lines[i] = renamed(line.decode(), vs(own), vs(new)).encode()
            patch = b"\n".join(lines)
        if patch.strip():
            r = git("apply", "--3way", "--index", "--whitespace=nowarn", cwd=wt, input=patch, check=False)
            if r.returncode != 0:
                sys.exit(f"push-main: {c[:7]} ({subject}) does not apply on the new main:\n"
                         f"{r.stderr.decode(errors='replace')}\n"
                         "Nothing was changed. `git rebase origin/main`, resolve, fix the version, "
                         "the CHANGELOG heading and the subject, and run this again.")

        if new:
            mine, before = show(c, "CHANGELOG.md"), show(f"{c}^", "CHANGELOG.md")
            m = re.search(r"^## \[" + re.escape(vs(own)) + r"\][^\n]*\n.*?(?=^## \[|\Z)", mine, re.S | re.M)
            if not m or mine[:m.start()] + mine[m.end():] != before:
                sys.exit(f"push-main: {c[:7]} ({subject}) changes CHANGELOG.md other than adding [{vs(own)}]; replay it by hand")
            section = renamed(m.group(0).replace(f"## [{vs(own)}]", f"## [{vs(new)}]", 1), vs(own), vs(new))
            path = os.path.join(wt, "CHANGELOG.md")
            log = open(path, encoding="utf-8", newline="").read()
            at = re.search(r"^## \[\d", log, re.M)
            log = log[:at.start()] + section + log[at.start():] if at else log + "\n" + section
            open(path, "w", encoding="utf-8", newline="").write(log)

            for f, pat in (("Cargo.toml", r'^(version = ")[\d.]+(")'),
                           ("Cargo.lock", r'^(name = "' + re.escape(CRATE) + r'"\nversion = ")[\d.]+(")')):
                path = os.path.join(wt, f)
                text = open(path, encoding="utf-8", newline="").read()
                text, n = re.subn(pat, r"\g<1>" + vs(new) + r"\g<2>", text, count=1, flags=re.M)
                if n != 1:
                    sys.exit(f"push-main: could not set the version in {f}")
                open(path, "w", encoding="utf-8", newline="").write(text)
            git("add", "CHANGELOG.md", "Cargo.toml", "Cargo.lock", cwd=wt)

        message = out("log", "-1", "--format=%B", c)
        if new and new != own:
            message = renamed(message, vs(own), vs(new))
        env = dict(os.environ)
        for key, fmt in (("GIT_AUTHOR_NAME", "%an"), ("GIT_AUTHOR_EMAIL", "%ae"), ("GIT_AUTHOR_DATE", "%aD")):
            env[key] = out("log", "-1", f"--format={fmt}", c).strip()
        r = subprocess.run(["git", "commit", "-q", "--allow-empty", "-F", "-"], cwd=wt, env=env,
                           input=message.encode(), capture_output=True)
        if r.returncode != 0:
            sys.exit(f"push-main: commit failed:\n{r.stderr.decode(errors='replace')}")
        said = f"v{vs(own)} -> v{vs(new)}" if new and new != own else "kept its version"
        print(f"push-main: {c[:7]} {said}: {subject}")

    head = out("rev-parse", "HEAD", cwd=wt).strip()
finally:
    git("worktree", "remove", "--force", wt, check=False)

git("reset", "-q", "--keep", head)
print(f"push-main: replayed onto main as {head[:7]}")
PY
}

needs_replay() {
    ! git merge-base --is-ancestor origin/main HEAD || [ -n "$(git rev-list --merges origin/main..HEAD)" ]
}

for attempt in 1 2 3 4 5; do
    git fetch -q origin main
    [ -n "$(git rev-list origin/main..HEAD)" ] || { echo "push-main: nothing to push"; exit 0; }
    if needs_replay; then
        replay
    fi
    scripts/verify.sh
    changed=$(git status --porcelain --untracked-files=no)
    [ -z "$changed" ] || die "the checks changed tracked files the commit should have held:
$changed
Commit them into it (git commit --amend: it has not been pushed) and run this again."
    git fetch -q origin main
    if needs_replay; then
        echo "push-main: main moved during the checks; again (attempt $attempt)"
        continue
    fi
    wait_for_ci
    git fetch -q origin main
    if needs_replay; then
        echo "push-main: main moved while waiting for its CI; again (attempt $attempt)"
        continue
    fi
    if res=$(git push origin HEAD:main 2>&1); then
        echo "$res" | tail -1
        git log -1 --format='push-main: pushed %h %s'
        exit 0
    fi
    echo "$res" | grep -qE 'rejected|fetch first|non-fast-forward' || die "push failed: $res"
    echo "push-main: main moved before the push; again (attempt $attempt)"
done
die "main kept moving; five tries and no push"
