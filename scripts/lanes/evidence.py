#!/usr/bin/env python3
"""Run folder, manifest, check and verdict for a visual-test run (ito#205 part C).

    evidence.py new [--evidence PARENT] (--run-id ID | --label ROW) [--spec PATH]...
    evidence.py manifest --run DIR --exit-code N [--killed] --cleanup PASS|FAIL [--note TEXT]
                         [--scenario PATH]... [--spec PATH]... [--app-commit SHA]
    evidence.py check --run DIR
    evidence.py verdict --run DIR --spec tests/visual/<row>.toml [--replace]
    evidence.py --self-test [--emit DIR]

Copied byte for byte into every app by lanes.sh sync: it reads nothing from ito's docs/. Stdlib only, Python 3.11+.
"""
import sys

if sys.version_info < (3, 11):
    sys.exit("evidence.py: Python 3.11 or newer is required (tomllib); this is %d.%d" % sys.version_info[:2])

import argparse
import contextlib
import hashlib
import io
import json
import math
import os
import re
import secrets
import shutil
import struct
import subprocess
import tempfile
import time
import tomllib
import zlib

TOOL = {"name": "evidence.py", "version": 1}
# The one-line text keys.ps1 writes into `.keys-run` (the self-test compares it with keys.ps1 when that sits beside).
MARKER_TEXT = "a keys.ps1 run folder: each run empties it of the last run's files"
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))  # the self-test points this at a temp folder

RUN_ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
ROW_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z._-]{0,63}$")
NAME_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
OID_RE = re.compile(r"^[0-9a-f]{40}([0-9a-f]{24})?$")
STATUS_OK = ("done", "quit", "closed")
UNLISTED_OK = ("machine/", "vision/", "crops/", "canary/")

# The closed set of check names (visual-check.py's), so a spec it would reject is rejected here too.
CHECK_NAMES = {
    "present", "visible", "text", "label", "value", "role", "elided", "enabled", "focused", "screen", "overlay",
    "no_overlay", "toast", "no_toast", "app_state", "theme", "dark", "width_px", "height_px", "left_gap_px",
    "right_gap_px", "top_gap_px", "bottom_gap_px", "inside", "unclipped", "in_window", "aligned", "no_overlap", "gap",
    "color_token",
}
SPEC_KEYS = {"spec", "row", "tier", "fixtures", "theme", "lang", "window", "point"}
POINT_KEYS = {"id", "snap", "state", "target", "relative_to", "fail_looks_like", "expect"}


# ---------------------------------------------------------------- small helpers


def _no_constant(name):
    raise ValueError("non-finite number %s" % name)


def loads(text):
    return json.loads(text, parse_constant=_no_constant)


def dumps(obj):
    return json.dumps(obj, ensure_ascii=False, indent=2, sort_keys=False) + "\n"


def sha256_bytes(b):
    return hashlib.sha256(b).hexdigest()


def read_bytes(path):
    with open(path, "rb") as f:
        return f.read()


def read_json(path):
    """(object, None) or (None, reason)."""
    try:
        return loads(read_bytes(path).decode("utf-8")), None
    except FileNotFoundError:
        return None, "missing"
    except (OSError, ValueError) as e:
        return None, "unreadable: %s" % e


