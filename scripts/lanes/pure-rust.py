#!/usr/bin/env python3
# Source is uchmk/ito scripts/lanes/; an app's copy is written by its scripts/lanes.sh sync, so edit ito's.
#
# "Pure Rust": fail when a C toolchain creeps into an app's build. uchmk apps cross-compile to 6 targets, and a new
# dependency that pulls in `cc` or a `-sys` crate goes unnoticed until a cross build breaks. This reads the dependency
# tree of every target and flags a crate that is a build helper (HELPERS below) or is named `*-sys`.
#
#   scripts/pure-rust.py            check the workspace in the current folder on the 6 targets
#   scripts/pure-rust.py --self-test
#
# Exceptions are lines of two files beside this script: the shared pure-rust.txt (from ito) and the app's own
# pure-rust.allow (may be missing). Grammar, `#` comments and blank lines skipped:
#
#   <crate> [via <parent>] [with <feature> | without <feature>] <reason ...>
#
# `via` limits the exception to one parent crate; `with`/`without` is a condition on the feature set the tree shows
# (the parent's when `via` is given, else the crate's own). A line without a reason does not count. A build helper
# (cc, cmake ...) is judged by the crate that runs it, so its exception needs `via`.
#
# Limits, so nobody reads more into an OK than is there:
# - It is an early signal, not a proof: it judges by names (HELPERS and `*-sys`). A build.rs that runs a compiler on its
#   own, or through a helper not in the list, slips through; a `-sys` crate can be pure Rust bindings (hence the list).
#   The release builds of the 6 targets are the real test.
# - It looks at the whole workspace with default features, a superset of what release.yml builds (`-p <crate>`), so a
#   member that never ships can make it fail; such a crate gets its own line in pure-rust.allow.

import os
import re
import subprocess
import sys
from collections import deque, namedtuple

HERE = os.path.dirname(os.path.abspath(__file__))

TARGETS = [
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
]

# Build helpers that run a C toolchain or probe for system C libraries.
HELPERS = {"cc", "cmake", "nasm-rs", "bindgen", "pkg-config", "system-deps", "vcpkg", "cxx-build"}

SHARED = "pure-rust.txt"
APP = "scripts/pure-rust.allow"

Node = namedtuple("Node", "name ver feats")
Entry = namedtuple("Entry", "crate via cond reason file line")  # cond: None | ("with"|"without", feature)

# `--prefix depth` puts the depth right before the name; cargo refuses a package name that starts with a digit, so the
# digits end where the name begins.
LINE = re.compile(r"^(\d+)(\S+) v(\S+)(.*)$")
CRATE = re.compile(r"^[A-Za-z0-9_-]+$")


def flagged(name):
    return name in HELPERS or name.endswith("-sys")


def parse_tree(text):
    """(edges, roots) of one `cargo tree --prefix depth --format "{p}|{f}"` output.

    edges: every occurrence as (parent Node, child Node), a `(*)` line included; roots: names at depth 0.
    Each line keeps its own feature set (resolver 2 can print one package as two nodes).
    Raises ValueError on a line it cannot read or a tree with no member, so a changed format never reads as "OK"."""
    edges, roots, stack = [], [], []
    for raw in text.splitlines():
        line = raw.rstrip()
        if not line.strip():
            stack = []
            continue
        head, _, feats = line.rpartition("|")
        m = LINE.match(head)
        if not m:
            raise ValueError(f"cannot read the line {line!r}")
        feats = feats.strip()
        if feats.endswith("(*)"):
            feats = feats[:-3].strip()
        depth = int(m.group(1))
        node = Node(m.group(2), m.group(3), frozenset(f for f in feats.split(",") if f))
        if depth == 0:
            roots.append(node.name)
            stack = [node]
            continue
        if depth > len(stack):
            raise ValueError(f"the line {line!r} has no parent at depth {depth - 1}")
        edges.append((stack[depth - 1], node))
        stack = stack[:depth] + [node]
    if not roots:
        raise ValueError("no workspace member in it")
    return edges, sorted(set(roots))


