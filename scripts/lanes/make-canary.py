#!/usr/bin/env python3
"""make-canary.py: copy a real visual-check run, make the measured value wrong, and check that the checker says FAIL.

    python3 scripts/make-canary.py --run <run dir> --spec <spec.toml> [--canaries <file.toml>] <kind> [<kind> ...]
    python3 scripts/make-canary.py --self-test

A canary is the same run with the evidence rewritten so that one measured value is wrong. No product code changes.
visual-check.py has to answer FAIL (exit 1). PASS means the check is blind to the change. ERROR means the rewrite broke the
evidence instead of the value. Both fail the canary. Each copy goes to <run>/canary/<kind>/<run id>/ (evidence.py leaves
`canary/` unlisted, and the checker wants the folder named after the run).

The kinds are data, not code. They live in a file beside the spec, `<spec without .toml>.canary.toml` by default, so that
adding a canary later does not move the spec's SHA (a spec is committed before its run):

    [[canary]]
    name = "thumb-gone"                 # the kind: ^[a-z0-9][a-z0-9._-]*$

    [[canary.edit]]                     # one or more, applied in the order written
    snap = "top"                        # a <Snap:> name (state/<n>-before.json and -after.json, both the same way),
    op = "drop"                         # or `state = "palette"` for a plain <State:> (state/<n>.json): exactly one
    target = "spot.scroll.thumb"

    op         keys                                  effect
    drop       target                                remove the ui[] entry whose test_id is target
    shift      target, dx, dy                        rect_px moves by (dx, dy)
    set_text   target, value                         set `text`, and `label` too if the entry has the key
    copy_from  target, from                          append the entry from state/<from>.json (of the original run) to ui[]
    set        path, value | clear = true            set overlay.* / app_state.* (the final key must exist); clear = null
    paint      snap, rect | target, token | rgb      fill that rectangle of shots/<snap>.png with an opaque colour

Exit 0: every kind failed as it should. 1: at least one did not. 2: a usage or data problem (nothing is copied).
A problem in the run's contents (a missing target, a path that does not exist, ...) fails that kind only, creates no folder
for it, and the other kinds still run. Stdlib only, Python 3.11+.
This file is copied byte for byte into every app by `lanes.sh sync`: it reads nothing of ito's and depends on no layout
except the sibling visual-check.py.
"""
import sys

if sys.version_info < (3, 11):
    sys.stderr.write("make-canary.py: Python 3.11 or newer is required (this is %d.%d)\n" % sys.version_info[:2])
    sys.exit(2)

import argparse
import contextlib
import copy
import hashlib
import importlib.util
import io
import json
import math
import os
import re
import shutil
import subprocess
import tempfile
import tomllib
from collections import namedtuple

HERE = os.path.dirname(os.path.abspath(__file__))
CHECKER = os.path.join(HERE, "visual-check.py")
SKIP_AT_ROOT = ("canary", "machine", "vision", "crops")
KIND_RE = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
STEM_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9_.-]*$")
RGB_RE = re.compile(r"^[0-9a-fA-F]{6}$")
# op -> (keys it takes, keys it needs); `snap`/`state` and `op` are checked apart from these
OPS = {
    "drop": ({"target"}, {"target"}),
    "shift": ({"target", "dx", "dy"}, {"target", "dx", "dy"}),
    "set_text": ({"target", "value"}, {"target", "value"}),
    "copy_from": ({"target", "from"}, {"target", "from"}),
    "set": ({"path", "value", "clear"}, {"path"}),
    "paint": ({"rect", "target", "token", "rgb"}, set()),
}
EDIT_KEYS = {"snap", "state", "op", "target", "dx", "dy", "value", "from", "path", "clear", "rect", "token", "rgb"}
CANARY_KEYS = {"name", "edit"}
SET_ROOTS = ("overlay", "app_state")

Result = namedtuple("Result", "ok lines dest")


class DataError(Exception):
    """The command line or the data file is wrong: exit 2 before anything is copied."""


class KindError(Exception):
    """The run's contents do not fit this kind: the kind fails, nothing is created for it."""


def load_checker():
    """The sibling visual-check.py as a module (its file name has a hyphen), for decode_png and encode_png."""
    if not os.path.isfile(CHECKER):
        raise DataError("the sibling visual-check.py is missing (looked for %s)" % CHECKER)
    sys.dont_write_bytecode = True  # this file is copied into every app: leave no __pycache__ beside it
    spec = importlib.util.spec_from_file_location("visual_check", CHECKER)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


# ---------------------------------------------------------------------------------------------------------------------
# The data file
# ---------------------------------------------------------------------------------------------------------------------