def write_atomic(path, data):
    """`<path>.part` -> flush -> fsync -> replace; the `.part` is removed on failure."""
    if isinstance(data, str):
        data = data.encode("utf-8")
    part = path + ".part"
    try:
        with open(part, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(part, path)
    except BaseException:
        try:
            os.remove(part)
        except OSError:
            pass
        raise


def is_int(v):
    return isinstance(v, int) and not isinstance(v, bool)


def is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def safe_artifact_path(p):
    """A path inside the run folder: relative, `/`-separated, no `..`, no empty parts."""
    if not isinstance(p, str) or not p or "\\" in p or "\0" in p or p.startswith("/") or re.match(r"^[A-Za-z]:", p):
        return False
    parts = p.split("/")
    return all(x not in ("", ".", "..") for x in parts)


def sha_size(path):
    b = read_bytes(path)
    return sha256_bytes(b), len(b)


def is_link(entry):
    st = entry.stat(follow_symlinks=False)
    return entry.is_symlink() or bool(getattr(st, "st_file_attributes", 0) & 0x400)


def walk(root):
    """[(posix path, 'file' | 'link' | 'other')] for everything under root; links are not followed."""
    out = []
    stack = [""]
    while stack:
        rel = stack.pop()
        with os.scandir(os.path.join(root, rel) if rel else root) as it:
            entries = sorted(it, key=lambda e: e.name)
        for e in entries:
            r = rel + "/" + e.name if rel else e.name
            if is_link(e):
                out.append((r, "link"))
            elif e.is_dir(follow_symlinks=False):
                stack.append(r)
            elif e.is_file(follow_symlinks=False):
                out.append((r, "file"))
            else:
                out.append((r, "other"))
    return sorted(out)


def err(msg):
    print(msg, file=sys.stderr)


# ---------------------------------------------------------------- git


def git(top, *args):
    """(returncode, stdout, stderr); never depends on the cwd."""
    try:
        p = subprocess.run(
            ["git", "-C", top, "--literal-pathspecs", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
        )
    except OSError as e:
        return 127, "", str(e)
    return p.returncode, p.stdout.strip(), p.stderr.strip()


def repo_info(path):
    """(top level or None, repo path). Outside a repository the path is returned as given, with `/`."""
    ap = os.path.abspath(path)
    d = os.path.dirname(ap)
    if os.path.isdir(d):
        rc, out, _ = git(d, "rev-parse", "--show-toplevel", "--show-prefix")
        if rc == 0 and out:
            lines = out.split("\n")
            prefix = lines[1].strip() if len(lines) > 1 else ""
            return lines[0].strip(), prefix + os.path.basename(ap)
    return None, path.replace("\\", "/")


# ---------------------------------------------------------------- lanes.conf


def read_conf():
    """lanes.conf beside the script: {key: value}; empty when there is none."""
    conf = {}
    try:
        text = read_bytes(os.path.join(SCRIPT_DIR, "lanes.conf")).decode("utf-8", errors="replace")
    except OSError:
        return conf
    for line in text.split("\n"):
        line = line.rstrip("\r")
        m = re.match(r"^([a-z_]+)=(.*)$", line)
        if m:
            conf[m.group(1)] = m.group(2)
    return conf


# ---------------------------------------------------------------- the spec (only what is cross-checked)


def read_spec(path):
    """(info, errors). info = {row, tier, points: [(id, [expect keys])]} when errors is empty."""
    errors = []
    try:
        data = tomllib.loads(read_bytes(path).decode("utf-8"))
    except (OSError, ValueError) as e:
        return None, ["cannot read spec: %s" % e]
    for k in data:
        if k not in SPEC_KEYS:
            errors.append("unknown key `%s`" % k)
    if data.get("spec") != 1 or not is_int(data.get("spec")):
        errors.append("`spec` must be 1")
    row = data.get("row")
    if not isinstance(row, str) or not ROW_RE.match(row) or ".." in row:
        errors.append("`row` must be a row id string")
    tier = data.get("tier")
    if not is_int(tier) or tier not in (1, 2, 3):
        errors.append("`tier` must be 1, 2 or 3")
    w = data.get("window")
    if not isinstance(w, dict) or set(w) != {"size_pt", "pixels_per_point"}:
        errors.append("`window` must be a table with size_pt and pixels_per_point")
    else:
        sp = w["size_pt"]
        if not (isinstance(sp, list) and len(sp) == 2 and all(is_num(x) and x > 0 for x in sp)):
            errors.append("`window.size_pt` must be two positive numbers")
        if not (is_num(w["pixels_per_point"]) and w["pixels_per_point"] > 0):
            errors.append("`window.pixels_per_point` must be positive")
    pts = data.get("point")
    points, seen = [], set()
    if not isinstance(pts, list) or not pts or not all(isinstance(p, dict) for p in pts):
        errors.append("the spec needs at least one [[point]]")
        pts = []
    for p in pts:
        pid = p.get("id")
        for k in p:
            if k not in POINT_KEYS:
                errors.append("point `%s`: unknown key `%s`" % (pid, k))
        if not isinstance(pid, str) or not re.match(r"^[A-Za-z0-9._-]+$", pid):
            errors.append("a point id must match [A-Za-z0-9._-]+")
            continue
        if pid in seen:
            errors.append("duplicate point id `%s`" % pid)
        seen.add(pid)
        if ("snap" in p) == ("state" in p):
            errors.append("point `%s`: exactly one of snap / state" % pid)
        elif not isinstance(p.get("snap", p.get("state")), str) or not NAME_RE.match(p.get("snap", p.get("state"))):
            errors.append("point `%s`: bad snap / state name" % pid)
        if not isinstance(p.get("fail_looks_like"), str) or not p.get("fail_looks_like").strip():
            errors.append("point `%s`: fail_looks_like is required" % pid)
        ex = p.get("expect")
        if not isinstance(ex, dict) or not ex:
            errors.append("point `%s`: expect must be a non-empty table" % pid)
            continue
        for k in ex:
            if k not in CHECK_NAMES:
                errors.append("point `%s`: unknown check `%s`" % (pid, k))
        points.append((pid, list(ex)))
    if errors:
        return None, errors
    return {"row": row, "tier": tier, "points": points, "pixels_per_point": w["pixels_per_point"]}, []


# ---------------------------------------------------------------- new


def spec_gate(path):
    """None when the spec is a committed, unchanged file; else the reason."""
    if not os.path.isfile(path):
        return "spec does not exist: %s" % path
    top, rel = repo_info(path)
    if top is None:
        return "spec is not in a git repository: %s" % path
    rc, _, _ = git(top, "ls-files", "--error-unmatch", "--", rel)
    if rc != 0:
        return "spec is not tracked by git: git add and commit %s before the run" % rel
    rc, _, _ = git(top, "rev-parse", "--verify", "-q", "HEAD")
    if rc != 0:
        return "spec is not committed: the repository has no commit; commit %s before the run" % rel
    rc, _, _ = git(top, "diff", "--quiet", "HEAD", "--", rel)
    if rc != 0:
        return "spec is not committed: commit %s before the run" % rel
    return None


def foreign_path(p, name=os.name):
    """`p` is written for the other OS, so it cannot name a folder here: `C:\\x`, `C:x` or `\\\\host\\x` on POSIX, `/x` on Windows."""
    if name == "nt":
        return p.startswith("/") and not p.startswith("//")
    if p.startswith("/"):
        return False
    return "\\" in p or (p[:1].isalpha() and p[1:2] == ":")


SCALE_ENV_RE = re.compile(r"^[A-Z_][A-Z0-9_]*$")


def scale_for_run(conf, specs):
    """scale_env= in lanes.conf is set: check it and the specs' pixels_per_point.

    Returns the text of the first spec's scale (None with no spec), or 2 after an `err(...)`. Nothing is made yet."""
    name = conf["scale_env"]
    if not SCALE_ENV_RE.match(name):
        err("scale_env=%s in scripts/lanes.conf is not an environment variable name (A-Z 0-9 _, not starting with a digit)" % name)
        return 2
    keys_env = conf.get("keys_env", "")
    taken = {}
    if keys_env:
        taken[keys_env] = "keys_env"
        if keys_env.endswith("_DONE"):
            taken[keys_env[: -len("_DONE")] + "_RUN_ID"] = "the run-id variable"
    for v in conf.get("isolate", "").split():
        taken[v] = "an isolate= variable"
    for other, what in taken.items():
        if other.upper() == name.upper():  # Windows environment names are case-insensitive
            err("scale_env=%s in scripts/lanes.conf is the same variable as %s (%s): the scale would overwrite it or the reverse"
                % (name, what, other))
            return 2
    first = None
    for s in specs:
        info, errs = read_spec(s)
        if errs:
            err("spec %s: %s (scale_env is set, so the spec's window is read before the run)" % (s, "; ".join(errs)))
            return 2
        ppp = info["pixels_per_point"]
        if first is None:
            first = (s, ppp)
        elif ppp != first[1]:
            err("the specs disagree on window.pixels_per_point: %s has %s, %s has %s; one run has one scale" % (first[0], first[1], s, ppp))
            return 2
    return None if first is None else str(first[1])


def cmd_new(a):
    conf = read_conf()
    parent = a.evidence or conf.get("evidence")
    if not parent:
        err("no evidence folder: pass --evidence or set evidence= in scripts/lanes.conf")
        return 2
    if not a.evidence:
        if foreign_path(parent):
            err("evidence=%s in scripts/lanes.conf is a path for another OS: on this machine pass --evidence "
                "(xrun.sh --evidence-root, keys.ps1 -EvidenceRoot)" % parent)
            return 2
        if not os.path.isabs(parent):
            parent = os.path.join(os.path.dirname(SCRIPT_DIR), parent)
    if a.run_id is not None:
        rid = a.run_id
        if not RUN_ID_RE.match(rid):
            err("bad --run-id: 1 to 64 of A-Z a-z 0-9 _ -")
            return 2
    else:
        label = re.sub(r"[^A-Za-z0-9_-]", "_", a.label or "")
        if not label:
            err("--label is empty")
            return 2
        head = time.strftime("%Y%m%d-%H%M%S-")
        label = label[: 64 - len(head) - 5]
        rid = head + label + "-" + secrets.token_hex(2)
    for s in a.spec:
        why = spec_gate(s)
        if why:
            err(why)
            return 2
    scale = None
    if conf.get("scale_env"):
        scale = scale_for_run(conf, a.spec)
        if isinstance(scale, int):
            return scale
    intents = []
    for s in a.spec:
        top, rel = repo_info(s)
        rc1, blob, _ = git(top, "hash-object", "--", rel)
        rc2, head, _ = git(top, "rev-parse", "HEAD")
        if rc1 != 0 or rc2 != 0 or not OID_RE.match(blob) or not OID_RE.match(head):
            err("git could not name %s" % rel)
            return 2
        intents.append({"path": rel, "sha256": sha256_bytes(read_bytes(s)), "blob": blob, "head": head})
    try:
        os.makedirs(parent, exist_ok=True)
        run = os.path.join(parent, rid)
        os.mkdir(run)
    except OSError as e:
        err("cannot make the run folder (an existing one is never reused): %s" % e)
        return 2
    try:
        write_atomic(os.path.join(run, ".keys-run"), MARKER_TEXT + "\n")
        os.mkdir(os.path.join(run, "pre"))
        intent = {
            "schema": "intent.v1", "tool": TOOL, "run_id": rid, "written_ms": time.time_ns() // 1_000_000, "specs": intents,
        }
        write_atomic(os.path.join(run, "pre", "intent.json"), dumps(intent))
    except OSError as e:
        shutil.rmtree(run, ignore_errors=True)
        err("cannot write the run folder: %s" % e)
        return 2
    print("run_id=" + rid)
    print("dir=" + os.path.abspath(run))
    env = conf.get("keys_env", "")
    if env.endswith("_DONE"):
        print("env=" + env[: -len("_DONE")] + "_RUN_ID")
    if scale is not None:
        print("scale_env=" + conf["scale_env"])
        print("scale=" + scale)
    return 0


# ---------------------------------------------------------------- manifest


def opt_str(v):
    return v if isinstance(v, str) else None


def opt_int(v):
    return v if is_int(v) else None


def repo_entries(paths, problems, what):
    out = []
    for p in paths:
        _, rel = repo_info(p)
        if os.path.isfile(p):
            sha = sha256_bytes(read_bytes(p))
        else:
            sha = None
            problems.append("%s missing: %s" % (what, rel))
        out.append({"path": rel, "sha256": sha})
    return out


def cmd_manifest(a):
    run = os.path.abspath(a.run)
    if not os.path.isdir(run):
        err("run folder does not exist: %s" % a.run)
        return 2
    rid = os.path.basename(run)
    problems = []
    rj, why = read_json(os.path.join(run, "run.json"))
    if rj is None:
        problems.append("run.json %s" % why)
        rj = {}
    elif not isinstance(rj, dict):
        problems.append("run.json unreadable: not an object")
        rj = {}
    else:
        if rj.get("run_id") != rid:
            problems.append("run.json run_id %r != folder name %r" % (rj.get("run_id"), rid))
        if rj.get("status") not in STATUS_OK:
            problems.append("run.json status %r is not done, quit or closed" % (rj.get("status"),))
        if rj.get("failed") is not False:
            problems.append("run.json failed is not false")
        if not (is_int(rj.get("left")) and rj.get("left") == 0):
            problems.append("run.json left is not 0")
    sub = lambda k: rj.get(k) if isinstance(rj.get(k), dict) else {}  # noqa: E731
    kd_run = kd_last = None
    kd = os.path.join(run, "keys.done")
    if not os.path.isfile(kd):
        problems.append("keys.done missing")
    else:
        lines = read_bytes(kd).decode("utf-8", errors="replace").splitlines()
        for ln in lines:
            if ln.startswith("run: "):
                kd_run = ln[5:].strip()
                break
        nonempty = [ln.strip() for ln in lines if ln.strip()]
        kd_last = nonempty[-1] if nonempty else None
        if kd_run != rid:
            problems.append("keys.done run %r != folder name %r" % (kd_run, rid))
        if kd_last != "keys: done":
            problems.append("keys.done last line is not 'keys: done'")
    if not os.path.isfile(os.path.join(run, "trace.jsonl")):
        problems.append("trace.jsonl missing")
    intent, why = read_json(os.path.join(run, "pre", "intent.json"))
    if intent is None:
        problems.append("pre/intent.json %s" % why)
    elif not isinstance(intent, dict) or intent.get("run_id") != rid:
        problems.append("pre/intent.json run_id != folder name")
    if a.exit_code != 0:
        problems.append("exit code %d != 0" % a.exit_code)
    if a.killed:
        problems.append("killed")
    if a.cleanup != "PASS":
        problems.append("cleanup %s != PASS" % a.cleanup)
    if os.path.exists(os.path.join(run, "blocked.txt")):
        problems.append("blocked.txt present")
    scenarios = repo_entries(a.scenario, problems, "scenario")
    specs = repo_entries(a.spec, problems, "spec")
    envs = {}
    shots = os.path.join(run, "shots")
    if os.path.isdir(shots) and not os.path.islink(shots):
        for n in sorted(os.listdir(shots)):
            if n.endswith(".json"):
                sj, why = read_json(os.path.join(shots, n))
                if sj is None or not isinstance(sj, dict):
                    problems.append("shots/%s unreadable" % n)
                elif isinstance(sj.get("env"), dict):
                    envs[json.dumps(sj["env"], ensure_ascii=False)] = sj["env"]
    files = []
    for rel, kind in walk(run):
        if rel in ("manifest.json", "manifest.json.part"):
            continue
        if kind != "file":
            problems.append("not a regular file, not followed: %s" % rel)
            continue
        if rel.endswith(".part"):
            problems.append("partial file present: %s" % rel)
        sha, size = sha_size(os.path.join(run, rel))
        files.append({"path": rel, "sha256": sha, "size": size})
    given = [a.scale_env, a.scale_value, a.scale_source]
    scale_override = None
    if all(v is not None for v in given):
        scale_override = {"env": a.scale_env, "value": a.scale_value, "source": a.scale_source}
    elif any(v is not None for v in given):
        problems.append("scale override: --scale-env, --scale-value and --scale-source must be given together")
    manifest = {
        "schema": "manifest.v1",
        "tool": TOOL,
        "run_id": rid,
        "written_ms": time.time_ns() // 1_000_000,
        "app": {"name": opt_str(sub("app").get("name")), "version": opt_str(sub("app").get("version")), "commit": a.app_commit},
        "runner": opt_str(rj.get("runner")),
        "ito": {"version": opt_str(sub("ito").get("version")), "rev": opt_str(sub("ito").get("rev"))},
        "os": opt_str(rj.get("os")),
        "arch": opt_str(rj.get("arch")),
        "started_ms": opt_int(rj.get("started_ms")),
        "ended_ms": opt_int(rj.get("ended_ms")),
        "status": opt_str(rj.get("status")),
        "keys_done": {"run": kd_run, "last": kd_last},
        "exit_code": a.exit_code,
        "killed": bool(a.killed),
        "cleanup": a.cleanup,
        "note": a.note,
        "scenarios": scenarios,
        "specs": specs,
        "envs": [envs[k] for k in sorted(envs)],
        "scale_override": scale_override,
        "files": files,
        "problems": problems,
    }
    try:
        write_atomic(os.path.join(run, "manifest.json"), dumps(manifest))
    except OSError as e:
        err("cannot write manifest.json: %s" % e)
        return 2
    for p in problems:
        print("problem: " + p)
    return 1 if problems else 0


# ---------------------------------------------------------------- check


def check_run(run):
    """(manifest or None, problems). None means no readable manifest."""
    run = os.path.abspath(run)
    rid = os.path.basename(run)
    m, why = read_json(os.path.join(run, "manifest.json"))
    if m is None:
        return None, ["manifest.json %s" % why]
    if not isinstance(m, dict) or m.get("schema") != "manifest.v1" or not isinstance(m.get("files"), list):
        return None, ["manifest.json is not a manifest.v1"]
    problems = []
    if m.get("run_id") != rid:
        problems.append("manifest run_id %r != folder name %r" % (m.get("run_id"), rid))
    listed = set()
    for f in m["files"]:
        p = f.get("path") if isinstance(f, dict) else None
        if not safe_artifact_path(p) or p in ("manifest.json", "manifest.json.part"):
            problems.append("bad path in the manifest: %r" % (p,))
            continue
        if p in listed:
            problems.append("listed twice: %s" % p)
            continue
        listed.add(p)
        if not (isinstance(f.get("sha256"), str) and SHA_RE.match(f["sha256"]) and is_int(f.get("size"))):
            problems.append("bad entry in the manifest: %s" % p)
            continue
        full = os.path.join(run, *p.split("/"))
        try:
            st = os.lstat(full)
        except OSError:
            problems.append("missing: %s" % p)
            continue
        if os.path.islink(full) or bool(getattr(st, "st_file_attributes", 0) & 0x400) or not os.path.isfile(full):
            problems.append("not a regular file: %s" % p)
            continue
        sha, size = sha_size(full)
        if size != f["size"]:
            problems.append("size changed: %s" % p)
        elif sha != f["sha256"]:
            problems.append("changed: %s" % p)
    for rel, kind in walk(run):
        if rel in listed or rel in ("manifest.json", "manifest.json.part"):
            continue
        if kind != "file":
            problems.append("not a regular file: %s" % rel)
        elif rel.startswith(UNLISTED_OK) or (re.match(r"^verdict-[^/]+\.json$", rel) is not None):
            continue
        else:
            problems.append("not in the manifest: %s" % rel)
    return m, problems


def cmd_check(a):
    m, problems = check_run(a.run)
    for p in problems:
        print("problem: " + p)
    if m is None:
        return 2
    return 1 if problems else 0


# ---------------------------------------------------------------- verdict


def outcome_of(checks, errors):
    outs = [c.get("outcome") for c in checks]
    if errors or "ERROR" in outs:
        return "ERROR"
    return "FAIL" if "FAIL" in outs else "PASS"


def assertions_problems(asr, spec, spec_path, spec_sha):
    """Reasons the assertions do not match the spec (empty = they do)."""
    r = []
    if not isinstance(asr, dict):
        return ["assertions.json is not an object"]
    if asr.get("schema") != "assertions.v1":
        r.append("schema is not assertions.v1")
    if asr.get("row") != spec["row"]:
        r.append("row %r != spec row %r" % (asr.get("row"), spec["row"]))
    if asr.get("tier") != spec["tier"] or not is_int(asr.get("tier")):
        r.append("tier %r != spec tier %r" % (asr.get("tier"), spec["tier"]))
    sp = asr.get("spec") if isinstance(asr.get("spec"), dict) else {}
    if sp.get("path") != spec_path:
        r.append("spec.path %r != %r" % (sp.get("path"), spec_path))
    if sp.get("sha256") != spec_sha:
        r.append("spec.sha256 is not the spec's sha256 now")
    pts = asr.get("points")
    if not isinstance(pts, list) or not all(isinstance(p, dict) for p in pts):
        return r + ["points is not a list of objects"]
    if [p.get("id") for p in pts] != [pid for pid, _ in spec["points"]]:
        r.append("point ids differ from the spec's")
    else:
        for p, (pid, keys) in zip(pts, spec["points"]):
            checks = p.get("checks")
            if not isinstance(checks, list) or not all(isinstance(c, dict) for c in checks):
                r.append("point %s: checks is not a list of objects" % pid)
                continue
            names = [c.get("check") for c in checks]
            if sorted(map(str, names)) != sorted(keys) or len(set(map(str, names))) != len(names):
                r.append("point %s: checks %s != expect keys %s" % (pid, names, keys))
            if any(c.get("outcome") not in ("PASS", "FAIL", "ERROR") for c in checks):
                r.append("point %s: a check outcome is not PASS / FAIL / ERROR" % pid)
                continue
            perr = p.get("errors")
            if not isinstance(perr, list):
                r.append("point %s: errors is not a list" % pid)
                continue
            if p.get("outcome") != outcome_of(checks, perr):
                r.append("point %s: outcome %r does not follow from its checks" % (pid, p.get("outcome")))
    terr = asr.get("errors")
    if not isinstance(terr, list):
        r.append("errors is not a list")
    else:
        outs = [p.get("outcome") for p in pts]
        want = "ERROR" if terr or "ERROR" in outs else ("FAIL" if "FAIL" in outs else "PASS")
        if asr.get("overall") != want:
            r.append("overall %r does not follow from the points (%s)" % (asr.get("overall"), want))
    return r


def expected_defined(run, rid, spec_top, spec_rel, spec_sha, m, asr):
    """(bool, reasons): the spec was fixed before the run and is unchanged since."""
    why = []
    intent, e = read_json(os.path.join(run, "pre", "intent.json"))
    if intent is None or not isinstance(intent, dict):
        return False, ["pre/intent.json %s" % (e or "is not an object")]
    mf = {f.get("path"): f for f in m.get("files", []) if isinstance(f, dict)}
    cur = os.path.join(run, "pre", "intent.json")
    if "pre/intent.json" not in mf or mf["pre/intent.json"].get("sha256") != sha256_bytes(read_bytes(cur)):
        why.append("pre/intent.json is not in the manifest with its sha256")
    if intent.get("run_id") != rid:
        why.append("pre/intent.json run_id != folder name")
    entries = [s for s in intent.get("specs", []) if isinstance(s, dict) and s.get("path") == spec_rel] if isinstance(intent.get("specs"), list) else []
    if len(entries) != 1:
        why.append("pre/intent.json lists %s %d times, not once" % (spec_rel, len(entries)))
        return False, why
    ent = entries[0]
    wm, sm = intent.get("written_ms"), m.get("started_ms")
    if not (is_int(wm) and is_int(sm)) or wm > sm:
        why.append("intent written_ms %r is not before run started_ms %r" % (wm, sm))
    ms = [s for s in m.get("specs", []) if isinstance(s, dict) and s.get("path") == spec_rel]
    asha = asr.get("spec", {}).get("sha256") if isinstance(asr.get("spec"), dict) else None
    if len(ms) != 1 or not (spec_sha == ent.get("sha256") == ms[0].get("sha256") == asha):
        why.append("spec sha256 differs between now, the intent, the manifest and the assertions")
    if spec_top is None:
        why.append("the spec is not in a git repository")
    else:
        head, blob = ent.get("head"), ent.get("blob")
        if not (isinstance(head, str) and OID_RE.match(head) and isinstance(blob, str) and OID_RE.match(blob)):
            why.append("intent head / blob are not git object ids")
        else:
            rc1, at_head, _ = git(spec_top, "rev-parse", "%s:%s" % (head, spec_rel))
            rc2, now, _ = git(spec_top, "hash-object", "--", spec_rel)
            if rc1 != 0 or rc2 != 0:
                why.append("git could not name the spec at the recorded HEAD")
            elif not (at_head == blob == now):
                why.append("the spec at the recorded HEAD is not the content checked (blob mismatch)")
    return not why, why


def cmd_verdict(a):
    run = os.path.abspath(a.run)
    rid = os.path.basename(run)
    if not os.path.isdir(run):
        err("run folder does not exist: %s" % a.run)
        return 2
    spec, errs = read_spec(a.spec)
    if spec is None:
        err("bad spec: " + "; ".join(errs))
        return 2
    row = spec["row"]
    out_path = os.path.join(run, "verdict-%s.json" % row)
    if os.path.exists(out_path) and not a.replace:
        err("%s exists; pass --replace" % os.path.basename(out_path))
        return 2
    m, problems = check_run(run)
    if m is None:
        err("no readable manifest: %s" % problems[0])
        return 2
    asr, e = read_json(os.path.join(run, "machine", "assertions.json"))
    if asr is None:
        err("machine/assertions.json %s" % e)
        return 2
    spec_sha = sha256_bytes(read_bytes(a.spec))
    top, rel = repo_info(a.spec)
    tier = spec["tier"]
    asr_ok = isinstance(asr, dict)
    A = asr if asr_ok else {}
    mism = assertions_problems(asr, spec, rel, spec_sha)
    asr_match = not mism
    mfiles = {f["path"]: f.get("sha256") for f in m.get("files", []) if isinstance(f, dict) and "path" in f}
    pts = [p for p in A.get("points", []) if isinstance(p, dict)] if isinstance(A.get("points"), list) else []

    def inputs_of(obj):
        v = obj.get("inputs")
        return [i for i in v if isinstance(i, dict)] if isinstance(v, list) else []

    def in_manifest(i):
        return safe_artifact_path(i.get("path")) and mfiles.get(i["path"]) == i.get("sha256") and isinstance(i.get("sha256"), str)

    # same_run
    same_why = []
    if not (A.get("run_id") == m.get("run_id") == rid):
        same_why.append("run_id differs between the assertions, the manifest and the folder")
    for i in inputs_of(A) + [i for p in pts for i in inputs_of(p)]:
        if not in_manifest(i):
            same_why.append("assertions input not in the manifest with its sha256: %r" % (i.get("path"),))
    same_run = not same_why
    # screenshot
    snaps = [p for p in pts if p.get("snap") is not None]
    shot_why = []
    for p in snaps:
        want = "shots/%s.png" % p["snap"] if isinstance(p["snap"], str) else None
        if not any(i.get("path") == want and in_manifest(i) for i in inputs_of(p)):
            shot_why.append("point %s: %s is not an input with the manifest's sha256" % (p.get("id"), want))
    screenshot = (not shot_why) if snaps else "n/a"
    # state_json
    st_why = []
    for p in pts:
        n = p.get("snap") if p.get("snap") is not None else p.get("state")
        need = ["state/%s-before.json" % n, "state/%s-after.json" % n] if p.get("snap") is not None else ["state/%s.json" % n]
        for path in need:
            if not any(i.get("path") == path and in_manifest(i) for i in inputs_of(p)):
                st_why.append("point %s: %s is not an input with the manifest's sha256" % (p.get("id"), path))
    state_json = bool(pts) and not st_why
    errors_top = A.get("errors") if isinstance(A.get("errors"), list) else []
    machine_issues = sum(1 for p in pts if p.get("outcome") != "PASS") + len(errors_top)
    unconfirmed = len(errors_top)
    for p in pts:
        perr = p.get("errors") if isinstance(p.get("errors"), list) else []
        unconfirmed += len(perr) + sum(1 for c in p.get("checks", []) if isinstance(c, dict) and c.get("outcome") == "ERROR")
    exp_ok, exp_why = expected_defined(run, rid, top, rel, spec_sha, m, A)
    cleanup = m.get("cleanup")
    mprob = m.get("problems") if isinstance(m.get("problems"), list) else None
    cond = {
        "expected_defined": exp_ok,
        "assertions_match": asr_match,
        "screenshot": screenshot,
        "state_json": state_json,
        "same_run": same_run,
        "machine_issues": machine_issues,
        "vision_points": "n/a" if tier != 2 else "not implemented",
        "major_mismatch": False,
        "unconfirmed": unconfirmed,
        "cleanup": cleanup,
        "rationale_saved": bool(pts),
        "manifest_intact": not problems,
        "run_problems": len(mprob) if mprob is not None else -1,
    }
    reasons = []
    if tier == 3:
        reasons.append("tier 3 is decided by the owner")
    if tier == 2:
        reasons.append("Vision cross-check is not implemented in this version")
    if not exp_ok:
        reasons.append("expected_defined: " + "; ".join(exp_why))
    if not asr_match:
        reasons.append("assertions_match: " + "; ".join(mism))
    if screenshot is False:
        reasons.append("screenshot: " + "; ".join(shot_why))
    if not state_json:
        reasons.append("state_json: " + ("; ".join(st_why) or "no points"))
    if not same_run:
        reasons.append("same_run: " + "; ".join(same_why))
    if machine_issues:
        reasons.append("machine_issues: %d" % machine_issues)
    if unconfirmed:
        reasons.append("unconfirmed: %d" % unconfirmed)
    if cleanup != "PASS":
        reasons.append("cleanup: %s" % cleanup)
    if not cond["rationale_saved"]:
        reasons.append("rationale_saved: the assertions have no points")
    if problems:
        reasons.append("manifest_intact: " + "; ".join(problems[:5]))
    if cond["run_problems"] != 0:
        reasons.append("run_problems: %d" % cond["run_problems"])
    overall = A.get("overall")
    if tier == 3:
        result = "owner"
    elif not (cond["manifest_intact"] and same_run and asr_match and exp_ok):
        result = "undecidable"
    elif overall == "FAIL":
        result = "fail"
    elif tier == 2:
        result = "undecidable"
    elif (
        overall == "PASS" and screenshot in (True, "n/a") and state_json and machine_issues == 0 and unconfirmed == 0
        and cleanup == "PASS" and cond["rationale_saved"] and cond["run_problems"] == 0
    ):
        result = "auto-pass-candidate"
    else:
        result = "undecidable"
    if result == "auto-pass-candidate":
        reasons = []
    elif not reasons:
        reasons.append("assertions overall is %r" % (overall,))
    inputs = [{"path": "manifest.json", "sha256": sha256_bytes(read_bytes(os.path.join(run, "manifest.json")))}]
    intent_path = os.path.join(run, "pre", "intent.json")
    if os.path.isfile(intent_path):
        inputs.append({"path": "pre/intent.json", "sha256": sha256_bytes(read_bytes(intent_path))})
    inputs.append({"path": "machine/assertions.json", "sha256": sha256_bytes(read_bytes(os.path.join(run, "machine", "assertions.json")))})
    verdict = {
        "schema": "verdict.v1",
        "tool": TOOL,
        "row": row,
        "tier": tier,
        "run_id": rid,
        "result": result,
        "conditions": cond,
        "reasons": reasons,
        "evidence": ["machine/assertions.json#%s" % p["id"] for p in pts if isinstance(p.get("id"), str)],
        "inputs": inputs,
        "spec": {"path": rel, "sha256": spec_sha},
        "mark": None,
        "decided_by": None,
    }
    try:
        write_atomic(out_path, dumps(verdict))
    except OSError as e:
        err("cannot write %s: %s" % (out_path, e))
        return 2
    print("%s: %s" % (os.path.basename(out_path), result))
    for r in reasons:
        print("reason: " + r)
    return {"auto-pass-candidate": 0, "fail": 1}.get(result, 3)


# ---------------------------------------------------------------- CLI


def build_parser():
    ap = argparse.ArgumentParser(prog="evidence.py")
    sub = ap.add_subparsers(dest="cmd", required=True)
    n = sub.add_parser("new")
    n.add_argument("--evidence")
    g = n.add_mutually_exclusive_group(required=True)
    g.add_argument("--run-id")
    g.add_argument("--label")
    n.add_argument("--spec", action="append", default=[])
    m = sub.add_parser("manifest")
    m.add_argument("--run", required=True)
    m.add_argument("--exit-code", type=int, required=True)
    m.add_argument("--killed", action="store_true")
    m.add_argument("--cleanup", choices=["PASS", "FAIL"], required=True)
    m.add_argument("--note")
    m.add_argument("--scenario", action="append", default=[])
    m.add_argument("--spec", action="append", default=[])
    m.add_argument("--app-commit")
    m.add_argument("--scale-env")
    m.add_argument("--scale-value")
    m.add_argument("--scale-source", choices=["spec", "environment"])
    c = sub.add_parser("check")
    c.add_argument("--run", required=True)
    v = sub.add_parser("verdict")
    v.add_argument("--run", required=True)
    v.add_argument("--spec", required=True)
    v.add_argument("--replace", action="store_true")
    return ap


class _Usage(Exception):
    pass


def main(argv):
    if "--self-test" in argv:
        rest = [x for x in argv if x != "--self-test"]
        emit = None
        if rest[:1] == ["--emit"] and len(rest) == 2:
            emit = rest[1]
        elif rest:
            err("usage: evidence.py --self-test [--emit DIR]")
            return 2
        return self_test(emit)
    ap = build_parser()
    try:
        with contextlib.redirect_stderr(io.StringIO()) as buf:
            a = ap.parse_args(argv)
    except SystemExit as e:
        text = buf.getvalue()
        if e.code in (0, None):
            return 0
        sys.stderr.write(text)
        return 2
    return {"new": cmd_new, "manifest": cmd_manifest, "check": cmd_check, "verdict": cmd_verdict}[a.cmd](a)


# ---------------------------------------------------------------- self-test


def png_bytes(w, h, rgba=(10, 20, 30, 255)):
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    row = b"\0" + bytes(rgba) * w
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)) + chunk(
        b"IDAT", zlib.compress(row * h)
    ) + chunk(b"IEND", b"")