def parse_exceptions(text, file):
    """(entries, errors) of one exception file's text. A rejected line is an error and gives no entry."""
    entries, errors = [], []
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        where = f"pure-rust: {file}:{n}:"
        w = line.split()
        crate, i, via, cond = w[0], 1, None, None
        bad = None
        if not CRATE.match(crate):
            bad = f"`{crate}` is not a crate name"
        if bad is None and len(w) > i and w[i] == "via":
            if len(w) <= i + 1:
                bad = "`via` needs a parent crate after it"
            else:
                via, i = w[i + 1], i + 2
                if not CRATE.match(via):
                    bad = f"`{via}` is not a crate name"
        if bad is None and len(w) > i and w[i] in ("with", "without"):
            if len(w) <= i + 1:
                bad = f"`{w[i]}` needs a feature after it"
            else:
                cond, i = (w[i], w[i + 1]), i + 2
                if len(w) > i and w[i] in ("with", "without"):
                    bad = "a condition is given twice"
        if bad:
            errors.append(f"{where} malformed line ({bad}); the line does not count")
            continue
        reason = " ".join(w[i:])
        if not reason:
            errors.append(f"{where} no reason; the line does not count")
            continue
        if crate in HELPERS and via is None:
            errors.append(f"{where} `{crate}` is a build helper; name the crate that pulls it in (`{crate} via <parent> <why>`)")
            continue
        entries.append(Entry(crate, via, cond, reason, file, n))
    return entries, errors


def judge(entry, parent, child):
    """None when the entry does not apply to this occurrence, else (covers, note)."""
    if entry.crate != child.name or (entry.via is not None and entry.via != parent.name):
        return None
    if entry.cond is None:
        return True, ""
    kind, feat = entry.cond
    subject = parent if entry.via else child
    on = feat in subject.feats
    if (kind == "with") == on:
        return True, ""
    note = (
        f"the exception in {entry.file}:{entry.line} needs {subject.name} {kind} `{feat}`, "
        f"and `{feat}` is {'on' if on else 'off'}"
    )
    return False, note


def shortest_path(edges, roots, goal_parent, child):
    """Names from a root to `child` through `goal_parent`, the shortest by BFS ([] when unreachable)."""
    adj = {}
    for p, c in edges:
        adj.setdefault(p.name, []).append(c.name)
    prev = {r: None for r in roots}
    queue = deque(roots)
    while queue:
        cur = queue.popleft()
        if cur == goal_parent:
            break
        for nxt in adj.get(cur, []):
            if nxt not in prev:
                prev[nxt] = cur
                queue.append(nxt)
    if goal_parent not in prev:
        return []
    path, cur = [], goal_parent
    while cur is not None:
        path.append(cur)
        cur = prev[cur]
    return list(reversed(path)) + [child]


def analyze(trees, shared, app):
    """(lines, code) for {target: tree text}, the shared entries and the app's entries."""
    parsed = {}
    for t in TARGETS:
        if t in trees:
            try:
                parsed[t] = parse_tree(trees[t])
            except ValueError as e:
                return [f"pure-rust: cannot read the cargo tree of {t}: {e}"], 1
    used, touched = set(), set()
    groups = {}  # (child, parent) -> {"targets": [...], "notes": [...]}
    for t, (edges, _roots) in parsed.items():
        for parent, child in edges:
            if not flagged(child.name):
                continue
            covered, notes = False, []
            for e in shared + app:
                r = judge(e, parent, child)
                if r is None:
                    continue
                touched.add((e.file, e.line))
                if r[0]:
                    covered = True
                    used.add((e.file, e.line))
                elif r[1] not in notes:
                    notes.append(r[1])
            if covered:
                continue
            g = groups.setdefault((child.name, parent.name), {"targets": [], "notes": []})
            if t not in g["targets"]:
                g["targets"].append(t)
            g["notes"].extend(n for n in notes if n not in g["notes"])
    lines = []
    for (child, parent), g in sorted(groups.items()):
        ts = g["targets"]
        on = "all 6 targets" if ts == TARGETS else ", ".join(ts)
        edges, roots = parsed[ts[0]]
        path = " -> ".join(shortest_path(edges, roots, parent, child))
        note = f" ({'; '.join(g['notes'])})" if g["notes"] else ""
        lines.append(f"pure-rust: `{child}` (pulled in by `{parent}`) on {on}: {path}{note}")
    stale = [e for e in app if (e.file, e.line) not in touched]
    for e in stale:
        what = e.crate + (f" via {e.via}" if e.via else "")
        lines.append(f"pure-rust: {e.file}:{e.line} (`{what}`) matches nothing; drop the line")
    if groups:
        lines.append(
            "pure-rust: these need a C toolchain to build, which makes cross-compiling to the 6 targets hard; prefer a pure Rust crate."
        )
        lines.append(
            "pure-rust: when one builds no C here (bindings only, or its C is behind a feature that is off), read its build.rs "
            "and add `<crate> [via <parent>] [with|without <feature>] <why>` to scripts/pure-rust.allow; "
            "`cargo tree -i <crate> -e normal,build --target <t>` shows every path."
        )
    if not lines:
        lines.append(f"pure-rust: OK ({len(parsed)} targets, {len(used)} exceptions used)")
        return lines, 0
    return lines, 1