def is_num(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def check_edit(where, ed):
    if not isinstance(ed, dict):
        raise DataError("%s: an edit must be a table" % where)
    unknown = sorted(set(ed) - EDIT_KEYS)
    if unknown:
        raise DataError("%s: unknown key %s" % (where, ", ".join(unknown)))
    op = ed.get("op")
    if not isinstance(op, str) or op not in OPS:
        raise DataError("%s: unknown op %r (known: %s)" % (where, op, ", ".join(OPS)))
    if ("snap" in ed) == ("state" in ed):
        raise DataError("%s: exactly one of `snap` and `state` is required" % where)
    which = "snap" if "snap" in ed else "state"
    if op == "paint" and which != "snap":
        raise DataError("%s: paint works on a picture, so it takes `snap`, not `state`" % where)
    takes, needs = OPS[op]
    for key in ed:
        if key not in takes and key not in ("op", "snap", "state"):
            raise DataError("%s: op %s does not take `%s`" % (where, op, key))
    for key in sorted(needs):
        if key not in ed:
            raise DataError("%s: op %s needs `%s`" % (where, op, key))
    names = [(which, ed[which])] + ([("from", ed["from"])] if "from" in ed else [])
    for key, v in names:
        if not isinstance(v, str) or not STEM_RE.match(v):
            raise DataError("%s: `%s` %r is not a plain file stem" % (where, key, v))
    for key in ("target", "path", "token"):
        if key in ed and (not isinstance(ed[key], str) or not ed[key]):
            raise DataError("%s: `%s` must be a non-empty string" % (where, key))
    if op == "shift":
        for key in ("dx", "dy"):
            if not is_num(ed[key]):
                raise DataError("%s: `%s` must be a finite number (not a boolean)" % (where, key))
    if op == "set_text" and not isinstance(ed["value"], str):
        raise DataError("%s: `value` must be a string" % where)
    if op == "set":
        segs = ed["path"].split(".")
        if any(not s for s in segs):
            raise DataError("%s: `path` %r has an empty segment" % (where, ed["path"]))
        if segs[0] not in SET_ROOTS:
            raise DataError("%s: `path` must start with %s, not %r" % (where, " or ".join(SET_ROOTS), segs[0]))
        if ("value" in ed) == ("clear" in ed):
            raise DataError("%s: set needs exactly one of `value` and `clear = true`" % where)
        if "clear" in ed and ed["clear"] is not True:
            raise DataError("%s: `clear` can only be true" % where)
        if "value" in ed:
            v = ed["value"]
            if type(v) not in (str, bool, int, float):
                raise DataError("%s: `value` must be a string, boolean, integer or float (not %s)" % (where, type(v).__name__))
            if isinstance(v, float) and not math.isfinite(v):
                raise DataError("%s: `value` %r has no JSON form" % (where, v))
    if op == "paint":
        if ("rect" in ed) == ("target" in ed):
            raise DataError("%s: paint needs exactly one of `rect` and `target`" % where)
        if ("token" in ed) == ("rgb" in ed):
            raise DataError("%s: paint needs exactly one of `token` and `rgb`" % where)
        if "rect" in ed:
            r = ed["rect"]
            if not isinstance(r, list) or len(r) != 4 or not all(is_num(x) for x in r):
                raise DataError("%s: `rect` must be [x0, y0, x1, y1], four finite numbers" % where)
        if "rgb" in ed and (not isinstance(ed["rgb"], str) or not RGB_RE.match(ed["rgb"])):
            raise DataError("%s: `rgb` must be exactly six hex digits" % where)


def load_canaries(path):
    """-> {kind: [edit, ...]}. The whole file is validated, so a typo anywhere stops everything."""
    try:
        with open(path, "rb") as f:
            doc = tomllib.load(f)
    except OSError as e:
        raise DataError("cannot read %s: %s" % (path, e))
    except tomllib.TOMLDecodeError as e:
        raise DataError("%s: %s" % (path, e))
    unknown = sorted(set(doc) - {"canary"})
    if unknown:
        raise DataError("%s: unknown top-level key %s" % (path, ", ".join(unknown)))
    out = {}
    canaries = doc.get("canary", [])
    if not isinstance(canaries, list):
        raise DataError("%s: `canary` must be an array of tables" % path)
    for i, c in enumerate(canaries, 1):
        if not isinstance(c, dict):
            raise DataError("%s: canary #%d must be a table" % (path, i))
        unknown = sorted(set(c) - CANARY_KEYS)
        if unknown:
            raise DataError("%s: canary #%d: unknown key %s" % (path, i, ", ".join(unknown)))
        name = c.get("name")
        if not isinstance(name, str) or not KIND_RE.match(name):
            raise DataError("%s: canary #%d: `name` %r must match %s" % (path, i, name, KIND_RE.pattern))
        if name in out:
            raise DataError("%s: canary %s is defined twice" % (path, name))
        edits = c.get("edit")
        if not isinstance(edits, list) or not edits:
            raise DataError("%s: canary %s needs at least one [[canary.edit]]" % (path, name))
        for j, ed in enumerate(edits, 1):
            check_edit("%s: canary %s edit #%d" % (path, name, j), ed)
        out[name] = edits
    return out


# ---------------------------------------------------------------------------------------------------------------------
# Rewriting in memory
# ---------------------------------------------------------------------------------------------------------------------


def sha256_hex(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(doc):
    return json.dumps(doc, ensure_ascii=False).encode("utf-8")


def read_json(run, rel):
    path = os.path.join(run, *rel.split("/"))
    try:
        with open(path, "rb") as f:
            doc = json.loads(f.read().decode("utf-8"))
    except OSError as e:
        raise KindError("cannot read %s: %s" % (rel, e))
    except (ValueError, UnicodeDecodeError) as e:
        raise KindError("%s is not JSON: %s" % (rel, e))
    if not isinstance(doc, dict):
        raise KindError("%s is not a JSON object" % rel)
    return doc


def ui_list(doc, rel):
    ui = doc.get("ui")
    if not isinstance(ui, list):
        raise KindError("%s has no ui[] list" % rel)
    return ui


def find_entries(doc, rel, target):
    return [e for e in ui_list(doc, rel) if isinstance(e, dict) and e.get("test_id") == target]


def need_entry(doc, rel, target):
    found = find_entries(doc, rel, target)
    if not found:
        raise KindError("%s has no ui entry with test_id %r" % (rel, target))
    return found[0]


def apply_edit(ed, doc, rel, orig):
    """One non-paint edit on one state document, in place. `orig(rel)` reads a state file of the untouched run."""
    op = ed["op"]
    if op == "drop":
        need_entry(doc, rel, ed["target"])
        doc["ui"] = [e for e in doc["ui"] if not (isinstance(e, dict) and e.get("test_id") == ed["target"])]
    elif op == "shift":
        e = need_entry(doc, rel, ed["target"])
        r = e.get("rect_px")
        if not isinstance(r, list) or len(r) != 4 or not all(is_num(x) for x in r):
            raise KindError("%s: %r has no usable rect_px" % (rel, ed["target"]))
        dx, dy = ed["dx"], ed["dy"]
        e["rect_px"] = [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy]
    elif op == "set_text":
        e = need_entry(doc, rel, ed["target"])
        e["text"] = ed["value"]
        if "label" in e:
            e["label"] = ed["value"]
    elif op == "copy_from":
        if find_entries(doc, rel, ed["target"]):
            raise KindError("%s already has a ui entry with test_id %r" % (rel, ed["target"]))
        src_rel = "state/%s.json" % ed["from"]
        src = need_entry(orig(src_rel), src_rel, ed["target"])
        doc["ui"].append(copy.deepcopy(src))
    elif op == "set":
        segs = ed["path"].split(".")
        cur = doc
        for i, s in enumerate(segs):
            if not isinstance(cur, dict) or s not in cur:
                raise KindError("%s: `%s` does not exist (stopped at %r)" % (rel, ed["path"], ".".join(segs[: i + 1])))
            if i < len(segs) - 1:
                cur = cur[s]
        cur[segs[-1]] = None if "clear" in ed else ed["value"]
    else:  # paint is handled by the caller
        raise KindError("internal: op %s" % op)


def rect_to_pixels(vc, rect, iw, ih):
    """The same rounding as c_color_token: a pixel is in when its centre is, clamped to the picture."""
    l, t, r, b = rect
    x0, x1 = max(0, math.ceil(l - 0.5)), min(iw, math.ceil(r - 0.5))
    y0, y1 = max(0, math.ceil(t - 0.5)), min(ih, math.ceil(b - 0.5))
    if x1 <= x0 or y1 <= y0:
        raise KindError("the paint rectangle %s is empty inside the %dx%d picture" % (list(rect), iw, ih))
    return x0, y0, x1, y1


def token_colour(vc, doc, rel, name):
    tokens = (doc.get("theme") or {}).get("tokens") if isinstance(doc.get("theme"), dict) else None
    if not isinstance(tokens, dict):
        raise KindError("%s has no theme.tokens" % rel)
    if name not in tokens:
        raise KindError("%s: unknown token %r (known: %s)" % (rel, name, ", ".join(sorted(tokens))))
    try:
        r, g, b, a = vc._hex_rgba(tokens[name])
    except (ValueError, TypeError, IndexError, AttributeError):
        raise KindError("%s: token %r is %r, not a hex colour" % (rel, name, tokens[name]))
    if a != 255:
        raise KindError("%s: token %r is translucent (alpha %02x); the checker needs `over` for it, so paint with rgb" % (rel, name, a))
    return r, g, b


def plan_kind(vc, run, edits):
    """Load what the kind needs from the untouched run and apply every edit in memory.

    -> (files, side) where files = {relative path: bytes} for the states and pictures, and side = {snap: set of
    {"states", "png"}} naming the shots/<snap>.json hashes that must be recomputed. Raises KindError."""
    docs = {}
    origs = {}
    pngs = {}
    dirty_states = set()
    side = {}

    def get(rel):
        if rel not in docs:
            docs[rel] = read_json(run, rel)
        return docs[rel]

    def orig(rel):
        if rel not in origs:
            origs[rel] = read_json(run, rel)
        return origs[rel]

    def png(snap):
        if snap not in pngs:
            rel = "shots/%s.png" % snap
            try:
                with open(os.path.join(run, *rel.split("/")), "rb") as f:
                    data = f.read()
                w, h, pix = vc.decode_png(data)
            except OSError as e:
                raise KindError("cannot read %s: %s" % (rel, e))
            except vc.PngError as e:
                raise KindError("%s cannot be decoded: %s" % (rel, e))
            pngs[snap] = [w, h, bytearray(pix)]
        return pngs[snap]

    for ed in edits:
        if "snap" in ed:
            snap = ed["snap"]
            rels = ["state/%s-before.json" % snap, "state/%s-after.json" % snap]
        else:
            snap = None
            rels = ["state/%s.json" % ed["state"]]
        if ed["op"] == "paint":
            w, h, pix = png(snap)
            before = rels[0]
            if "rect" in ed:
                rect = ed["rect"]
            else:
                e = need_entry(get(before), before, ed["target"])
                rect = e.get("rect_px")
                if not isinstance(rect, list) or len(rect) != 4 or not all(is_num(x) for x in rect):
                    raise KindError("%s: %r has no usable rect_px" % (before, ed["target"]))
            if "token" in ed:
                rgb = token_colour(vc, get(before), before, ed["token"])
            else:
                rgb = tuple(int(ed["rgb"][i : i + 2], 16) for i in (0, 2, 4))
            x0, y0, x1, y1 = rect_to_pixels(vc, rect, w, h)
            px = bytes(rgb) + b"\xff"
            for y in range(y0, y1):
                pix[(y * w + x0) * 4 : (y * w + x1) * 4] = px * (x1 - x0)
            side.setdefault(snap, set()).add("png")
        else:
            for rel in rels:
                apply_edit(ed, get(rel), rel, orig)
                dirty_states.add(rel)
            if snap is not None:
                side.setdefault(snap, set()).add("states")
    files = {rel: json_bytes(docs[rel]) for rel in sorted(dirty_states)}
    for snap, (w, h, pix) in pngs.items():
        if "png" in side.get(snap, ()):
            files["shots/%s.png" % snap] = vc.encode_png(w, h, pix, 4)
    return files, side


def rehash_side(run, files, side):
    """shots/<snap>.json with the new hashes -> {rel: bytes}. Without this the checker answers ERROR (broken evidence)."""
    out = {}
    for snap, parts in side.items():
        rel = "shots/%s.json" % snap
        doc = read_json(run, rel)
        if "states" in parts:
            for which in ("before", "after"):
                key = "state_" + which
                if not isinstance(doc.get(key), dict) or "sha256" not in doc[key]:
                    raise KindError("%s has no %s.sha256" % (rel, key))
                doc[key]["sha256"] = sha256_hex(files["state/%s-%s.json" % (snap, which)])
        if "png" in parts:
            if "png_sha256" not in doc:
                raise KindError("%s has no png_sha256" % rel)
            doc["png_sha256"] = sha256_hex(files["shots/%s.png" % snap])
        out[rel] = json_bytes(doc)
    return out


# ---------------------------------------------------------------------------------------------------------------------
# Writing and checking one kind
# ---------------------------------------------------------------------------------------------------------------------


def copy_run(run, dest):
    run_abs = os.path.abspath(run)

    def ignore(d, names):
        if os.path.abspath(d) == run_abs:
            return [n for n in names if n in SKIP_AT_ROOT]
        return []

    shutil.copytree(run, dest, ignore=ignore)


def describe(a):
    lines = []
    for pt in a.get("points") or []:
        for c in pt.get("checks") or []:
            if c.get("outcome") != "PASS":
                why = c.get("why") or c.get("measured")
                lines.append("    %s/%s: %s" % (pt.get("id"), c.get("check"), why))
    for err in a.get("errors") or []:
        lines.append("    error: %s" % (err,))
    for pt in a.get("points") or []:
        for err in pt.get("errors") or []:
            lines.append("    error in %s: %s" % (pt.get("id"), err))
    return lines


def check_copy(spec, dest):
    """Run the checker on the run folder `dest`. -> (process, assertions or None, error or None)."""
    p = subprocess.run(
        [sys.executable, "-X", "utf8", CHECKER, "--spec", spec, "--run", dest], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    try:
        with open(os.path.join(dest, "machine", "assertions.json"), "rb") as f:
            a = json.loads(f.read().decode("utf-8"))
        a["overall"]
        return p, a, None
    except (OSError, ValueError, KeyError, TypeError, AttributeError) as e:
        return p, None, e


def process_kind(vc, run, spec, kind, edits, rehash=True):
    """Make and check one kind. `rehash=False` is for the self-test only: it leaves the stale hashes in place."""
    run_id = os.path.basename(os.path.abspath(run))
    dest = os.path.join(run, "canary", kind, run_id)
    if os.path.exists(dest):
        return Result(False, ["%s: not made: %s is already there" % (kind, dest)], dest)
    try:
        files, side = plan_kind(vc, run, edits)
        if rehash:
            files.update(rehash_side(run, files, side))
    except KindError as e:
        return Result(False, ["%s: not made: %s" % (kind, e)], dest)
    try:
        copy_run(run, dest)
        for rel, data in files.items():
            with open(os.path.join(dest, *rel.split("/")), "wb") as f:
                f.write(data)
    except OSError as e:
        shutil.rmtree(dest, ignore_errors=True)
        return Result(False, ["%s: not made: %s" % (kind, e)], dest)
    p, a, e = check_copy(spec, dest)
    if e is None:
        lines = ["%s: overall=%s exit=%d" % (kind, a["overall"], p.returncode)] + describe(a)
        ok = a["overall"] == "FAIL" and p.returncode == 1
    else:
        tail = (p.stderr or p.stdout or "").strip().splitlines()[-3:]
        lines = ["%s: exit=%d, no usable machine/assertions.json (%s)" % (kind, p.returncode, e)] + ["    " + t for t in tail]
        ok = False
    return Result(ok, lines, dest)


def run_tool(args):
    run, spec = args.run, args.spec
    if not os.path.isdir(run):
        raise DataError("no such run folder: %s" % run)
    if not os.path.isfile(spec):
        raise DataError("no such spec: %s" % spec)
    if not args.kinds:
        raise DataError("name at least one kind")
    if len(set(args.kinds)) != len(args.kinds):
        raise DataError("a kind is named twice")
    path = args.canaries
    if path is None:
        stem = spec[:-5] if spec.endswith(".toml") else spec
        path = stem + ".canary.toml"
    if not os.path.isfile(path):
        raise DataError("no such canary file: %s" % path)
    kinds = load_canaries(path)
    for k in args.kinds:
        if k not in kinds:
            raise DataError("%s: no canary named %r (has: %s)" % (path, k, ", ".join(kinds) or "none"))
    vc = load_checker()
    # The run must pass on its own first: on a run that already fails, every canary would "fail as it should".
    with tempfile.TemporaryDirectory(prefix="make-canary-base-") as tmp:
        base = os.path.join(tmp, os.path.basename(os.path.abspath(run)))
        try:
            copy_run(run, base)
        except OSError as e:
            raise DataError("cannot copy the run to check it first: %s" % e)
        p, a, e = check_copy(spec, base)
    if e is not None:
        raise DataError("the run itself is not PASS: exit=%d, no usable machine/assertions.json (%s); a canary on it would prove nothing" % (p.returncode, e))
    if a["overall"] != "PASS" or p.returncode != 0:
        raise DataError("the run itself is not PASS: overall=%s exit=%d; a canary on it would prove nothing" % (a["overall"], p.returncode))
    worst = 0
    for k in args.kinds:
        res = process_kind(vc, run, spec, k, kinds[k])
        for line in res.lines:
            print(line)
        if not res.ok:
            worst = 1
    print("CANARY " + ("NOT ALL FAIL" if worst else "ALL FAIL (as they should)"))
    return worst


def main(argv=None):
    try:
        sys.stdout.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass
    ap = argparse.ArgumentParser(prog="make-canary.py", description="Copy a real run, make a measured value wrong, and check the checker answers FAIL.")
    ap.add_argument("--run", help="the run folder")
    ap.add_argument("--spec", help="the check spec (tests/visual/<row>.toml)")
    ap.add_argument("--canaries", help="the canary data file (default: <spec without .toml>.canary.toml)")
    ap.add_argument("--self-test", action="store_true", help="run the canaries of this tool")
    ap.add_argument("kinds", nargs="*", help="the canary kinds to make")
    args = ap.parse_args(argv)
    if args.self_test:
        if args.run or args.spec or args.canaries or args.kinds:
            ap.error("--self-test takes nothing else")
        return selftest()
    if not args.run or not args.spec:
        ap.error("--run and --spec are required (or --self-test)")
    try:
        return run_tool(args)
    except DataError as e:
        sys.stderr.write("make-canary.py: %s\n" % e)
        return 2


# ---------------------------------------------------------------------------------------------------------------------
# Self-test: a small real run, the real checker in a subprocess
# ---------------------------------------------------------------------------------------------------------------------


def tree_hashes(root, skip_canary=True):
    out = {}
    for d, dirs, names in os.walk(root):
        if skip_canary and os.path.abspath(d) == os.path.abspath(root):
            dirs[:] = [x for x in dirs if x != "canary"]
        for n in names:
            p = os.path.join(d, n)
            with open(p, "rb") as f:
                out[os.path.relpath(p, root).replace(os.sep, "/")] = sha256_hex(f.read())
    return out


def call(argv):
    """main(argv) with its output captured -> (exit code, stdout, stderr)."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            code = main(argv)
        except SystemExit as e:
            code = e.code if isinstance(e.code, int) else 2
    return code, out.getvalue(), err.getvalue()


def selftest():
    vc = load_checker()
    fails = []
    base = tempfile.mkdtemp(prefix="make-canary-selftest-")
    counter = [0]

    def world(extra=None):
        """A fresh real run + its folder. -> (run dir, folder)."""
        counter[0] += 1
        root = os.path.join(base, "w%02d" % counter[0])
        m = vc.Model("run1")
        m.states["palette"] = vc.make_state("palette", "run1")
        other = vc.make_state("other", "run1")
        other["ui"].append(vc.ui_entry("spot.extra", [40, 60, 90, 80], text="Extra"))
        m.states["other"] = other
        run = os.path.join(root, "run1")
        vc.write_run(m, run)
        if extra:
            extra(run)
        return run, root

    def put(root, name, text):
        p = os.path.join(root, name)
        with open(p, "wb") as f:
            f.write(text.encode("utf-8"))
        return p

    def tool(root, spec_text, canary_text, kinds, run=None):
        spec = put(root, "spec.toml", spec_text)
        put(root, "spec.canary.toml", canary_text)
        return call(["--run", run or os.path.join(root, "run1"), "--spec", spec] + kinds)

    def canary(name, *edits):
        text = '[[canary]]\nname = "%s"\n' % name
        for e in edits:
            text += "\n[[canary.edit]]\n" + e.strip() + "\n"
        return text

    spec_thumb_present = vc.HEAD + vc.snap_point("present = true", pid="thumb")
    spec_label = vc.HEAD + vc.state_point('present = true, text = "Hello world"', pid="label")
    spec_extra_absent = vc.HEAD + vc.state_point("present = false", pid="extra", target="spot.extra")
    spec_overlay = vc.HEAD + vc.state_point("no_overlay = true", pid="overlay")
    spec_mode = vc.HEAD + vc.state_point('app_state = { nested = { a = 1 } }', pid="mode")
    spec_colour = vc.HEAD + vc.snap_point('color_token = { token = "done", delta_e_max = 3, min_fraction = 1 }', pid="colour")

    # 1. every op ends FAIL (exit 1 from the checker, 0 from this tool), and names the point it broke
    cases = [
        ("drop", 'snap = "top"\nop = "drop"\ntarget = "spot.scroll.thumb"', spec_thumb_present, "thumb/"),
        ("shift", 'snap = "top"\nop = "shift"\ntarget = "spot.scroll.thumb"\ndx = -40\ndy = 0', vc.SPEC_THUMB, "p1/right_gap_px"),
        ("set-text", 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "Goodbye"', spec_label, "label/"),
        ("copy-from", 'state = "main"\nop = "copy_from"\ntarget = "spot.extra"\nfrom = "other"', spec_extra_absent, "extra/"),
        ("set-value", 'state = "main"\nop = "set"\npath = "overlay"\nvalue = "help"', spec_overlay, "overlay/"),
        ("set-clear", 'state = "main"\nop = "set"\npath = "app_state.nested.a"\nclear = true', spec_mode, "mode/"),
        ("paint-token", 'snap = "top"\nop = "paint"\ntarget = "spot.scroll.thumb"\ntoken = "bg"', spec_colour, "colour/"),
        ("paint-rgb", 'snap = "top"\nop = "paint"\nrect = [270, 38, 281, 92]\nrgb = "f07178"', spec_colour, "colour/"),
    ]
    for kind, edit, spec, needle in cases:
        run, root = world()
        before = tree_hashes(run)
        code, out, err = tool(root, spec, canary(kind, edit), [kind])
        if code != 0 or "CANARY ALL FAIL (as they should)" not in out or ("%s: overall=FAIL exit=1" % kind) not in out:
            fails.append("op %s: code %s, output %r %r" % (kind, code, out[:500], err[:200]))
            continue
        if not any(l.startswith("    " + needle) for l in out.splitlines()):
            fails.append("op %s: the broken check %r is not in the output %r" % (kind, needle, out[:500]))
        if tree_hashes(run) != before:
            fails.append("op %s: the original run changed" % kind)
        if not os.path.isdir(os.path.join(run, "canary", kind, "run1")):
            fails.append("op %s: no canary/%s/run1" % (kind, kind))

    # 2. a paint-only kind leaves the states alone; a snap edit rewrites both states the same way and the side file hashes
    run, root = world()
    tool(root, vc.SPEC_THUMB, canary("p", 'snap = "top"\nop = "paint"\nrect = [0, 0, 10, 10]\nrgb = "ffffff"'), ["p"])
    dest = os.path.join(run, "canary", "p", "run1")
    for rel in ("state/top-before.json", "state/top-after.json", "state/main.json"):
        if tree_hashes(run).get(rel) != tree_hashes(dest, False).get(rel):
            fails.append("paint-only: %s was rewritten" % rel)
    with open(os.path.join(dest, "shots", "top.png"), "rb") as f:
        w, h, pix = vc.decode_png(f.read())
    if (w, h) != (vc.IMG_W, vc.IMG_H) or bytes(pix[0:4]) != b"\xff\xff\xff\xff" or bytes(pix[(10 * w) * 4 : (10 * w) * 4 + 4]) != bytes(vc.GOOD_PIX["bg"]) + b"\xff":
        fails.append("paint: the picture is wrong (size %s, first pixel %s)" % ((w, h), bytes(pix[0:4]).hex()))
    run, root = world()
    tool(root, vc.SPEC_THUMB, canary("s", 'snap = "top"\nop = "shift"\ntarget = "spot.scroll.thumb"\ndx = 5\ndy = 0'), ["s"])
    dest = os.path.join(run, "canary", "s", "run1")
    b, a = (json.load(open(os.path.join(dest, "state", "top-%s.json" % w_), encoding="utf-8")) for w_ in ("before", "after"))
    if vc.ui_of(b, "spot.scroll.thumb")["rect_px"] != vc.ui_of(a, "spot.scroll.thumb")["rect_px"] or vc.ui_of(b, "spot.scroll.thumb")["rect_px"][0] != 276:
        fails.append("snap edit: before and after are not both shifted by 5")
    side = json.load(open(os.path.join(dest, "shots", "top.json"), encoding="utf-8"))
    for w_ in ("before", "after"):
        with open(os.path.join(dest, "state", "top-%s.json" % w_), "rb") as f:
            if side["state_" + w_]["sha256"] != sha256_hex(f.read()):
                fails.append("snap edit: state_%s.sha256 was not recomputed" % w_)

    # 3. without the rehash the run is broken evidence: ERROR, and that is not a pass
    for label, edit, spec in (
        ("states", 'snap = "top"\nop = "shift"\ntarget = "spot.scroll.thumb"\ndx = -40\ndy = 0', vc.SPEC_THUMB),
        ("png", 'snap = "top"\nop = "paint"\nrect = [270, 38, 281, 92]\nrgb = "f07178"', spec_colour),
    ):
        run, root = world()
        spec_path = put(root, "spec.toml", spec)
        edits = load_canaries(put(root, "spec.canary.toml", canary("k", edit)))["k"]
        res = process_kind(vc, run, spec_path, "k", edits, rehash=False)
        if res.ok or not res.lines[0].startswith("k: overall=ERROR exit=2"):
            fails.append("no rehash (%s) must end ERROR and not pass: %r" % (label, res.lines[:2]))

    # 4. a harmless edit the spec does not look at ends PASS and the tool says so
    run, root = world()
    code, out, _ = tool(root, spec_label, canary("quiet", 'state = "main"\nop = "set_text"\ntarget = "spot.button"\nvalue = "Stop"'), ["quiet"])
    if code != 1 or "quiet: overall=PASS exit=0" not in out or out.splitlines()[-1] != "CANARY NOT ALL FAIL":
        fails.append("harmless edit: code %s, output %r" % (code, out[:400]))

    # 5. root-level machine/vision/crops/canary are skipped, nested ones are copied
    def litter(run):
        for rel in ("machine/old.txt", "vision/v.txt", "crops/c.txt", "canary/old/x.txt", "shots/machine/n.txt", "state/crops/m.txt"):
            os.makedirs(os.path.dirname(os.path.join(run, *rel.split("/"))), exist_ok=True)
            with open(os.path.join(run, *rel.split("/")), "wb") as f:
                f.write(b"x")

    run, root = world(litter)
    code, out, err = tool(root, spec_label, canary("c", 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "Goodbye"'), ["c"])
    dest = os.path.join(run, "canary", "c", "run1")
    for rel in ("machine/old.txt", "vision", "crops", "canary"):
        if os.path.exists(os.path.join(dest, *rel.split("/"))):
            fails.append("root-level %s was copied" % rel)
    for rel in ("shots/machine/n.txt", "state/crops/m.txt"):
        if not os.path.exists(os.path.join(dest, *rel.split("/"))):
            fails.append("nested %s was not copied" % rel)
    if code != 0:
        fails.append("litter run: code %s %r %r" % (code, out[:300], err[:200]))

    # 6. bad data: exit 2 and nothing copied
    ok_edit = 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "x"'
    good = canary("good", ok_edit)
    bad = {
        "unknown canary key": '[[canary]]\nname = "bad"\nnme = 1\n[[canary.edit]]\n' + ok_edit,
        "unknown edit key": canary("bad", ok_edit + "\ntargt = 1"),
        "key from another op": canary("bad", ok_edit + "\ndx = 1"),
        "unknown op": canary("bad", 'state = "main"\nop = "explode"\ntarget = "spot.label"'),
        "bad name": canary("Bad Name", ok_edit),
        "empty name start": canary("-x", ok_edit),
        "no edit": '[[canary]]\nname = "bad"\n',
        "twice": good + good,
        "toml syntax": "[[canary\n",
        "unknown top key": good + "\nextra = 1\n",
        "both snap and state": canary("bad", ok_edit + '\nsnap = "top"'),
        "neither snap nor state": canary("bad", 'op = "drop"\ntarget = "t"'),
        "stem with slash": canary("bad", 'snap = "../x"\nop = "drop"\ntarget = "t"'),
        "from with slash": canary("bad", 'state = "main"\nop = "copy_from"\ntarget = "t"\nfrom = "a/b"'),
        "copy_from without from": canary("bad", 'state = "main"\nop = "copy_from"\ntarget = "t"'),
        "boolean dx": canary("bad", 'snap = "top"\nop = "shift"\ntarget = "t"\ndx = true\ndy = 0'),
        "string dy": canary("bad", 'snap = "top"\nop = "shift"\ntarget = "t"\ndx = 0\ndy = "1"'),
        "inf dx": canary("bad", 'snap = "top"\nop = "shift"\ntarget = "t"\ndx = inf\ndy = 0'),
        "inf value": canary("bad", 'state = "main"\nop = "set"\npath = "app_state.mode"\nvalue = inf'),
        "nan value": canary("bad", 'state = "main"\nop = "set"\npath = "app_state.mode"\nvalue = nan'),
        "date value": canary("bad", 'state = "main"\nop = "set"\npath = "app_state.mode"\nvalue = 2020-01-01'),
        "array value": canary("bad", 'state = "main"\nop = "set"\npath = "app_state.mode"\nvalue = [1]'),
        "set root not allowed": canary("bad", 'state = "main"\nop = "set"\npath = "ui"\nvalue = "x"'),
        "set empty segment": canary("bad", 'state = "main"\nop = "set"\npath = "overlay..x"\nvalue = "x"'),
        "set value and clear": canary("bad", 'state = "main"\nop = "set"\npath = "overlay"\nvalue = "x"\nclear = true'),
        "set neither": canary("bad", 'state = "main"\nop = "set"\npath = "overlay"'),
        "set clear false": canary("bad", 'state = "main"\nop = "set"\npath = "overlay"\nclear = false'),
        "set_text number": canary("bad", 'state = "main"\nop = "set_text"\ntarget = "t"\nvalue = 3'),
        "paint rect and target": canary("bad", 'snap = "top"\nop = "paint"\nrect = [0, 0, 1, 1]\ntarget = "t"\nrgb = "ffffff"'),
        "paint neither rect nor target": canary("bad", 'snap = "top"\nop = "paint"\nrgb = "ffffff"'),
        "paint token and rgb": canary("bad", 'snap = "top"\nop = "paint"\ntarget = "t"\ntoken = "bg"\nrgb = "ffffff"'),
        "paint neither token nor rgb": canary("bad", 'snap = "top"\nop = "paint"\ntarget = "t"'),
        "paint bad rgb": canary("bad", 'snap = "top"\nop = "paint"\ntarget = "t"\nrgb = "fff"'),
        "paint rect boolean": canary("bad", 'snap = "top"\nop = "paint"\nrect = [0, 0, true, 1]\nrgb = "ffffff"'),
        "paint rect length": canary("bad", 'snap = "top"\nop = "paint"\nrect = [0, 0, 1]\nrgb = "ffffff"'),
        "paint on a state": canary("bad", 'state = "main"\nop = "paint"\ntarget = "t"\nrgb = "ffffff"'),
        "valid kind next to a bad one": good + canary("bad", 'op = "drop"\ntarget = "t"'),
    }
    for label, text in bad.items():
        run, root = world()
        code, out, err = tool(root, vc.SPEC_THUMB, text, ["good"] if "valid kind" in label else ["bad"])
        if code != 2 or out or os.path.exists(os.path.join(run, "canary")):
            fails.append("bad data (%s): code %s, output %r, canary folder %s" % (label, code, out[:200], os.path.exists(os.path.join(run, "canary"))))
    run, root = world()
    code, out, err = tool(root, vc.SPEC_THUMB, good, ["nope"])
    if code != 2 or os.path.exists(os.path.join(run, "canary")) or "nope" not in err:
        fails.append("unknown kind: code %s, err %r" % (code, err[:200]))
    code, out, err = tool(root, vc.SPEC_THUMB, good, ["good", "good"])
    if code != 2 or os.path.exists(os.path.join(run, "canary")):
        fails.append("a kind named twice must exit 2")
    code, _, _ = tool(root, vc.SPEC_THUMB, good, ["good"], run=os.path.join(root, "no-such-run"))
    if code != 2:
        fails.append("a missing run must exit 2")
    code, _, _ = call(["--run", run, "--spec", os.path.join(root, "no-such.toml"), "good"])
    if code != 2:
        fails.append("a missing spec must exit 2")
    os.remove(os.path.join(root, "spec.canary.toml"))
    code, _, err = call(["--run", run, "--spec", os.path.join(root, "spec.toml"), "good"])
    if code != 2 or os.path.exists(os.path.join(run, "canary")):
        fails.append("a missing data file must exit 2 (got %s)" % code)
    put(root, "spec.toml", spec_label)
    elsewhere = put(root, "elsewhere.toml", good)
    code, out, _ = call(["--run", run, "--spec", os.path.join(root, "spec.toml"), "--canaries", elsewhere, "good"])
    if code != 0 or "good: overall=FAIL" not in out:
        fails.append("--canaries must name the data file: code %s %r" % (code, out[:300]))
    if call([])[0] != 2:
        fails.append("no arguments must exit 2")
    if call(["--self-test", "x"])[0] != 2:
        fails.append("--self-test takes nothing else")

    # 7. contents that do not fit fail that kind only: no folder for it, the others still run
    run, root = world()
    before = tree_hashes(run)
    text = (
        canary("ok", 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "Goodbye"')
        + canary("missing-target", 'state = "main"\nop = "drop"\ntarget = "spot.nothing"')
        + canary("bad-path", 'state = "main"\nop = "set"\npath = "app_state.nope"\nvalue = 1')
        + canary("bad-parent", 'state = "main"\nop = "set"\npath = "app_state.mode.deeper"\nvalue = 1')
        + canary("bad-root", 'state = "main"\nop = "set"\npath = "overlay.x"\nvalue = 1')
        + canary("translucent", 'snap = "top"\nop = "paint"\ntarget = "spot.scroll.thumb"\ntoken = "ghost"')
        + canary("unknown-token", 'snap = "top"\nop = "paint"\ntarget = "spot.scroll.thumb"\ntoken = "nope"')
        + canary("already-there", 'state = "main"\nop = "copy_from"\ntarget = "spot.label"\nfrom = "other"')
        + canary("not-in-source", 'state = "main"\nop = "copy_from"\ntarget = "spot.extra"\nfrom = "palette"')
        + canary("empty-rect", 'snap = "top"\nop = "paint"\nrect = [500, 500, 600, 600]\nrgb = "ffffff"')
        + canary("no-such-state", 'state = "ghostly"\nop = "drop"\ntarget = "t"')
        + canary("no-such-snap", 'snap = "ghostly"\nop = "paint"\nrect = [0, 0, 5, 5]\nrgb = "ffffff"')
        + canary("late-failure", 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "x"\n\n[[canary.edit]]\nstate = "main"\nop = "drop"\ntarget = "spot.nothing"')
    )
    names = ["missing-target", "bad-path", "bad-parent", "bad-root", "translucent", "unknown-token", "already-there", "not-in-source", "empty-rect",
             "no-such-state", "no-such-snap", "late-failure"]
    code, out, err = tool(root, spec_label, text, names[:5] + ["ok"] + names[5:])
    if code != 1 or out.splitlines()[-1] != "CANARY NOT ALL FAIL" or "ok: overall=FAIL exit=1" not in out:
        fails.append("run contents: code %s output %r" % (code, out[:900]))
    for n in names:
        if os.path.exists(os.path.join(run, "canary", n)):
            fails.append("run contents: a folder was created for %s" % n)
        if not any(l.startswith(n + ": not made: ") for l in out.splitlines()):
            fails.append("run contents: %s is not reported as not made" % n)
    if not os.path.isdir(os.path.join(run, "canary", "ok", "run1")):
        fails.append("run contents: the good kind did not run")
    if tree_hashes(run) != before:
        fails.append("run contents: the original run changed")

    # 8. an existing destination is refused and left as it was
    run, root = world()
    keep = os.path.join(run, "canary", "again", "run1")
    os.makedirs(keep)
    with open(os.path.join(keep, "mine.txt"), "wb") as f:
        f.write(b"mine")
    code, out, _ = tool(root, spec_label, canary("again", ok_edit), ["again"])
    if code != 1 or "already there" not in out or os.listdir(keep) != ["mine.txt"]:
        fails.append("an existing destination: code %s, output %r, left %r" % (code, out[:200], os.listdir(keep)))

    # 9. a run that does not pass on its own: exit 2, nothing made, the original untouched
    run, root = world()
    before = tree_hashes(run)
    spec_wrong = vc.HEAD + vc.state_point('present = true, text = "Not the label"', pid="label")
    code, out, err = tool(root, spec_wrong, canary("k", 'state = "main"\nop = "set_text"\ntarget = "spot.label"\nvalue = "Goodbye"'), ["k"])
    if code != 2 or out or "not PASS" not in err or "overall=FAIL" not in err:
        fails.append("a run that is not PASS: code %s, output %r, err %r" % (code, out[:200], err[:300]))
    if os.path.exists(os.path.join(run, "canary")) or tree_hashes(run, False) != before:
        fails.append("a run that is not PASS: the original run changed or a canary folder was made")

    shutil.rmtree(base, ignore_errors=True)
    if fails:
        for f in fails:
            print("make-canary: FAIL " + f)
        print("make-canary: self-test FAILED (%d)" % len(fails))
        return 1
    print("make-canary: self-test OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