SPEC_TEMPLATE = """spec = 1
row = "{row}"
tier = {tier}
window = {{ size_pt = [90, 42], pixels_per_point = 1.5 }}

[[point]]
id = "thumb-top"
snap = "top"
target = "spot.scroll.thumb"
relative_to = "spot.panel"
fail_looks_like = "the thumb sticks out of the panel"
expect = {{ right_gap_px = {{ min = 0, max = 2 }}, width_px = {{ min = 4, max = 9 }} }}

[[point]]
id = "title"
state = "hello"
target = "title"
fail_looks_like = "no title"
expect = {{ present = true }}
"""

STABLE = {"projection": 1, "ui_sha256": "a" * 64, "app_sha256": "b" * 64}


def state_doc(rid, name, phase, frame, after_step, after_label):
    ui = [{
        "test_id": "title", "role": "label", "text": "Hello", "label": "Hello", "value": None, "rect_px": [10.0, 5.0, 60.0, 20.0],
        "clip_px": [0.0, 0.0, 135.0, 63.0], "visible": True, "elided": False, "enabled": True, "focused": False, "hidden": False,
        "layer": "Middle",
    }, {"test_id": "dup", "error": "the id appears twice"}]
    return {
        "schema_version": 1, "name": name, "run_id": rid, "snapshot_seq": 1, "phase": phase,
        "capture_id": "cap-1" if phase else None, "after_step": after_step, "after_label": after_label, "frame": frame,
        "settled": True, "app": {"name": "demo", "version": "0.1.0"},
        "theme": {"requested": "dark", "name": "dark", "dark": True, "tokens": {"bg": "#101010ff", "done": "#22aa44ff"},
                  "tokens_sha256": "c" * 64},
        "lang": {"requested": "ja", "in_force": "ja"},
        "window": {"inner_px": [0.0, 0.0, 135.0, 63.0], "pixels_per_point": 1.5, "monitor_work_area_px": None},
        "screen": "main", "overlay": None, "toasts": [], "app_state": {"n": 1}, "ui": ui,
        "capabilities": ["ui", "window", "theme", "lang", "screen", "overlay", "toasts", "app_state"],
        "stable": dict(STABLE),
    }