def read_text(path):
    try:
        with open(path, encoding="utf-8-sig") as f:
            return f.read()
    except FileNotFoundError:
        return ""


def run(trees, shared_text, app_text):
    """(lines, code) for the trees and the two exception files' text. A rejected line fails the check, and then
    there is no OK line."""
    shared, e1 = parse_exceptions(shared_text, SHARED)
    app, e2 = parse_exceptions(app_text, APP)
    lines, code = analyze(trees, shared, app)
    if e1 or e2:
        return e1 + e2 + (lines if code else []), 1
    return lines, code


def cargo_tree(target):
    return subprocess.run(
        ["cargo", "tree", "--locked", "--workspace", "-e", "normal,build", "--target", target, "--prefix", "depth", "--format", "{p}|{f}"],
        capture_output=True, text=True, encoding="utf-8",
    )


def check():
    trees = {}
    for t in TARGETS:
        r = cargo_tree(t)
        if r.returncode != 0:
            print(f"pure-rust: cargo tree failed for {t}:")
            print(r.stderr.rstrip())
            return 1
        trees[t] = r.stdout
    lines, code = run(trees, read_text(os.path.join(HERE, "pure-rust.txt")), read_text(os.path.join(HERE, "pure-rust.allow")))
    for line in lines:
        print(line)
    return code


# A cut of a real tree (kura's, x86_64-unknown-linux-gnu): `(*)` follows the features and repeats them.
REAL = """\
0kura v0.164.5 (C:\\dev\\kura)|
1winit v0.30.13|default,wayland
2smithay-client-toolkit v0.19.2|calloop,calloop-wayland-source
3calloop v0.13.0| (*)
3wayland-backend v0.3.17|client_system,dlopen
4wayland-sys v0.31.11|client,dlopen,egl,once_cell
4cc v1.4.7|
5shlex v2.0.1|default,std
3wayland-client v0.31.15|
4wayland-backend v0.3.17|client_system,dlopen (*)
4wayland-scanner v0.31.11 (proc-macro)|
5proc-macro2 v1.0.107|default,proc-macro (*)
1wayland-sys v0.31.11|client,dlopen,egl,once_cell (*)
"""


def tree(text):
    return {t: text for t in TARGETS}