class Case:
    """One run folder under a temp repo, built the way the real tools would."""

    def __init__(self, st, name):
        self.st, self.name = st, name
        st.n += 1
        self.rid = "c%d-%s" % (st.n, re.sub(r"[^A-Za-z0-9_-]", "_", name))[:60]
        self.row = "r%d" % st.n
        self.run = os.path.join(st.ev, self.rid)

    def spec(self, tier=1):
        p = os.path.join(self.st.repo, "tests", "visual", self.row + ".toml")
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(SPEC_TEMPLATE.format(row=self.row, tier=tier))
        self.st.commit(p)
        self.spec_path = p
        return p

    def new(self, with_spec=True):
        args = ["new", "--evidence", self.st.ev, "--run-id", self.rid]
        if with_spec:
            args += ["--spec", self.spec_path]
        rc, out, e = self.st.call(args)
        self.st.expect(rc == 0, "%s: new rc %s %s" % (self.name, rc, e))
        return self

    def put(self, rel, data):
        p = os.path.join(self.run, *rel.split("/"))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "wb") as f:
            f.write(data if isinstance(data, bytes) else data.encode("utf-8"))
        return p

    def put_json(self, rel, obj):
        return self.put(rel, dumps(obj))

    def populate(self, status="done", failed=False, left=0, done_run=None, done_last="keys: done", trace=True, shots=True):
        rid = self.rid
        intent = loads(read_bytes(os.path.join(self.run, "pre", "intent.json")).decode()) if os.path.exists(
            os.path.join(self.run, "pre", "intent.json")) else {"written_ms": 0}
        t0 = intent["written_ms"] + 1000
        self.started = t0
        run = {
            "schema": 1, "runner": "ito-keyscript", "ito": {"version": "0.277.0", "rev": "7d28f32", "rev_source": "env"},
            "app": {"name": "demo", "version": "0.1.0"}, "run_id": rid, "os": "windows", "arch": "x86_64", "steps": 3,
            "started_ms": t0, "ended_ms": t0 + 500, "status": status, "failed": failed, "step": None,
            "why": "boom" if failed else None, "left": left,
        }
        self.put_json("run.json", run)
        self.put("keys.done", "run: %s\nkeys: ok\n%s\n" % (done_run or rid, done_last))
        if trace:
            labels = ["<Wait:0.1>", "<Snap:top>", "<State:hello>"]
            lines = []
            for i, lb in enumerate(labels, 1):
                lines.append(json.dumps({
                    "schema_version": 1, "run_id": rid, "step": i, "label": lb, "outcome": "done", "settled": True, "frame": 10 * i,
                    "time_ms": t0 + i, "end_frame": 10 * i + 1, "end_ms": t0 + i + 1, "why": None}, separators=(",", ":")))
            self.put("trace.jsonl", "\n".join(lines) + "\n")
        if shots:
            self.put_json("state/top-before.json", state_doc(rid, "top-before", "before", 10, 0, "<Wait:0.1>"))
            self.put_json("state/top-after.json", state_doc(rid, "top-after", "after", 13, 0, "<Wait:0.1>"))
            self.put_json("state/hello.json", state_doc(rid, "hello", None, 30, 1, "<Snap:top>"))
            png = png_bytes(135, 63)
            self.put("shots/top.png", png)
            ref = lambda n: {"path": "state/top-%s.json" % n, "sha256": sha256_bytes(read_bytes(os.path.join(  # noqa: E731
                self.run, "state", "top-%s.json" % n))), "stable": dict(STABLE)}
            self.put_json("shots/top.json", {
                "name": "top", "app": "demo", "version": "0.1.0", "theme": "dark", "theme_requested": "dark", "lang": "ja",
                "lang_requested": "ja", "size": [90, 42], "zoom": 1, "state": "", "component": "window", "rect": [0, 0, 90, 42],
                "time": 1731050000, "pixels_per_point": 1.5, "run_id": rid, "snapshot_seq": 1, "capture_id": "cap-1",
                "png": "top.png", "png_sha256": sha256_bytes(png), "size_px": [135, 63], "state_before": ref("before"),
                "state_after": ref("after"), "state_unchanged": True, "requested_frame": 11, "received_frame": 12,
                "env": {"os": "windows", "arch": "x86_64", "wgpu_backend": None, "font_set_sha256": None, "theme": "dark",
                        "pixels_per_point": 1.5},
            })
        return self

    def manifest(self, exit_code=0, killed=False, cleanup="PASS", extra=()):
        args = ["manifest", "--run", self.run, "--exit-code", str(exit_code), "--cleanup", cleanup, "--spec", self.spec_path]
        if killed:
            args.append("--killed")
        rc, out, e = self.st.call(args + list(extra))
        m, _ = read_json(os.path.join(self.run, "manifest.json"))
        self.mrc, self.mout = rc, out
        return rc, m

    def assertions(self, mod=None):
        spec, _ = read_spec(self.spec_path)
        sha = lambda rel: sha256_bytes(read_bytes(os.path.join(self.run, *rel.split("/"))))  # noqa: E731
        inp = lambda rels: [{"path": r, "sha256": sha(r)} for r in rels]  # noqa: E731
        raw = tomllib.loads(read_bytes(self.spec_path).decode())
        points = []
        for p in raw["point"]:
            rels = (["state/top-before.json", "state/top-after.json", "shots/top.json", "shots/top.png"] if "snap" in p
                    else ["state/hello.json"])
            points.append({
                "id": p["id"], "snap": p.get("snap"), "state": p.get("state"), "target": p.get("target"),
                "relative_to": p.get("relative_to"), "outcome": "PASS", "inputs": inp(rels),
                "checks": [{"check": k, "outcome": "PASS", "measured": 1, "expected": v, "why": None} for k, v in p["expect"].items()],
                "errors": [],
            })
        a = {
            "schema": "assertions.v1", "tool": {"name": "visual-check.py", "version": 1}, "row": spec["row"], "tier": spec["tier"],
            "spec": {"path": "tests/visual/%s.toml" % self.row, "sha256": sha256_bytes(read_bytes(self.spec_path))},
            "run_id": self.rid, "inputs": inp(["run.json", "keys.done", "trace.jsonl"]), "overall": "PASS", "points": points,
            "errors": [],
        }
        if mod:
            mod(a)
        self.put_json("machine/assertions.json", a)
        return a

    def verdict(self, replace=False, spec=None):
        args = ["verdict", "--run", self.run, "--spec", spec or self.spec_path]
        if replace:
            args.append("--replace")
        rc, out, e = self.st.call(args)
        v, _ = read_json(os.path.join(self.run, "verdict-%s.json" % self.row))
        return rc, v, out

    def emit(self, label):
        if self.st.emit:
            dst = os.path.join(self.st.emit, "evidence", "%02d-%s" % (self.st.n, re.sub(r"[^A-Za-z0-9_-]", "_", label)))
            shutil.copytree(self.run, dst, ignore=shutil.ignore_patterns("*.part", "*.png"))


class State:
    def __init__(self, tmp, emit):
        self.tmp, self.emit, self.n, self.failures = tmp, emit, 0, []
        self.repo = os.path.join(tmp, "app")
        self.ev = os.path.join(tmp, "evidence")
        os.makedirs(self.repo)
        self.git("init", "-q")

    def git(self, *args):
        p = subprocess.run(["git", "-C", self.repo, *args], capture_output=True, text=True)
        if p.returncode != 0:
            raise RuntimeError("git %s failed: %s" % (args, p.stderr))
        return p.stdout.strip()

    def commit(self, path):
        rel = os.path.relpath(path, self.repo).replace("\\", "/")
        self.git("add", "--", rel)
        self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "add " + rel)

    def call(self, argv):
        o, e = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(o), contextlib.redirect_stderr(e):
            rc = main(argv)
        return rc, o.getvalue(), e.getvalue()

    def expect(self, cond, msg):
        if not cond:
            self.failures.append(msg)

    def case(self, name, tier=1, populate=True, **kw):
        c = Case(self, name)
        c.spec(tier)
        c.new()
        if populate:
            c.populate(**kw)
        return c


def self_test(emit):
    if shutil.which("git") is None:
        print("evidence: self-test needs git on PATH (verify.sh and CI have it); install it")
        return 1
    global SCRIPT_DIR
    saved = (SCRIPT_DIR, os.getcwd(), {k: os.environ.get(k) for k in ("GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM")})
    tmp = tempfile.mkdtemp(prefix="evidence-selftest-")
    try:
        empty = os.path.join(tmp, "empty.gitconfig")
        open(empty, "w").close()
        os.environ["GIT_CONFIG_GLOBAL"] = empty
        os.environ["GIT_CONFIG_SYSTEM"] = empty
        os.environ.pop("GIT_CONFIG_NOSYSTEM", None)
        if emit:
            os.makedirs(emit, exist_ok=True)
        st = State(tmp, os.path.abspath(emit) if emit else None)
        st.git("config", "user.name", "t")
        st.git("config", "user.email", "t@t")
        st.git("config", "core.autocrlf", "false")
        os.makedirs(os.path.join(st.repo, "scripts"))
        SCRIPT_DIR = os.path.join(st.repo, "scripts")
        t_new(st)
        t_manifest(st)
        t_check(st)
        t_verdict(st)
    except Exception as e:  # a bug in the test itself still has to fail loudly
        import traceback

        st_fail = "self-test crashed: %s\n%s" % (e, traceback.format_exc())
        print(st_fail)
        return 1
    finally:
        SCRIPT_DIR = saved[0]
        os.chdir(saved[1])
        for k, v in saved[2].items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v
        shutil.rmtree(tmp, ignore_errors=True)
    if st.failures:
        for f in st.failures:
            print("FAIL: " + f)
        print("evidence: self-test FAILED (%d)" % len(st.failures))
        return 1
    print("evidence: self-test OK")
    return 0