def self_test():
    # parsing
    edges, roots = parse_tree(REAL)
    assert roots == ["kura"]
    pairs = [(p.name, c.name) for p, c in edges]
    assert ("smithay-client-toolkit", "calloop") in pairs  # a `(*)` line still gives its edge
    assert ("wayland-client", "wayland-backend") in pairs
    assert ("wayland-backend", "cc") in pairs and ("kura", "winit") in pairs
    star = [c for p, c in edges if c.name == "wayland-sys" and p.name == "kura"][0]
    assert star.feats == {"client", "dlopen", "egl", "once_cell"} and star.ver == "0.31.11"
    assert [c for p, c in edges if c.name == "calloop"][0].feats == frozenset()
    assert [c for p, c in edges if c.name == "wayland-scanner"][0].feats == frozenset()
    assert [c for p, c in edges if c.name == "proc-macro2"][0].feats == {"default", "proc-macro"}
    spaced = "0myapp v0.1.0 (C:\\Program Files (x86)\\my app)|default\n1cc v1.0.0|parallel\n\n0other v0.2.0 (C:\\o)|\n1ring v0.17.0|\n"
    # (a name never has a space; the path does, and holds parentheses)
    edges2, roots2 = parse_tree(spaced)
    assert roots2 == ["myapp", "other"] and [(p.name, c.name) for p, c in edges2] == [("myapp", "cc"), ("other", "ring")]
    assert edges2[0][1].feats == {"parallel"}
    two = "0app v1.0.0 (/a)|\n1x v1|\n2foo-sys v1.0.0|a\n1y v1|\n2foo-sys v1.0.0|b\n"
    e3, _ = parse_tree(two)
    assert [c.feats for p, c in e3 if c.name == "foo-sys"] == [{"a"}, {"b"}]
    assert [p.name for p, c in e3 if c.name == "foo-sys"] == ["x", "y"]  # a flagged crate under two parents

    # grammar
    text = "# c\n\nfoo-sys bindings only\nbar-sys via baz does nothing\nqux-sys with f one\nqux-sys without g two\n"
    ents, errs = parse_exceptions(text, "t")
    assert not errs and [(e.crate, e.via, e.cond) for e in ents] == [
        ("foo-sys", None, None),
        ("bar-sys", "baz", None),
        ("qux-sys", None, ("with", "f")),
        ("qux-sys", None, ("without", "g")),
    ]
    assert ents[1].reason == "does nothing"
    ents, errs = parse_exceptions("cc via wayland-backend without log compiles only with log\n", "t")
    assert ents[0].via == "wayland-backend" and ents[0].cond == ("without", "log") and ents[0].reason == "compiles only with log"
    _, errs = parse_exceptions("foo-sys\nfoo-sys via bar\nfoo-sys with f\n", "t")
    assert len(errs) == 3 and all("no reason" in e for e in errs) and errs[0].startswith("pure-rust: t:1:")
    ents, errs = parse_exceptions("cc just because\n", "t")
    assert not ents and "build helper" in errs[0] and "`cc via <parent> <why>`" in errs[0]
    for bad in ["foo-sys via", "foo-sys with", "foo-sys without", "foo.sys why", "foo-sys via a.b why", "foo-sys with a with b why"]:
        ents, errs = parse_exceptions(bad, "t")
        assert not ents and len(errs) == 1, bad

    # matching, per occurrence
    wb = Node("wayland-backend", "0.3.17", frozenset({"log", "dlopen"}))
    wb_off = Node("wayland-backend", "0.3.17", frozenset({"dlopen"}))
    cc = Node("cc", "1.4.7", frozenset())
    ex = parse_exceptions("cc via wayland-backend without log C only with log\n", "pure-rust.txt")[0][0]
    assert judge(ex, wb_off, cc) == (True, "")
    ok, note = judge(ex, wb, cc)
    assert not ok and note == "the exception in pure-rust.txt:1 needs wayland-backend without `log`, and `log` is on"
    assert judge(ex, Node("ring", "1", frozenset()), cc) is None  # via mismatch
    own = parse_exceptions("libz-sys with static bundled\n", "t")[0][0]
    assert judge(own, wb, Node("libz-sys", "1", frozenset({"static"}))) == (True, "")
    assert judge(own, wb, Node("libz-sys", "1", frozenset()))[0] is False  # about the crate itself, no via
    host_target = (
        "0app v1 (/a)|\n1wayland-backend v0.3.17|log\n2cc v1.4.7|\n1wayland-backend v0.3.17|dlopen\n2cc v1.4.7|\n")
    shared = parse_exceptions("cc via wayland-backend without log C only with log\n", "pure-rust.txt")[0]
    lines, code = analyze(tree(host_target), shared, [])
    assert code == 1 and lines[0] == (
        "pure-rust: `cc` (pulled in by `wayland-backend`) on all 6 targets: app -> wayland-backend -> cc "
        "(the exception in pure-rust.txt:1 needs wayland-backend without `log`, and `log` is on)"), lines[0]
    assert len(lines) == 3 and lines[1].startswith("pure-rust: these need a C toolchain")
    assert "scripts/pure-rust.allow" in lines[2] and "cargo tree -i <crate>" in lines[2]
    lines, code = analyze(tree("0app v1 (/a)|\n1wayland-backend v0.3.17|dlopen\n2cc v1.4.7|\n"), shared, [])
    assert (code, lines) == (0, ["pure-rust: OK (6 targets, 1 exceptions used)"])
    # the shared list's own `cc via wayland-backend without log` against kura's real cut
    real_shared = parse_exceptions(read_text(os.path.join(HERE, "pure-rust.txt")), SHARED)[0]
    lines, code = analyze(tree(REAL), real_shared, [])
    assert code == 0, lines

    # stale: an app line that matches nothing; a shared line never
    app = parse_exceptions("zzz-sys gone\ncc via wayland-backend nothing wrong here\n", APP)[0]
    lines, code = analyze(tree(REAL), real_shared + parse_exceptions("nope-sys never used\n", "pure-rust.txt")[0], app)
    assert code == 1 and lines == ["pure-rust: scripts/pure-rust.allow:1 (`zzz-sys`) matches nothing; drop the line"], lines
    assert analyze(tree(REAL), real_shared + parse_exceptions("nope-sys never used\n", "pure-rust.txt")[0], app[1:])[1] == 0

    # grouping and path: the same (child, parent) on some targets prints once; the path is the shortest from a root
    deep = "0app v1 (/a)|\n1a v1|\n2b v1|\n3ring v1|\n4cc v1|\n1c v1|\n2ring v1|\n3cc v1|\n"
    mixed = {t: deep for t in TARGETS[2:4]}
    mixed.update({t: "0app v1 (/a)|\n" for t in TARGETS if t not in mixed})
    lines, code = analyze(mixed, [], [])
    assert lines[0] == (
        "pure-rust: `cc` (pulled in by `ring`) on x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu: app -> c -> ring -> cc"), lines[0]
    assert sum(1 for line in lines if "`cc`" in line) == 1
    assert shortest_path([(Node("a", "1", frozenset()), Node("b", "1", frozenset()))], ["a"], "b", "cc") == ["a", "b", "cc"]
    assert shortest_path([], ["a"], "zz", "cc") == []
    # the target list is only the real ones; a tree of members alone is OK
    assert analyze(tree("0app v1 (/a)|\n"), [], [])[0] == ["pure-rust: OK (6 targets, 0 exceptions used)"]

    # a tree it cannot read fails, never OK: a changed format, an orphan line, no member at all
    unreadable = [("app v1|\n", "cannot read the line"), ("0app v1 (/a)|\n2cc v1|\n", "no parent at depth 1"), ("", "no workspace member")]
    for text, why in unreadable:
        lines, code = analyze(tree(text), [], [])
        assert code == 1 and why in lines[0], lines
        assert lines[0].startswith("pure-rust: cannot read the cargo tree of x86_64-pc-windows-msvc:"), lines

    # run(): a rejected line in either file fails, and there is then no OK line
    ok_tree = tree("0app v1 (/a)|\n1foo-sys v1|\n")
    assert run(ok_tree, "foo-sys bindings only\n", "") == (["pure-rust: OK (6 targets, 1 exceptions used)"], 0)
    lines, code = run(ok_tree, "foo-sys bindings only\nbar-sys\n", "")
    assert (code, lines) == (1, ["pure-rust: pure-rust.txt:2: no reason; the line does not count"]), lines
    lines, code = run(ok_tree, "foo-sys bindings only\n", "cc nope\n")
    assert code == 1 and len(lines) == 1 and lines[0].startswith("pure-rust: scripts/pure-rust.allow:1: `cc` is a build helper"), lines
    lines, code = run(ok_tree, "", "bar-sys via\n")
    assert code == 1 and "malformed line" in lines[0] and "`foo-sys` (pulled in by `app`)" in lines[1], lines
    assert run(ok_tree, "﻿foo-sys bindings only\n", "")[1] == 1  # a BOM is the caller's (read_text drops it)

    # the shared list parses clean and carries the two lines kura needs
    entries, errs = parse_exceptions(read_text(os.path.join(HERE, "pure-rust.txt")), SHARED)
    assert not errs, errs
    assert any(e.crate == "cc" and e.via == "wayland-backend" and e.cond == ("without", "log") for e in entries)
    assert any(e.crate == "pkg-config" and e.via == "wayland-sys" and e.cond == ("with", "dlopen") for e in entries)
    assert any(e.crate == "wayland-sys" and e.via is None and e.cond == ("with", "dlopen") for e in entries)
    print("pure-rust: self-test OK")


if __name__ == "__main__":
    sys.stdout.reconfigure(errors="replace")  # cargo's stderr on a pipe with a narrow code page (cp932) must not crash it
    if sys.argv[1:] == ["--self-test"]:
        self_test()
    else:
        sys.exit(check())