def t_new(st):
    """Canary 1."""
    ex = st.expect
    spec = Case(st, "new").spec()
    ev = os.path.join(st.tmp, "ev-new")
    # generated id from a label, spec bound into pre/intent.json
    rc, out, e = st.call(["new", "--evidence", ev, "--label", "41.15", "--spec", spec])
    ex(rc == 0, "new --label rc %s %s" % (rc, e))
    m = re.search(r"^run_id=(\d{8}-\d{6}-41_15-[0-9a-f]{4})$", out, re.M)
    ex(m is not None, "generated id shape: %r" % out)
    if m:
        d = os.path.join(ev, m.group(1))
        ex(os.path.isdir(d) and ("dir=" + os.path.abspath(d)) in out.replace("\\", "\\"), "dir= line: %r" % out)
        ex(read_bytes(os.path.join(d, ".keys-run")) == (MARKER_TEXT + "\n").encode(), ".keys-run text")
        it, _ = read_json(os.path.join(d, "pre", "intent.json"))
        s = it["specs"][0] if it and it.get("specs") else {}
        ex(s.get("path") == "tests/visual/%s.toml" % os.path.basename(spec)[:-5], "intent path %r" % (s.get("path"),))
        ex(s.get("sha256") == sha256_bytes(read_bytes(spec)) and OID_RE.match(s.get("blob", "")) and OID_RE.match(s.get("head", "")),
           "intent sha256 / blob / head")
        ex(it["run_id"] == m.group(1) and is_int(it["written_ms"]) and it["schema"] == "intent.v1", "intent header")
    # a long label is cut to 64
    rc, out, _ = st.call(["new", "--evidence", ev, "--label", "x" * 100])
    mm = re.search(r"^run_id=(\S+)$", out, re.M)
    ex(rc == 0 and mm and len(mm.group(1)) == 64, "long label cut to 64: %r" % out)
    # no --spec: an empty specs list; an existing folder is never reused; bad ids
    rc, out, _ = st.call(["new", "--evidence", ev, "--run-id", "fixed-1"])
    it, _ = read_json(os.path.join(ev, "fixed-1", "pre", "intent.json"))
    ex(rc == 0 and it and it["specs"] == [], "no --spec -> empty specs list")
    rc, _, _ = st.call(["new", "--evidence", ev, "--run-id", "fixed-1"])
    ex(rc == 2, "existing folder refused, rc %s" % rc)
    for bad in ("a.b", "", "x" * 65, "a/b", "a b"):
        rc, _, _ = st.call(["new", "--evidence", ev, "--run-id", bad])
        ex(rc == 2, "bad run id %r -> rc %s" % (bad, rc))
    rc, _, _ = st.call(["new", "--run-id", "no-parent"])
    ex(rc == 2, "no evidence folder -> rc %s" % rc)
    # lanes.conf: env= line (LF and CRLF), relative evidence= under the repo root, not the cwd
    conf = os.path.join(SCRIPT_DIR, "lanes.conf")
    for eol in ("\n", "\r\n"):
        with open(conf, "w", newline="") as f:
            f.write("bin=demo%skeys_env=KURA_KEYS_DONE%sevidence=ev-rel%s" % (eol, eol, eol))
        os.chdir(st.tmp)
        rc, out, e = st.call(["new", "--run-id", "conf-%d" % len(eol)])
        os.chdir(st.tmp)
        ex(rc == 0 and "env=KURA_KEYS_RUN_ID" in out.split("\n"), "env= line (eol %r): %r %s" % (eol, out, e))
        ex(os.path.isdir(os.path.join(st.repo, "ev-rel", "conf-%d" % len(eol))), "relative evidence= is under the repo root")
        ex(not os.path.exists(os.path.join(st.tmp, "ev-rel")), "relative evidence= must not use the cwd")
    # a lanes.conf path written for the other OS is refused before anything is created; an explicit --evidence is not checked
    for name, cases in (
        ("posix", (("C:\\ev", 1), ("C:ev", 1), ("\\\\host\\share\\ev", 1), ("\\ev", 1), ("ev\\sub", 1),
                   ("/srv/ev", 0), ("//host/ev", 0), ("ev", 0), ("ev/sub", 0), ("./ev", 0))),
        ("nt", (("/srv/ev", 1), ("//host/share/ev", 0), ("C:\\ev", 0), ("C:/ev", 0), ("\\\\host\\share\\ev", 0), ("ev", 0),
                ("ev\\sub", 0))),
    ):
        for p, want in cases:
            ex(foreign_path(p, name) == bool(want), "foreign_path(%r, %s) should be %s" % (p, name, bool(want)))
    # On Windows a rooted path with no drive, inside the temp tree: if the refusal ever breaks, the folder still lands there.
    foreign = os.path.splitdrive(st.tmp)[1].replace("\\", "/") + "/ev-foreign" if os.name == "nt" else "C:\\ev-foreign"
    with open(conf, "w", newline="") as f:
        f.write("evidence=%s\n" % foreign)
    before = sorted(os.listdir(st.repo))
    rc, _, e = st.call(["new", "--run-id", "x"])
    ex(rc == 2 and "another OS" in e, "foreign evidence= is refused: rc %s %s" % (rc, e))
    ex(sorted(os.listdir(st.repo)) == before and not os.path.exists(foreign), "a foreign evidence= creates nothing")
    ev3 = os.path.join(st.tmp, "ev-explicit")
    rc, _, e = st.call(["new", "--evidence", ev3, "--run-id", "x"])
    ex(rc == 0 and os.path.isdir(os.path.join(ev3, "x")), "explicit --evidence beats a foreign evidence=: rc %s %s" % (rc, e))
    os.remove(conf)
    # .keys-run text equals keys.ps1's when keys.ps1 sits beside the real script
    kp = os.path.join(os.path.dirname(os.path.abspath(__file__)), "keys.ps1")
    if os.path.isfile(kp):
        mk = re.search(r'Set-Content -LiteralPath \$marker -Value "([^"]*)"', read_bytes(kp).decode("utf-8"))
        ex(mk is not None and mk.group(1) == MARKER_TEXT, "keys.ps1 .keys-run text differs from MARKER_TEXT")
    # spec gates: untracked, unstaged edit, staged edit -> rc 2 and no folder
    ev2 = os.path.join(st.tmp, "ev-gate")
    p = os.path.join(st.repo, "tests", "visual", "untracked.toml")
    with open(p, "w") as f:
        f.write("spec = 1\n")
    rc, _, e = st.call(["new", "--evidence", ev2, "--run-id", "g1", "--spec", p])
    ex(rc == 2 and "not tracked" in e and not os.path.exists(os.path.join(ev2, "g1")), "untracked spec: rc %s %s" % (rc, e))
    rc, _, e = st.call(["new", "--evidence", ev2, "--run-id", "g1", "--spec", os.path.join(st.repo, "nope.toml")])
    ex(rc == 2, "missing spec -> rc %s" % rc)
    with open(spec, "a") as f:
        f.write("# edit\n")
    rc, _, e = st.call(["new", "--evidence", ev2, "--run-id", "g2", "--spec", spec])
    ex(rc == 2 and "not committed" in e and not os.path.exists(os.path.join(ev2, "g2")), "unstaged edit: rc %s %s" % (rc, e))
    st.git("add", "--", os.path.relpath(spec, st.repo))
    rc, _, e = st.call(["new", "--evidence", ev2, "--run-id", "g3", "--spec", spec])
    ex(rc == 2 and "not committed" in e and not os.path.exists(os.path.join(ev2, "g3")), "staged edit: rc %s %s" % (rc, e))
    st.commit(spec)
    rc, _, e = st.call(["new", "--evidence", ev2, "--run-id", "g4", "--spec", spec])
    ex(rc == 0, "committed spec passes: %s" % e)
    t_new_scale(st)


def t_new_scale(st):
    """scale_env= in lanes.conf: the lane sets the app's scale from the spec, and every refusal comes before the run folder."""
    ex = st.expect
    conf = os.path.join(SCRIPT_DIR, "lanes.conf")
    nspec = [0]

    def mkspec(ppp, text=None):
        nspec[0] += 1
        p = os.path.join(st.repo, "tests", "visual", "scale%d.toml" % nspec[0])
        body = text if text is not None else SPEC_TEMPLATE.format(row="sc%d" % nspec[0], tier=1).replace(
            "pixels_per_point = 1.5", "pixels_per_point = %s" % ppp)
        with open(p, "w", encoding="utf-8", newline="") as f:
            f.write(body)
        st.commit(p)
        return p

    def lane(conf_text, specs, tag):
        with open(conf, "w", newline="") as f:
            f.write(conf_text)
        ev = os.path.join(st.tmp, "ev-scale-" + tag)
        args = ["new", "--evidence", ev, "--run-id", "r-" + tag]
        for s in specs:
            args += ["--spec", s]
        rc, out, e = st.call(args)
        made = os.path.isdir(ev) and bool(os.listdir(ev))
        return rc, out.split("\n"), e, made

    base = "bin=demo\nkeys_env=DEMO_KEYS_DONE\nisolate=DEMO_HOME DEMO_CFG\n"
    s15, s15b, s2, s20 = mkspec("1.5"), mkspec("1.5"), mkspec("2"), mkspec("2.0")
    try:
        # with the key and a spec: both lines, the first spec's number as Python prints it
        rc, out, e, made = lane(base + "scale_env=DEMO_SCALE\n", [s15], "ok")
        ex(rc == 0 and made and "scale_env=DEMO_SCALE" in out and "scale=1.5" in out, "scale_env + spec prints both lines: %s %s %s" % (rc, out, e))
        rc, out, e, _ = lane(base + "scale_env=DEMO_SCALE\n", [s2], "int")
        ex(rc == 0 and "scale=2" in out, "an integer scale prints as 2: %s %s" % (out, e))
        # without the key: neither line, and a spec is not even read (a spec read_spec rejects still gets its run)
        rc, out, e, _ = lane(base, [s15], "nokey")
        ex(rc == 0 and not any(l.startswith(("scale_env=", "scale=")) for l in out), "no scale_env -> no scale lines: %s" % out)
        bad = mkspec(None, text="spec = 1\nrow = \"x\"\n")
        rc, out, e, made = lane(base, [bad], "nokey-bad")
        ex(rc == 0 and made, "no scale_env -> the spec is not read by new: %s %s" % (rc, e))
        # with the key and no spec: no lines (nothing to take the scale from)
        rc, out, e, _ = lane(base + "scale_env=DEMO_SCALE\n", [], "nospec")
        ex(rc == 0 and not any(l.startswith(("scale_env=", "scale=")) for l in out), "scale_env without a spec -> no scale lines: %s" % out)
        # specs that agree (2 and 2.0 are equal) are accepted; the first one's text is printed
        rc, out, e, _ = lane(base + "scale_env=DEMO_SCALE\n", [s15, s15b], "agree")
        ex(rc == 0 and "scale=1.5" in out, "two specs at 1.5: %s %s" % (out, e))
        rc, out, e, _ = lane(base + "scale_env=DEMO_SCALE\n", [s2, s20], "agree2")
        ex(rc == 0 and "scale=2" in out, "2 and 2.0 agree: %s %s" % (out, e))
        # specs that disagree: refused, names both specs and values, no folder
        rc, out, e, made = lane(base + "scale_env=DEMO_SCALE\n", [s15, s2], "disagree")
        ex(rc == 2 and not made and "disagree" in e and os.path.basename(s15) in e and os.path.basename(s2) in e and "1.5" in e,
           "disagreeing specs are refused with no folder: rc %s made %s %s" % (rc, made, e))
        # a spec read_spec rejects is refused when the key is set
        rc, out, e, made = lane(base + "scale_env=DEMO_SCALE\n", [bad], "badspec")
        ex(rc == 2 and not made and os.path.basename(bad) in e, "a rejected spec is refused with scale_env: rc %s made %s %s" % (rc, made, e))
        # a bad name
        for i, name in enumerate(("demo_scale", "1SCALE", "A-B", "A B", "A=B")):
            rc, out, e, made = lane(base + "scale_env=%s\n" % name, [s15], "badname%d" % i)
            ex(rc == 2 and not made, "scale_env=%r is refused with no folder: rc %s made %s" % (name, rc, made))
        # the same variable as one the lane sets, in any letter case
        for i, name in enumerate(("DEMO_KEYS_DONE", "DEMO_KEYS_RUN_ID", "DEMO_HOME", "DEMO_CFG")):
            rc, out, e, made = lane(base + "scale_env=%s\n" % name, [s15], "clash%d" % i)
            ex(rc == 2 and not made and "same variable" in e, "scale_env=%s clashes: rc %s made %s %s" % (name, rc, made, e))
        # the other letter case: the conf names are lower or mixed case, the scale name (upper case only) still clashes
        lower = "bin=demo\nkeys_env=demo_keys_DONE\nisolate=Demo_Home demo_cfg\n"
        for i, name in enumerate(("DEMO_KEYS_DONE", "DEMO_KEYS_RUN_ID", "DEMO_HOME", "DEMO_CFG")):
            rc, out, e, made = lane(lower + "scale_env=%s\n" % name, [s15], "clashcase%d" % i)
            ex(rc == 2 and not made and "same variable" in e, "scale_env=%s clashes in another case: rc %s made %s %s" % (name, rc, made, e))
        # a keys_env without _DONE has no run-id variable, so that name is free
        rc, out, e, _ = lane("bin=demo\nkeys_env=DEMO_KEYS\nscale_env=DEMO_KEYS_RUN_ID\n", [s15], "norunid")
        ex(rc == 0, "keys_env without _DONE has no run-id variable to clash with: rc %s %s" % (rc, e))
    finally:
        if os.path.exists(conf):
            os.remove(conf)


def t_manifest(st):
    """Canary 2."""
    ex = st.expect
    c = st.case("clean")
    rc, m = c.manifest()
    paths = [f["path"] for f in m["files"]]
    ex(rc == 0 and m["problems"] == [], "clean folder: rc %s %s" % (rc, m and m["problems"]))
    ex(paths == sorted(paths) and "manifest.json" not in paths, "files sorted, manifest not listed: %s" % paths)
    ex(".keys-run" in paths and "pre/intent.json" in paths, ".keys-run and pre/intent.json are listed")
    ex(m["run_id"] == c.rid and m["status"] == "done" and m["app"] == {"name": "demo", "version": "0.1.0", "commit": None}
       and m["keys_done"] == {"run": c.rid, "last": "keys: done"} and m["started_ms"] == c.started, "fields from run.json / keys.done")
    ex(len(m["envs"]) == 1 and m["envs"][0]["theme"] == "dark", "envs from shots/*.json: %s" % m["envs"])
    ex(m["specs"] == [{"path": "tests/visual/%s.toml" % c.row, "sha256": sha256_bytes(read_bytes(c.spec_path))}], "specs entry")
    c.emit("clean")
    ex(m["scale_override"] is None, "no --scale-* -> scale_override null: %r" % (m.get("scale_override", "absent"),))

    # scale override: all three arguments are recorded as the text handed to the app; a partial set is a problem and records null
    for src in ("spec", "environment"):
        cs = st.case("scale-" + src)
        rc, ms = cs.manifest(extra=["--scale-env", "DEMO_SCALE", "--scale-value=1.5", "--scale-source", src])
        ex(rc == 0 and ms["problems"] == [] and ms["scale_override"] == {"env": "DEMO_SCALE", "value": "1.5", "source": src},
           "scale_override %s: rc %s %s" % (src, rc, ms and (ms["scale_override"], ms["problems"])))
    cs = st.case("scale-dash")
    rc, ms = cs.manifest(extra=["--scale-env", "DEMO_SCALE", "--scale-value=-1", "--scale-source", "spec"])
    ex(rc == 0 and ms["scale_override"] == {"env": "DEMO_SCALE", "value": "-1", "source": "spec"}, "a value starting with - is one token: rc %s" % rc)
    for i, part in enumerate((["--scale-env", "DEMO_SCALE"], ["--scale-value=1.5"], ["--scale-source", "spec"],
                              ["--scale-env", "DEMO_SCALE", "--scale-value=1.5"])):
        cs = st.case("scale-partial%d" % i)
        rc, ms = cs.manifest(extra=part)
        ex(rc == 1 and ms["scale_override"] is None and any("scale override" in p for p in ms["problems"]),
           "partial scale set %s -> null and a problem: rc %s %s" % (part, rc, ms and (ms["scale_override"], ms["problems"])))
    # an old manifest (no scale_override) is still read back by check
    co = st.case("scale-old")
    rc, mo = co.manifest()
    del mo["scale_override"]
    co.put_json("manifest.json", mo)
    ro, mp = check_run(co.run)
    ex(ro is not None and not any("scale_override" in p for p in mp), "check does not require scale_override: %s" % (mp,))

    def bad(name, want, tier=1, populate=None, after=None, **mk):
        cc = st.case(name, populate=False)
        cc.populate(**(populate or {}))
        if after:
            after(cc)
        rc, m = cc.manifest(**mk)
        ex(rc == 1 and m is not None and any(want in p for p in m["problems"]), "%s: rc %s problems %s" % (name, rc, m and m["problems"]))
        ex(os.path.isfile(os.path.join(cc.run, "manifest.json")), "%s: manifest still written" % name)
        cc.emit(name)

    bad("status-failed", "status 'failed'", populate={"status": "failed", "failed": True})
    bad("quit-left2", "left is not 0", populate={"status": "quit", "left": 2})
    bad("keys-other-run", "keys.done run", populate={"done_run": "other"})
    bad("keys-not-done", "last line", populate={"done_last": "keys: failed"})
    bad("no-trace", "trace.jsonl missing", populate={"trace": False})
    bad("no-intent", "pre/intent.json missing", after=lambda cc: os.remove(os.path.join(cc.run, "pre", "intent.json")))
    bad("exit1", "exit code 1", exit_code=1)
    bad("killed", "killed", killed=True)
    bad("cleanup-fail", "cleanup FAIL", cleanup="FAIL")
    bad("blocked", "blocked.txt", after=lambda cc: cc.put("blocked.txt", "dialog"))
    bad("stray-part", "partial file present: shots/x.png.part", after=lambda cc: cc.put("shots/x.png.part", "x"))
    bad("no-run-json", "run.json missing", after=lambda cc: os.remove(os.path.join(cc.run, "run.json")))
    bad("missing-scenario", "scenario missing", extra=("--scenario", os.path.join(st.repo, "tests", "nope.md")))
    cc = st.case("closed", populate=False)
    cc.populate(status="closed", failed=False, left=0)
    rc, m = cc.manifest()
    ex(rc == 0 and m["problems"] == [], "status closed is fine: %s" % (m and m["problems"]))
    rc, _, _ = st.call(["manifest", "--run", os.path.join(st.tmp, "none"), "--exit-code", "0", "--cleanup", "PASS"])
    ex(rc == 2, "manifest on a missing folder -> rc %s" % rc)
    # a link inside the folder is a problem and not followed
    cc = st.case("link", populate=True)
    target = os.path.join(st.tmp, "outside")
    os.makedirs(target, exist_ok=True)
    try:
        os.symlink(target, os.path.join(cc.run, "isolate"), target_is_directory=True)
        linked = True
    except (OSError, NotImplementedError):
        linked = False
    if linked:
        rc, m = cc.manifest()
        ex(rc == 1 and any("not followed: isolate" in p for p in m["problems"]), "symlink is a problem: %s" % (m and m["problems"]))
        os.unlink(os.path.join(cc.run, "isolate"))


def t_check(st):
    """Canary 3."""
    ex = st.expect
    c = st.case("check")
    c.manifest()
    rc, _, _ = st.call(["check", "--run", c.run])
    ex(rc == 0, "intact -> rc %s" % rc)
    p = os.path.join(c.run, "run.json")
    original = read_bytes(p)
    with open(p, "wb") as f:
        f.write(original[:-2] + b"X\n")
    rc, out, _ = st.call(["check", "--run", c.run])
    ex(rc == 1 and "changed: run.json" in out, "one byte changed -> rc %s %s" % (rc, out))
    with open(p, "wb") as f:
        f.write(original)
    c.put("stray.txt", "x")
    rc, out, _ = st.call(["check", "--run", c.run])
    ex(rc == 1 and "not in the manifest: stray.txt" in out, "unlisted top-level file -> rc %s %s" % (rc, out))
    os.remove(os.path.join(c.run, "stray.txt"))
    c.put_json("machine/assertions.json", {})
    c.put("verdict-x.json", "{}")
    rc, out, _ = st.call(["check", "--run", c.run])
    ex(rc == 0, "machine/ and verdict-*.json may be added -> rc %s %s" % (rc, out))
    os.remove(os.path.join(c.run, "shots", "top.png"))
    rc, out, _ = st.call(["check", "--run", c.run])
    ex(rc == 1 and "missing: shots/top.png" in out, "listed file missing -> rc %s" % rc)
    rc, _, _ = st.call(["check", "--run", os.path.join(c.run, "shots")])
    ex(rc == 2, "no manifest -> rc %s" % rc)
    with open(os.path.join(c.run, "manifest.json"), "w") as f:
        f.write("{not json")
    rc, _, _ = st.call(["check", "--run", c.run])
    ex(rc == 2, "unreadable manifest -> rc %s" % rc)


def t_verdict(st):
    """Canary 4."""
    ex = st.expect

    def run_case(name, tier=1, mod=None, before_manifest=None, after_manifest=None, manifest_kw=None, populate=None, expect=None,
                 rc_want=None, build_assertions=True):
        c = st.case(name, tier=tier, populate=False)
        c.populate(**(populate or {}))
        if before_manifest:
            before_manifest(c)
        c.manifest(**(manifest_kw or {}))
        if build_assertions:
            c.assertions(mod)
        if after_manifest:
            after_manifest(c)
        rc, v, out = c.verdict()
        ex(v is not None and v["result"] == expect and (rc_want is None or rc == rc_want),
           "%s: want %s got %s (rc %s) %s" % (name, expect, v and v["result"], rc, v and v["reasons"]))
        c.emit(name)
        return c, rc, v

    c, rc, v = run_case("candidate", expect="auto-pass-candidate", rc_want=0)
    if v:
        ex(v["reasons"] == [] and v["mark"] is None and v["decided_by"] is None, "candidate: no reasons, mark/decided_by null")
        ex(v["conditions"]["screenshot"] is True and v["conditions"]["vision_points"] == "n/a" and v["evidence"] ==
           ["machine/assertions.json#thumb-top", "machine/assertions.json#title"], "candidate conditions / evidence: %s" % v["conditions"])
        ex(all(v["conditions"][k] is True for k in ("expected_defined", "assertions_match", "state_json", "same_run", "manifest_intact")),
           "candidate conditions true")
    # an existing verdict is not overwritten without --replace
    rc, _, _ = c.verdict()
    ex(rc == 2, "existing verdict without --replace -> rc %s" % rc)
    rc, v2, _ = c.verdict(replace=True)
    ex(rc == 0, "--replace -> rc %s" % rc)

    def fail_mod(a):
        a["overall"] = "FAIL"
        a["points"][0]["outcome"] = "FAIL"
        a["points"][0]["checks"][0]["outcome"] = "FAIL"

    run_case("assertions-fail", mod=fail_mod, expect="fail", rc_want=1)

    def err_mod(a):
        a["overall"] = "ERROR"
        a["points"][0]["outcome"] = "ERROR"
        a["points"][0]["checks"][0]["outcome"] = "ERROR"

    run_case("assertions-error", mod=err_mod, expect="undecidable", rc_want=3)
    run_case("tier2", tier=2, expect="undecidable", rc_want=3)
    run_case("tier3", tier=3, expect="owner", rc_want=3)
    run_case("manifest-tampered", after_manifest=lambda c: c.put("run.json", read_bytes(os.path.join(c.run, "run.json")) + b" "),
             expect="undecidable")
    run_case("no-assertions-file-is-exit-2", build_assertions=False, expect=None, rc_want=2) if False else None
    c = st.case("no-assertions", populate=True)
    c.manifest()
    rc, v, _ = c.verdict()
    ex(rc == 2 and v is None, "no assertions -> rc %s" % rc)
    c = st.case("no-manifest", populate=True)
    c.assertions()
    rc, v, _ = c.verdict()
    ex(rc == 2 and v is None, "no manifest -> rc %s" % rc)
    # intent
    def edit_spec(c):
        with open(c.spec_path, "a", newline="") as f:
            f.write("# edited after new\n")

    run_case("spec-edited", after_manifest=edit_spec, expect="undecidable")

    def edit_commit_rewrite_intent(c):
        # the spec edited and committed, the intent rewritten with the new sha but the old head
        with open(c.spec_path, "a", newline="") as f:
            f.write("# edited\n")
        st.commit(c.spec_path)
        ip = os.path.join(c.run, "pre", "intent.json")
        it = loads(read_bytes(ip).decode())
        it["specs"][0]["sha256"] = sha256_bytes(read_bytes(c.spec_path))
        c.put_json("pre/intent.json", it)

    def edit_commit_rewrite_all(c):
        # intent, manifest-listed? the manifest would no longer hold: rebuild it so only the blob identity can catch it
        edit_commit_rewrite_intent(c)
        c.manifest()
        c.assertions()

    c = st.case("blob-differs", populate=True)
    c.manifest()
    edit_commit_rewrite_all(c)
    rc, v, _ = c.verdict()
    ex(v is not None and v["result"] == "undecidable" and not v["conditions"]["expected_defined"]
       and "blob" in " ".join(v["reasons"]), "intent with new sha, old head: %s %s" % (v and v["result"], v and v["reasons"]))
    c.emit("blob-differs")

    def late_intent(c):
        ip = os.path.join(c.run, "pre", "intent.json")
        it = loads(read_bytes(ip).decode())
        it["written_ms"] = c.started + 10_000
        c.put_json("pre/intent.json", it)

    run_case("intent-after-start", before_manifest=late_intent, expect="undecidable")
    run_case("no-intent", before_manifest=lambda c: os.remove(os.path.join(c.run, "pre", "intent.json")), expect="undecidable")

    def spec_not_in_intent(c):
        c.put_json("pre/intent.json", {"schema": "intent.v1", "tool": TOOL, "run_id": c.rid, "written_ms": 1, "specs": []})

    run_case("spec-not-in-intent", before_manifest=spec_not_in_intent, expect="undecidable")

    # assertions against the spec
    def drop_point(a):
        del a["points"][1]

    def extra_point(a):
        a["points"].append(dict(a["points"][1], id="extra"))

    def drop_check(a):
        a["points"][0]["checks"].pop()

    def outcome_over_fail(a):
        a["points"][0]["checks"][0]["outcome"] = "FAIL"

    def overall_over_fail(a):
        a["points"][0]["outcome"] = "FAIL"
        a["points"][0]["checks"][0]["outcome"] = "FAIL"

    def other_row(a):
        a["row"] = "another"

    def reordered(a):
        a["points"].reverse()

    def dup_check(a):
        a["points"][0]["checks"].append(dict(a["points"][0]["checks"][0]))

    def spec_sha(a):
        a["spec"]["sha256"] = "0" * 64

    def input_off(a):
        a["points"][1]["inputs"][0]["sha256"] = "0" * 64

    def other_run(a):
        a["run_id"] = "someone-else"

    for nm, f in (("point-dropped", drop_point), ("extra-point", extra_point), ("check-dropped", drop_check),
                  ("outcome-pass-over-fail", outcome_over_fail), ("overall-pass-over-fail", overall_over_fail),
                  ("other-row", other_row), ("points-reordered", reordered), ("check-duplicated", dup_check),
                  ("spec-sha", spec_sha), ("input-sha-off", input_off), ("other-run-id", other_run)):
        run_case(nm, mod=f, expect="undecidable")
    # an assertions point with a point-level error counts as ERROR, not as a pass
    def point_error(a):
        a["points"][1]["errors"] = ["boom"]
        a["points"][1]["outcome"] = "ERROR"
        a["overall"] = "ERROR"

    run_case("point-error", mod=point_error, expect="undecidable")
    # the run's own problems keep a candidate from being one
    run_case("cleanup-fail", manifest_kw={"cleanup": "FAIL"}, expect="undecidable")
    run_case("exit-code-1", manifest_kw={"exit_code": 1}, expect="undecidable")
    # a bad spec is exit 2
    c = st.case("bad-spec", populate=True)
    c.manifest()
    c.assertions()
    bad = os.path.join(st.repo, "bad.toml")
    with open(bad, "w") as f:
        f.write('spec = 1\nrow = "x"\ntier = 1\nbogus = 1\n')
    rc, v, _ = c.verdict(spec=bad)
    ex(rc == 2 and v is None, "bad spec -> rc %s" % rc)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
