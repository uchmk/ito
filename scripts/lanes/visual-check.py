#!/usr/bin/env python3
"""visual-check.py: check one run folder against a check spec and write machine/assertions.json (assertions.v1).

    python3 scripts/visual-check.py --spec tests/visual/41.15.toml --run <run dir>
    python3 scripts/visual-check.py --self-test [--emit DIR]

Exit 0 PASS, 1 FAIL, 2 ERROR (broken evidence, a bad spec, a usage error). Stdlib only, Python 3.11+.
This file is copied byte for byte into every app by `lanes.sh sync`: it reads nothing of ito's and depends on no layout.
The rules are in ito's docs/visual-test.md (d6 the check spec, d7 the evidence folder); the code below is the exact reading.
"""
import sys

if sys.version_info < (3, 11):
    sys.stderr.write("visual-check.py: Python 3.11 or newer is required (this is %d.%d)\n" % sys.version_info[:2])
    sys.exit(2)

import argparse
import copy
import hashlib
import itertools
import json
import math
import os
import re
import shutil
import struct
import subprocess
import tempfile
import tomllib
import zlib
from collections import Counter, namedtuple

TOOL = {"name": "visual-check.py", "version": 1}
EPS = 1e-3
RUN_ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
ROW_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z._-]{0,63}$")
NAME_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
POINT_ID_RE = re.compile(r"^[A-Za-z0-9._-]+$")
HASH_RE = re.compile(r"^[0-9a-f]{64}$")
TOKEN_RE = re.compile(r"^#[0-9a-fA-F]{8}$")
HEX6_RE = re.compile(r"^#[0-9a-fA-F]{6}$")
CAPS = ("ui", "window", "theme", "lang", "screen", "overlay", "toasts", "app_state")


# ---------------------------------------------------------------------------------------------------------------------
# JSON and files
# ---------------------------------------------------------------------------------------------------------------------


def _no_const(name):
    raise ValueError("non-finite number %s" % name)


def _pairs(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError("duplicate key %r" % k)
        d[k] = v
    return d


def parse_json(data):
    """Bytes -> value. NaN / Infinity and duplicate keys are errors (ValueError)."""
    return json.loads(data.decode("utf-8"), parse_constant=_no_const, object_pairs_hook=_pairs)


def dumps(obj):
    return json.dumps(obj, ensure_ascii=False, indent=2, sort_keys=False, allow_nan=False) + "\n"


def sha256_hex(data):
    return hashlib.sha256(data).hexdigest()


def write_atomic(path, data):
    """`<file>.part` -> flush -> fsync -> replace; a failed write removes the .part."""
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


def write_text_lf(path, text):
    write_atomic(path, text.encode("utf-8"))


def isint(v):
    return isinstance(v, int) and not isinstance(v, bool)


def isnum(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def strict_eq(a, b):
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return a == b
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(strict_eq(x, y) for x, y in zip(a, b))
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(strict_eq(a[k], b[k]) for k in a)
    return type(a) is type(b) and a == b


def deep_diff(a, b, path="", skip=()):
    """The first key path where a and b differ (None when equal). `skip` names top-level keys to ignore."""
    if isinstance(a, dict) and isinstance(b, dict):
        for k in list(a) + [k for k in b if k not in a]:
            if not path and k in skip:
                continue
            p = path + "." + k if path else k
            if k not in a or k not in b:
                return p
            d = deep_diff(a[k], b[k], p)
            if d:
                return d
        return None
    if isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            return path + "[len]"
        for i, (x, y) in enumerate(zip(a, b)):
            d = deep_diff(x, y, "%s[%d]" % (path, i))
            if d:
                return d
        return None
    return None if strict_eq(a, b) else path


def repo_path(file_path):
    """The path relative to the repository's top level (`/`), or the path as given outside a repository."""
    given = file_path.replace("\\", "/")
    try:
        folder = os.path.dirname(os.path.abspath(file_path))
        r = subprocess.run(
            ["git", "-C", folder, "rev-parse", "--show-prefix"], capture_output=True, text=True, timeout=30, stdin=subprocess.DEVNULL
        )
        if r.returncode == 0:
            return r.stdout.strip().replace("\\", "/") + os.path.basename(file_path)
    except (OSError, subprocess.SubprocessError):
        pass
    return given


# ---------------------------------------------------------------------------------------------------------------------
# PNG (stdlib reader and a small writer for the self-test)
# ---------------------------------------------------------------------------------------------------------------------

PNG_SIG = b"\x89PNG\r\n\x1a\n"


class PngError(Exception):
    pass


def _add_bytes(a, b):
    """Bytewise (a + b) mod 256 of two equal-length byte strings, as big-int arithmetic."""
    n = len(a)
    if n == 0:
        return b""
    lo = int.from_bytes(b"\x7f" * n, "little")
    hi = int.from_bytes(b"\x80" * n, "little")
    x = int.from_bytes(a, "little")
    y = int.from_bytes(b, "little")
    return (((x & lo) + (y & lo)) ^ ((x ^ y) & hi)).to_bytes(n, "little")


_M255 = (255).__and__


def unfilter(raw, width, height, bpp):
    stride = width * bpp
    out = bytearray(height * stride)
    prev = bytes(stride)
    pos = 0
    for y in range(height):
        ft = raw[pos]
        line = raw[pos + 1 : pos + 1 + stride]
        pos += 1 + stride
        if ft == 0:
            cur = bytes(line)
        elif ft == 1:
            c = bytearray(stride)
            for k in range(bpp):
                c[k::bpp] = bytes(map(_M255, itertools.accumulate(line[k::bpp])))
            cur = bytes(c)
        elif ft == 2:
            cur = _add_bytes(line, prev)
        elif ft == 3:
            c = bytearray(stride)
            for i in range(min(bpp, stride)):
                c[i] = (line[i] + (prev[i] >> 1)) & 255
            for i in range(bpp, stride):
                c[i] = (line[i] + ((c[i - bpp] + prev[i]) >> 1)) & 255
            cur = bytes(c)
        elif ft == 4:
            c = bytearray(stride)
            for i in range(min(bpp, stride)):
                c[i] = (line[i] + prev[i]) & 255
            for i in range(bpp, stride):
                a = c[i - bpp]
                b = prev[i]
                cc = prev[i - bpp]
                pa = b - cc
                pb = a - cc
                pc = pa + pb
                if pa < 0:
                    pa = -pa
                if pb < 0:
                    pb = -pb
                if pc < 0:
                    pc = -pc
                if pa <= pb and pa <= pc:
                    pr = a
                elif pb <= pc:
                    pr = b
                else:
                    pr = cc
                c[i] = (line[i] + pr) & 255
            cur = bytes(c)
        else:
            raise PngError("bad filter type %d in row %d" % (ft, y))
        out[y * stride : (y + 1) * stride] = cur
        prev = cur
    return out


def decode_png(data):
    """-> (width, height, rgba bytearray). Raises PngError."""
    if data[:8] != PNG_SIG:
        raise PngError("not a PNG (bad signature)")
    pos = 8
    ihdr = None
    idat = []
    ended = False
    first = True
    while pos < len(data):
        if ended:
            raise PngError("data after IEND")
        if pos + 8 > len(data):
            raise PngError("truncated chunk header")
        length, ctype = struct.unpack(">I4s", data[pos : pos + 8])
        end = pos + 8 + length + 4
        if end > len(data):
            raise PngError("truncated %s chunk" % ctype.decode("latin-1"))
        body = data[pos + 8 : pos + 8 + length]
        (crc,) = struct.unpack(">I", data[end - 4 : end])
        if zlib.crc32(ctype + body) & 0xFFFFFFFF != crc:
            raise PngError("bad CRC in %s chunk" % ctype.decode("latin-1"))
        if first and ctype != b"IHDR":
            raise PngError("IHDR is not the first chunk")
        first = False
        if ctype == b"IHDR":
            if ihdr is not None or length != 13:
                raise PngError("bad IHDR")
            ihdr = struct.unpack(">IIBBBBB", body)
        elif ctype == b"IDAT":
            idat.append(body)
        elif ctype == b"IEND":
            if length != 0:
                raise PngError("bad IEND")
            ended = True
        elif ctype[0:1].isupper() and ctype != b"PLTE":
            raise PngError("unsupported PNG (critical chunk %s)" % ctype.decode("latin-1"))
        pos = end
    if ihdr is None:
        raise PngError("no IHDR")
    if not ended:
        raise PngError("no IEND (truncated)")
    if not idat:
        raise PngError("no IDAT")
    width, height, depth, ctype_, comp, flt, interlace = ihdr
    if not (1 <= width <= 16384 and 1 <= height <= 16384):
        raise PngError("unsupported PNG (size %dx%d)" % (width, height))
    if depth != 8:
        raise PngError("unsupported PNG (bit depth %d, only 8)" % depth)
    if ctype_ not in (2, 6):
        raise PngError("unsupported PNG (colour type %d, only 2 and 6)" % ctype_)
    if comp != 0 or flt != 0:
        raise PngError("unsupported PNG (compression %d, filter %d)" % (comp, flt))
    if interlace != 0:
        raise PngError("unsupported PNG (interlaced)")
    ch = 4 if ctype_ == 6 else 3
    want = height * (1 + width * ch)
    d = zlib.decompressobj()
    try:
        raw = d.decompress(b"".join(idat), want + 1)
    except zlib.error as e:
        raise PngError("bad IDAT data (%s)" % e)
    if len(raw) > want or d.unconsumed_tail:
        raise PngError("IDAT has more data than %d bytes" % want)
    if len(raw) < want:
        raise PngError("IDAT has less data than %d bytes (got %d)" % (want, len(raw)))
    if not d.eof:
        raise PngError("IDAT zlib stream is not finished")
    if d.unused_data:
        raise PngError("data after the IDAT zlib stream")
    pix = unfilter(raw, width, height, ch)
    if ch == 4:
        return width, height, pix
    rgba = bytearray(b"\xff" * (width * height * 4))
    rgba[0::4] = pix[0::3]
    rgba[1::4] = pix[1::3]
    rgba[2::4] = pix[2::3]
    return width, height, rgba


def png_chunk(ctype, body):
    return struct.pack(">I", len(body)) + ctype + body + struct.pack(">I", zlib.crc32(ctype + body) & 0xFFFFFFFF)


def encode_png(width, height, pix, channels=4, depth=8, interlace=0, raw=None, idat_data=None):
    """A PNG with filter 0 per row (or `raw` filtered scanlines as given, or `idat_data` verbatim)."""
    ct = 6 if channels == 4 else 2
    stride = width * channels
    if raw is None:
        raw = b"".join(b"\x00" + bytes(pix[y * stride : (y + 1) * stride]) for y in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, depth, ct, 0, 0, interlace)
    z = zlib.compress(raw) if idat_data is None else idat_data
    return PNG_SIG + png_chunk(b"IHDR", ihdr) + png_chunk(b"IDAT", z) + png_chunk(b"IEND", b"")


# ---------------------------------------------------------------------------------------------------------------------
# Colour: sRGB -> Lab, CIEDE2000
# ---------------------------------------------------------------------------------------------------------------------

_LAB_EPS = 216.0 / 24389.0
_LAB_KAPPA = 24389.0 / 27.0
_LAB_CACHE = {}


def srgb_to_lab(r, g, b):
    key = (r, g, b)
    hit = _LAB_CACHE.get(key)
    if hit is not None:
        return hit

    def lin(c):
        c = c / 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    rl, gl, bl = lin(r), lin(g), lin(b)
    x = 0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl
    y = 0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl
    z = 0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl

    def f(t):
        return t ** (1.0 / 3.0) if t > _LAB_EPS else (_LAB_KAPPA * t + 16.0) / 116.0

    fx, fy, fz = f(x / 0.95047), f(y / 1.0), f(z / 1.08883)
    lab = (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
    _LAB_CACHE[key] = lab
    return lab


def ciede2000(lab1, lab2):
    l1, a1, b1 = lab1
    l2, a2, b2 = lab2
    c1 = math.hypot(a1, b1)
    c2 = math.hypot(a2, b2)
    cbar7 = ((c1 + c2) / 2.0) ** 7
    g = 0.5 * (1.0 - math.sqrt(cbar7 / (cbar7 + 25.0**7)))
    a1p = (1.0 + g) * a1
    a2p = (1.0 + g) * a2
    c1p = math.hypot(a1p, b1)
    c2p = math.hypot(a2p, b2)
    h1p = 0.0 if (b1 == 0 and a1p == 0) else math.degrees(math.atan2(b1, a1p)) % 360.0
    h2p = 0.0 if (b2 == 0 and a2p == 0) else math.degrees(math.atan2(b2, a2p)) % 360.0
    dlp = l2 - l1
    dcp = c2p - c1p
    if c1p * c2p == 0:
        dhp = 0.0
    else:
        dhp = h2p - h1p
        if dhp > 180.0:
            dhp -= 360.0
        elif dhp < -180.0:
            dhp += 360.0
    dhh = 2.0 * math.sqrt(c1p * c2p) * math.sin(math.radians(dhp / 2.0))
    lbar = (l1 + l2) / 2.0
    cbar = (c1p + c2p) / 2.0
    if c1p * c2p == 0:
        hbar = h1p + h2p
    elif abs(h1p - h2p) <= 180.0:
        hbar = (h1p + h2p) / 2.0
    elif h1p + h2p < 360.0:
        hbar = (h1p + h2p + 360.0) / 2.0
    else:
        hbar = (h1p + h2p - 360.0) / 2.0
    t = (
        1.0
        - 0.17 * math.cos(math.radians(hbar - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hbar))
        + 0.32 * math.cos(math.radians(3.0 * hbar + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hbar - 63.0))
    )
    dtheta = 30.0 * math.exp(-(((hbar - 275.0) / 25.0) ** 2))
    cbar7p = cbar**7
    rc = 2.0 * math.sqrt(cbar7p / (cbar7p + 25.0**7))
    sl = 1.0 + 0.015 * (lbar - 50.0) ** 2 / math.sqrt(20.0 + (lbar - 50.0) ** 2)
    sc = 1.0 + 0.045 * cbar
    sh = 1.0 + 0.015 * cbar * t
    rt = -math.sin(math.radians(2.0 * dtheta)) * rc
    x = dlp / sl
    y = dcp / sc
    z = dhh / sh
    return math.sqrt(x * x + y * y + z * z + rt * y * z)


# ---------------------------------------------------------------------------------------------------------------------
# Runtime shape of the evidence files
# ---------------------------------------------------------------------------------------------------------------------

# type words: str int num bool hash rect any proj cap tokens ui, "?x" nullable, "list:x", "nums:N", "ints:N", "obj:NAME"
SHAPES = {
    "state": (
        {
            "schema_version": "int", "name": "str", "run_id": "?str", "snapshot_seq": "int", "phase": "?str",
            "capture_id": "?str", "after_step": "?int", "after_label": "?str", "frame": "int", "settled": "?bool",
            "app": "obj:app", "theme": "obj:theme", "lang": "obj:lang", "window": "obj:window", "screen": "?str",
            "overlay": "?str", "toasts": "?list:obj:toast", "app_state": "any", "ui": "list:ui",
            "capabilities": "list:cap", "stable": "obj:stable",
        },
        False,
    ),
    "app": ({"name": "?str", "version": "?str"}, False),
    "theme": ({"requested": "?str", "name": "?str", "dark": "?bool", "tokens": "?tokens", "tokens_sha256": "?hash"}, False),
    "lang": ({"requested": "?str", "in_force": "?str"}, False),
    "window": ({"inner_px": "rect", "pixels_per_point": "num", "monitor_work_area_px": "?rect"}, False),
    "toast": ({"text": "str", "kind": "str"}, False),
    "stable": ({"projection": "proj", "ui_sha256": "hash", "app_sha256": "hash"}, True),
    "side": (
        {
            "name": "str", "app": "str", "version": "str", "theme": "str", "theme_requested": "str", "lang": "str",
            "lang_requested": "str", "size": "nums:2", "zoom": "num", "state": "str", "component": "str",
            "rect": "nums:4", "time": "num", "pixels_per_point": "num", "run_id": "?str", "snapshot_seq": "?int",
            "capture_id": "str", "png": "str", "png_sha256": "hash", "size_px": "ints:2", "state_before": "?obj:sref",
            "state_after": "?obj:sref", "state_unchanged": "?bool", "requested_frame": "int", "received_frame": "int",
            "env": "obj:env",
        },
        False,
    ),
    "sref": ({"path": "str", "sha256": "hash", "stable": "obj:stable"}, True),
    "env": (
        {"os": "str", "arch": "str", "wgpu_backend": "?str", "font_set_sha256": "?hash", "theme": "str", "pixels_per_point": "num"},
        True,
    ),
    "trace": (
        {
            "schema_version": "int", "run_id": "str", "step": "?int", "label": "str", "outcome": "str", "settled": "?bool",
            "frame": "int", "time_ms": "num", "end_frame": "int", "end_ms": "num", "why": "?str",
        },
        False,
    ),
    "run": ({"schema": "int", "run_id": "?str", "steps": "int", "failed": "bool", "left": "int", "status": "str"}, False),
    "ui_ok": (
        {
            "test_id": "str", "role": "str", "text": "?str", "label": "?str", "value": "?str", "rect_px": "rect",
            "clip_px": "?rect", "visible": "?bool", "elided": "?bool", "enabled": "bool", "focused": "bool",
            "hidden": "bool", "layer": "str",
        },
        True,
    ),
    "ui_err": ({"test_id": "str", "error": "str"}, True),
}


def check_type(v, t, path, errs):
    if t.startswith("?"):
        if v is None:
            return
        t = t[1:]
    if t == "str":
        ok = isinstance(v, str)
    elif t == "int":
        ok = isint(v)
    elif t == "num":
        ok = isnum(v)
    elif t == "bool":
        ok = isinstance(v, bool)
    elif t == "hash":
        ok = isinstance(v, str) and HASH_RE.match(v) is not None
    elif t == "any":
        ok = True
    elif t == "proj":
        ok = isint(v) and v == 1
    elif t == "cap":
        ok = isinstance(v, str) and v in CAPS
    elif t == "rect":
        ok = isinstance(v, list) and len(v) == 4 and all(isnum(x) for x in v) and v[0] <= v[2] and v[1] <= v[3]
    elif t == "tokens":
        ok = isinstance(v, dict) and all(isinstance(k, str) and isinstance(x, str) and TOKEN_RE.match(x) for k, x in v.items())
    elif t.startswith("nums:"):
        ok = isinstance(v, list) and len(v) == int(t[5:]) and all(isnum(x) for x in v)
    elif t.startswith("ints:"):
        ok = isinstance(v, list) and len(v) == int(t[5:]) and all(isint(x) for x in v)
    elif t.startswith("list:"):
        if not isinstance(v, list):
            errs.append("%s: expected a list" % path)
            return
        for i, x in enumerate(v):
            check_type(x, t[5:], "%s[%d]" % (path, i), errs)
        return
    elif t.startswith("obj:"):
        check_obj(v, t[4:], path, errs)
        return
    elif t == "ui":
        if not isinstance(v, dict):
            errs.append("%s: expected an object" % path)
        else:
            check_obj(v, "ui_err" if "error" in v else "ui_ok", path, errs)
        return
    else:
        raise AssertionError("unknown type word " + t)
    if not ok:
        errs.append("%s: expected %s, got %s" % (path, t, json.dumps(v, ensure_ascii=False)[:60]))


def check_obj(v, shape, path, errs):
    fields, closed = SHAPES[shape]
    if not isinstance(v, dict):
        errs.append("%s: expected an object" % path)
        return
    for k, t in fields.items():
        if k not in v:
            errs.append("%s.%s: missing" % (path, k))
        else:
            check_type(v[k], t, "%s.%s" % (path, k), errs)
    if closed:
        for k in v:
            if k not in fields:
                errs.append("%s.%s: unknown key" % (path, k))


def shape_errors(doc, shape, label):
    errs = []
    check_obj(doc, shape, "$", errs)
    return ["%s: %s" % (label, e) for e in errs]


# ---------------------------------------------------------------------------------------------------------------------
# The check spec
# ---------------------------------------------------------------------------------------------------------------------


def v_bool(v, w):
    return [] if isinstance(v, bool) else ["%s: expected true or false" % w]


def v_true(v, w):
    return [] if v is True else ["%s: expected true" % w]


def v_str(v, w):
    return [] if isinstance(v, str) else ["%s: expected a string" % w]


def v_str_or_contains(v, w):
    if isinstance(v, str):
        return []
    if isinstance(v, dict):
        e = ["%s: unknown key `%s`" % (w, k) for k in v if k != "contains"]
        if "contains" not in v:
            e.append("%s: expected a string or { contains = \"...\" }" % w)
        elif not isinstance(v["contains"], str):
            e.append("%s.contains: expected a string" % w)
        return e
    return ["%s: expected a string or { contains = \"...\" }" % w]


def v_range(v, w, need_min=False, min_pos=False):
    if not isinstance(v, dict):
        return ["%s: expected a range table like { min = 0, max = 2 }" % w]
    e = ["%s: unknown key `%s`" % (w, k) for k in v if k not in ("min", "max")]
    if not v:
        e.append("%s: empty range table" % w)
    for k in ("min", "max"):
        if k in v and not isnum(v[k]):
            e.append("%s.%s: expected a finite number" % (w, k))
    if e:
        return e
    if need_min and "min" not in v:
        e.append("%s: `min` is required (a range with only `max` passes any protrusion)" % w)
    if min_pos and not (v.get("min", 0) > 0):
        e.append("%s: `min` must be present and > 0" % w)
    if "min" in v and "max" in v and v["min"] > v["max"]:
        e.append("%s: min > max" % w)
    return e


def v_range_min(v, w):
    return v_range(v, w, need_min=True)


def v_range_pos(v, w):
    return v_range(v, w, need_min=True, min_pos=True)


def v_toast(v, w):
    if not isinstance(v, dict):
        return ["%s: expected { text = ..., kind = ... }" % w]
    e = ["%s: unknown key `%s`" % (w, k) for k in v if k not in ("text", "kind")]
    if not v:
        e.append("%s: empty toast table" % w)
    if "text" in v:
        e += v_str_or_contains(v["text"], w + ".text")
    if "kind" in v and not isinstance(v["kind"], str):
        e.append("%s.kind: expected a string" % w)
    return e


def v_app_state(v, w):
    if not isinstance(v, dict) or not v:
        return ["%s: expected a non-empty table" % w]
    return []


def v_id(v, w):
    return [] if isinstance(v, str) and v else ["%s: expected a test id string" % w]


def v_aligned(v, w):
    if not isinstance(v, dict):
        return ["%s: expected { with = ..., edge = ..., tol_px = ... }" % w]
    e = ["%s: unknown key `%s`" % (w, k) for k in v if k not in ("with", "edge", "tol_px")]
    for k in ("with", "edge", "tol_px"):
        if k not in v:
            e.append("%s: `%s` is required" % (w, k))
    if e:
        return e
    e += v_id(v["with"], w + ".with")
    if v["edge"] not in ("left", "right", "top", "bottom", "center_x", "center_y"):
        e.append("%s.edge: expected left, right, top, bottom, center_x or center_y" % w)
    if not isnum(v["tol_px"]) or v["tol_px"] < 0:
        e.append("%s.tol_px: expected a number >= 0" % w)
    return e


def v_no_overlap(v, w):
    if not isinstance(v, list) or not v:
        return ["%s: expected a non-empty list of test ids" % w]
    return [x for i, item in enumerate(v) for x in v_id(item, "%s[%d]" % (w, i))]


def v_gap(v, w):
    if not isinstance(v, dict):
        return ["%s: expected { to = ..., side = ..., min = ... }" % w]
    e = ["%s: unknown key `%s`" % (w, k) for k in v if k not in ("to", "side", "min", "max")]
    for k in ("to", "side"):
        if k not in v:
            e.append("%s: `%s` is required" % (w, k))
    if e:
        return e
    e += v_id(v["to"], w + ".to")
    if v["side"] not in ("right", "left", "above", "below"):
        e.append("%s.side: expected right, left, above or below" % w)
    rng = {k: v[k] for k in ("min", "max") if k in v}
    e += v_range_min(rng, w)
    return e


def _is_rect_literal(x):
    return isinstance(x, list) and len(x) == 4 and all(isnum(n) for n in x) and x[0] <= x[2] and x[1] <= x[3]


def v_color(v, w):
    if not isinstance(v, dict):
        return ["%s: expected a table" % w]
    allowed = ("token", "delta_e_max", "min_fraction", "inset_px", "over", "mask", "max_mask_fraction")
    e = ["%s: unknown key `%s`" % (w, k) for k in v if k not in allowed]
    for k in ("token", "delta_e_max", "min_fraction"):
        if k not in v:
            e.append("%s: `%s` is required" % (w, k))
    if e:
        return e
    if not (isinstance(v["token"], str) and v["token"]):
        e.append("%s.token: expected a token name" % w)
    if not (isnum(v["delta_e_max"]) and v["delta_e_max"] > 0):
        e.append("%s.delta_e_max: expected a number > 0" % w)
    if not (isnum(v["min_fraction"]) and 0 < v["min_fraction"] <= 1):
        e.append("%s.min_fraction: expected 0 < x <= 1" % w)
    if "inset_px" in v and not (isnum(v["inset_px"]) and v["inset_px"] >= 0):
        e.append("%s.inset_px: expected a number >= 0" % w)
    if "over" in v and not (isinstance(v["over"], str) and v["over"]):
        e.append("%s.over: expected a token name or \"#rrggbb\"" % w)
    if ("mask" in v) != ("max_mask_fraction" in v):
        e.append("%s: `mask` and `max_mask_fraction` go together" % w)
    if "mask" in v:
        if not isinstance(v["mask"], list) or not v["mask"]:
            e.append("%s.mask: expected a non-empty list of test ids or [l, t, r, b] rects" % w)
        else:
            for i, m in enumerate(v["mask"]):
                if not ((isinstance(m, str) and m) or _is_rect_literal(m)):
                    e.append("%s.mask[%d]: expected a test id or [l, t, r, b] with l <= r and t <= b" % (w, i))
        mf = v["max_mask_fraction"]
        if not (isnum(mf) and 0 < mf <= 0.5):
            e.append("%s.max_mask_fraction: expected 0 < x <= 0.5 (a hard cap)" % w)
    return e


Check = namedtuple("Check", "fn validate target relative snap cap")


class Fail(Exception):
    def __init__(self, why, measured=None):
        super().__init__(why)
        self.measured = measured


class CheckError(Exception):
    pass


def has_area(r):
    return r[2] - r[0] > EPS and r[3] - r[1] > EPS


def rect_in(inner, outer):
    return inner[0] >= outer[0] - EPS and inner[1] >= outer[1] - EPS and inner[2] <= outer[2] + EPS and inner[3] <= outer[3] + EPS


def rnd(v):
    if isinstance(v, float):
        v = round(v, 6)
        return 0.0 if v == 0 else v
    return v


class PointCtx:
    def __init__(self, spec, pt, doc, snap):
        self.spec, self.pt, self.doc, self.snap = spec, pt, doc, snap
        self.caps = set(doc["capabilities"])
        self.ui = {e["test_id"]: e for e in doc["ui"]}

    def entry(self, tid):
        e = self.ui.get(tid)
        if e is not None and "error" in e:
            raise CheckError("ui entry `%s` is an error: %s" % (tid, e["error"]))
        return e

    def target(self):
        e = self.entry(self.pt["target"])
        if e is None:
            raise Fail("target not present")
        return e

    def trect(self, area=True):
        r = self.target()["rect_px"]
        if area and not has_area(r):
            raise Fail("%s has no area" % self.pt["target"])
        return r

    def orect(self, tid, area=True):
        e = self.entry(tid)
        if e is None:
            raise CheckError("`%s` is not present in ui" % tid)
        r = e["rect_px"]
        if area and not has_area(r):
            raise Fail("%s has no area" % tid)
        return r

    def window_rect(self):
        w = self.doc["window"]["inner_px"]
        return [0, 0, w[2], w[3]]


def range_result(v, rng):
    why = None
    if "min" in rng and v < rng["min"] - EPS:
        why = "%s < min %s" % (rnd(v), rng["min"])
    elif "max" in rng and v > rng["max"] + EPS:
        why = "%s > max %s" % (rnd(v), rng["max"])
    return why is None, rnd(v), why


def text_match(actual, want):
    if isinstance(want, dict):
        return actual is not None and want["contains"] in actual
    return actual == want


def subset_match(want, actual):
    if isinstance(want, dict):
        return isinstance(actual, dict) and all(k in actual and subset_match(x, actual[k]) for k, x in want.items())
    return strict_eq(want, actual)


def _bool_field(field, null_is_error):
    def fn(pc, want):
        v = pc.target()[field]
        if v is None:
            if null_is_error:
                raise CheckError("`%s` of the target is null (not known)" % field)
            return False, None, "%s is null" % field
        return v == want, v, None if v == want else "%s is %s" % (field, str(v).lower())

    return fn


def _text_field(field):
    def fn(pc, want):
        v = pc.target()[field]
        ok = text_match(v, want)
        return ok, v, None if ok else ("%s is null" % field if v is None else "%s differs" % field)

    return fn


def c_present(pc, want):
    e = pc.entry(pc.pt["target"])
    here = e is not None
    return here == want, here, None if here == want else ("target is present" if here else "target not present")


def c_role(pc, want):
    v = pc.target()["role"]
    return v == want, v, None if v == want else "role is %r" % v


def c_screen(pc, want):
    v = pc.doc["screen"]
    return v == want, v, None if v == want else "screen is %r" % v


def c_overlay(pc, want):
    v = pc.doc["overlay"]
    return v == want, v, None if v == want else "overlay is %r" % v


def c_no_overlay(pc, want):
    v = pc.doc["overlay"]
    return v is None, v, None if v is None else "overlay %r is open" % v


def c_toast(pc, want):
    toasts = pc.doc["toasts"]
    shown = json.dumps(toasts, ensure_ascii=False)
    for t in toasts:
        if ("text" not in want or text_match(t["text"], want["text"])) and ("kind" not in want or t["kind"] == want["kind"]):
            return True, shown, None
    return False, shown, "no toast matches"


def c_no_toast(pc, want):
    n = len(pc.doc["toasts"])
    return n == 0, n, None if n == 0 else "%d toast(s) on screen" % n


def c_app_state(pc, want):
    actual = pc.doc["app_state"]
    shown = json.dumps(actual, ensure_ascii=False)
    if len(shown) > 2000:
        shown = shown[:2000] + "..."
    ok = subset_match(want, actual)
    return ok, shown, None if ok else "app_state does not contain the expected subset"


def c_theme(pc, want):
    v = pc.doc["theme"]["name"]
    return v == want, v, None if v == want else "theme is %r" % v


def c_dark(pc, want):
    v = pc.doc["theme"]["dark"]
    if v is None:
        raise CheckError("theme.dark is null (not known)")
    return v == want, v, None if v == want else "dark is %s" % str(v).lower()


def c_width(pc, rng):
    r = pc.trect(area=False)
    return range_result(r[2] - r[0], rng)


def c_height(pc, rng):
    r = pc.trect(area=False)
    return range_result(r[3] - r[1], rng)


def _gap(fn):
    def run(pc, rng):
        t = pc.trect()
        r = pc.orect(pc.pt["relative_to"])
        return range_result(fn(t, r), rng)

    return run


def c_inside(pc, want):
    t = pc.trect()
    r = pc.orect(pc.pt["relative_to"])
    ok = rect_in(t, r)
    return ok, ok, None if ok else "target is not inside %s" % pc.pt["relative_to"]


def c_unclipped(pc, want):
    t = pc.trect()
    clip = pc.target()["clip_px"]
    if clip is None:
        raise CheckError("the target has no clip_px (null)")
    ok = rect_in(t, clip)
    return ok, ok, None if ok else "target is cut by its clip"


def c_in_window(pc, want):
    t = pc.trect()
    ok = rect_in(t, pc.window_rect())
    return ok, ok, None if ok else "target is outside the window"


def _edge(r, name):
    return {"left": r[0], "right": r[2], "top": r[1], "bottom": r[3], "center_x": (r[0] + r[2]) / 2.0, "center_y": (r[1] + r[3]) / 2.0}[name]


def c_aligned(pc, want):
    t = pc.trect()
    w = pc.orect(want["with"])
    d = abs(_edge(t, want["edge"]) - _edge(w, want["edge"]))
    ok = d <= want["tol_px"] + EPS
    return ok, rnd(d), None if ok else "%s differs by %s > tol %s" % (want["edge"], rnd(d), want["tol_px"])


def _intersection_area(a, b):
    w = min(a[2], b[2]) - max(a[0], b[0])
    h = min(a[3], b[3]) - max(a[1], b[1])
    return w * h if w > 0 and h > 0 else 0.0


def c_no_overlap(pc, want):
    t = pc.trect()
    worst = 0.0
    who = None
    for tid in want:
        area = _intersection_area(t, pc.orect(tid))
        if area > worst:
            worst, who = area, tid
    ok = worst <= EPS
    return ok, rnd(worst), None if ok else "overlaps %s by %s px2" % (who, rnd(worst))


def c_gap(pc, want):
    t = pc.trect()
    o = pc.orect(want["to"])
    side = want["side"]
    v = {"right": o[0] - t[2], "left": t[0] - o[2], "above": t[1] - o[3], "below": o[1] - t[3]}[side]
    return range_result(v, {k: want[k] for k in ("min", "max") if k in want})


def _hex_rgba(s):
    s = s.lstrip("#")
    return int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), int(s[6:8], 16) if len(s) == 8 else 255


def expected_colour(pc, want):
    tokens = pc.doc["theme"]["tokens"]
    if tokens is None:
        raise CheckError("the state has no theme tokens")
    name = want["token"]
    if name not in tokens:
        raise CheckError("unknown token `%s` (known: %s)" % (name, ", ".join(sorted(tokens))))
    r, g, b, a = _hex_rgba(tokens[name])
    if a == 255:
        return r, g, b
    over = want.get("over")
    if over is None:
        raise CheckError("token `%s` is translucent (alpha %02x): `over` is required" % (name, a))
    if over.startswith("#"):
        if not HEX6_RE.match(over):
            raise CheckError("`over` %r is not \"#rrggbb\"" % over)
        dr, dg, db, _ = _hex_rgba(over)
    else:
        if over not in tokens:
            raise CheckError("unknown `over` token `%s`" % over)
        dr, dg, db, da = _hex_rgba(tokens[over])
        if da != 255:
            raise CheckError("`over` token `%s` is not opaque" % over)
    al = a / 255.0
    return tuple(int(s * al + d * (1.0 - al) + 0.5) for s, d in ((r, dr), (g, dg), (b, db)))


def c_color_token(pc, want):
    t = pc.trect()
    entry = pc.target()
    exp = expected_colour(pc, want)
    if pc.snap is None or pc.snap.png is None:
        raise CheckError("no picture")
    iw, ih, pix = pc.snap.png
    # the region: target rect, its clip, the image, shrunk by inset
    l, tp, r, b = t
    if entry["clip_px"] is not None:
        c = entry["clip_px"]
        l, tp, r, b = max(l, c[0]), max(tp, c[1]), min(r, c[2]), min(b, c[3])
    l, tp, r, b = max(l, 0), max(tp, 0), min(r, iw), min(b, ih)
    inset = want.get("inset_px", 0)
    l, tp, r, b = l + inset, tp + inset, r - inset, b - inset
    x0, x1 = max(0, math.ceil(l - 0.5)), min(iw, math.ceil(r - 0.5))
    y0, y1 = max(0, math.ceil(tp - 0.5)), min(ih, math.ceil(b - 0.5))
    if x1 <= x0 or y1 <= y0:
        raise CheckError("the region to sample is empty")
    masks = []
    for m in want.get("mask", []):
        if isinstance(m, str):
            me = pc.entry(m)
            if me is None:
                raise CheckError("mask `%s` is not present in ui" % m)
            mr = list(me["rect_px"])
            if me["clip_px"] is not None:
                c = me["clip_px"]
                mr = [max(mr[0], c[0]), max(mr[1], c[1]), min(mr[2], c[2]), min(mr[3], c[3])]
        else:
            mr = list(m)
        masks.append(mr)
    counts = Counter()
    total = 0
    masked = 0
    mv = memoryview(pix)
    for y in range(y0, y1):
        cy = y + 0.5
        ivs = []
        for ml, mt, mr_, mb in masks:
            if mt <= cy < mb:
                a, z = max(x0, math.ceil(ml - 0.5)), min(x1, math.ceil(mr_ - 0.5))
                if z > a:
                    ivs.append((a, z))
        ivs.sort()
        merged = []
        for a, z in ivs:
            if merged and a <= merged[-1][1]:
                merged[-1][1] = max(merged[-1][1], z)
            else:
                merged.append([a, z])
        total += x1 - x0
        pos = x0
        for a, z in merged + [[x1, x1]]:
            if a > pos:
                seg = mv[(y * iw + pos) * 4 : (y * iw + a) * 4]
                counts.update(seg.cast("I"))
            masked += z - a
            pos = max(pos, z)
    sampled = total - masked
    if "max_mask_fraction" in want and masked / total > want["max_mask_fraction"] + 1e-12:
        raise CheckError("mask covers %.3f of the region, more than max_mask_fraction %s" % (masked / total, want["max_mask_fraction"]))
    if sampled <= 0:
        raise CheckError("no pixel left to sample")
    exp_lab = srgb_to_lab(*exp)
    matching = 0
    worst = 0.0
    acc = 0.0
    de_cache = {}
    for v, n in counts.items():
        pr, pg, pb, pa = v.to_bytes(4, sys.byteorder)
        if pa != 255:
            raise CheckError("a sampled pixel is not opaque (alpha %d): the picture is not opaque there" % pa)
        key = (pr, pg, pb)
        de = de_cache.get(key)
        if de is None:
            de = de_cache[key] = ciede2000(srgb_to_lab(pr, pg, pb), exp_lab)
        if de <= want["delta_e_max"]:
            matching += n
        worst = max(worst, de)
        acc += de * n
    fraction = matching / sampled
    measured = {
        "sampled": sampled, "masked": masked, "matching": matching, "fraction": rnd(fraction), "worst_delta_e": rnd(worst),
        "mean_delta_e": rnd(acc / sampled), "expected_hex": "#%02x%02x%02x" % exp,
    }
    ok = fraction >= want["min_fraction"] - 1e-9
    why = None if ok else "%d/%d pixels within dE %s (%s < min_fraction %s)" % (matching, sampled, want["delta_e_max"], rnd(fraction), want["min_fraction"])
    return ok, measured, why


# name -> Check(fn, validate, needs_target, needs_relative, needs_snap, capability)
CHECKS = {
    "present": Check(c_present, v_bool, True, False, False, "ui"),
    "visible": Check(_bool_field("visible", True), v_bool, True, False, False, "ui"),
    "text": Check(_text_field("text"), v_str_or_contains, True, False, False, "ui"),
    "label": Check(_text_field("label"), v_str_or_contains, True, False, False, "ui"),
    "value": Check(_text_field("value"), v_str_or_contains, True, False, False, "ui"),
    "role": Check(c_role, v_str, True, False, False, "ui"),
    "elided": Check(_bool_field("elided", True), v_bool, True, False, False, "ui"),
    "enabled": Check(_bool_field("enabled", False), v_bool, True, False, False, "ui"),
    "focused": Check(_bool_field("focused", False), v_bool, True, False, False, "ui"),
    "screen": Check(c_screen, v_str, False, False, False, "screen"),
    "overlay": Check(c_overlay, v_str, False, False, False, "overlay"),
    "no_overlay": Check(c_no_overlay, v_true, False, False, False, "overlay"),
    "toast": Check(c_toast, v_toast, False, False, False, "toasts"),
    "no_toast": Check(c_no_toast, v_true, False, False, False, "toasts"),
    "app_state": Check(c_app_state, v_app_state, False, False, False, "app_state"),
    "theme": Check(c_theme, v_str, False, False, False, "theme"),
    "dark": Check(c_dark, v_bool, False, False, False, "theme"),
    "width_px": Check(c_width, v_range_pos, True, False, False, None),
    "height_px": Check(c_height, v_range_pos, True, False, False, None),
    "left_gap_px": Check(_gap(lambda t, r: t[0] - r[0]), v_range_min, True, True, False, None),
    "right_gap_px": Check(_gap(lambda t, r: r[2] - t[2]), v_range_min, True, True, False, None),
    "top_gap_px": Check(_gap(lambda t, r: t[1] - r[1]), v_range_min, True, True, False, None),
    "bottom_gap_px": Check(_gap(lambda t, r: r[3] - t[3]), v_range_min, True, True, False, None),
    "inside": Check(c_inside, v_true, True, True, False, None),
    "unclipped": Check(c_unclipped, v_true, True, False, False, None),
    "in_window": Check(c_in_window, v_true, True, False, False, None),
    "aligned": Check(c_aligned, v_aligned, True, False, False, None),
    "no_overlap": Check(c_no_overlap, v_no_overlap, True, False, False, None),
    "gap": Check(c_gap, v_gap, True, False, False, None),
    "color_token": Check(c_color_token, v_color, True, False, True, None),
}


def validate_spec(d):
    """The list of spec problems (empty = a usable spec)."""
    errs = []
    if not isinstance(d, dict):
        return ["the spec is not a table"]
    for k in d:
        if k not in ("spec", "row", "tier", "fixtures", "theme", "lang", "window", "point"):
            errs.append("unknown key `%s`" % k)
    if not (isint(d.get("spec")) and d["spec"] == 1):
        errs.append("`spec` must be 1")
    row = d.get("row")
    if not (isinstance(row, str) and ROW_RE.match(row) and ".." not in row):
        errs.append("`row` must be a row id string (letters, digits, `.`, `_`, `-`; no `..`)")
    if not (isint(d.get("tier")) and d["tier"] in (1, 2, 3)):
        errs.append("`tier` must be 1, 2 or 3")
    for k in ("fixtures", "theme", "lang"):
        if k in d and not isinstance(d[k], str):
            errs.append("`%s` must be a string" % k)
    win = d.get("window")
    if not isinstance(win, dict):
        errs.append("`window` is required: { size_pt = [w, h], pixels_per_point = n }")
    else:
        for k in win:
            if k not in ("size_pt", "pixels_per_point"):
                errs.append("window: unknown key `%s`" % k)
        sz = win.get("size_pt")
        if not (isinstance(sz, list) and len(sz) == 2 and all(isnum(x) and x > 0 for x in sz)):
            errs.append("window.size_pt must be two positive numbers")
        ppp = win.get("pixels_per_point")
        if not (isnum(ppp) and ppp > 0):
            errs.append("window.pixels_per_point must be a positive number")
    points = d.get("point")
    if not isinstance(points, list) or not points or not all(isinstance(p, dict) for p in points):
        errs.append("at least one [[point]] table is required")
        return errs
    seen = set()
    for i, p in enumerate(points):
        pid = p.get("id")
        w = "point %s" % (pid if isinstance(pid, str) else "#%d" % (i + 1))
        for k in p:
            if k not in ("id", "snap", "state", "target", "relative_to", "fail_looks_like", "expect"):
                errs.append("%s: unknown key `%s`" % (w, k))
        if not (isinstance(pid, str) and POINT_ID_RE.match(pid)):
            errs.append("%s: `id` must match [A-Za-z0-9._-]+" % w)
        elif pid in seen:
            errs.append("%s: duplicate id" % w)
        else:
            seen.add(pid)
        if ("snap" in p) == ("state" in p):
            errs.append("%s: exactly one of `snap` / `state` is required" % w)
        for k in ("snap", "state"):
            if k in p and not (isinstance(p[k], str) and NAME_RE.match(p[k])):
                errs.append("%s: `%s` must match [A-Za-z0-9_-]{1,64}" % (w, k))
        for k in ("target", "relative_to"):
            if k in p and not (isinstance(p[k], str) and p[k]):
                errs.append("%s: `%s` must be a non-empty test id" % (w, k))
        if not (isinstance(p.get("fail_looks_like"), str) and p["fail_looks_like"].strip()):
            errs.append("%s: `fail_looks_like` is required (what a failure looks like)" % w)
        ex = p.get("expect")
        if not isinstance(ex, dict) or not ex:
            errs.append("%s: `expect` must be a table with at least one check" % w)
            continue
        for name, val in ex.items():
            c = CHECKS.get(name)
            if c is None:
                errs.append("%s: unknown check `%s`" % (w, name))
                continue
            errs += ["%s: %s" % (w, m) for m in c.validate(val, name)]
            if c.target and "target" not in p:
                errs.append("%s: check `%s` needs `target`" % (w, name))
            if c.relative and "relative_to" not in p:
                errs.append("%s: check `%s` needs `relative_to`" % (w, name))
            if c.snap and "snap" not in p:
                errs.append("%s: check `%s` needs `snap` (a picture)" % (w, name))
    return errs


# ---------------------------------------------------------------------------------------------------------------------
# The run folder
# ---------------------------------------------------------------------------------------------------------------------


class SnapData:
    def __init__(self, name):
        self.name = name
        self.errors = []
        self.before = None
        self.after = None
        self.side = None
        self.png = None


class Ctx:
    def __init__(self, run_dir):
        self.dir = run_dir
        self.run_id = os.path.basename(os.path.abspath(run_dir))
        self._bytes = {}
        self.sha = {}
        self._states = {}
        self._snaps = {}
        self._trace = None
        self.run_json = None

    def read(self, rel):
        if rel in self._bytes:
            return self._bytes[rel]
        data = None
        path = os.path.join(self.dir, *rel.split("/"))
        try:
            with open(path, "rb") as f:
                data = f.read()
        except OSError:
            data = None
        self._bytes[rel] = data
        if data is not None:
            self.sha[rel] = sha256_hex(data)
        return data

    def exists(self, rel):
        return os.path.exists(os.path.join(self.dir, *rel.split("/")))

    def json_file(self, rel):
        """-> (value, error string or None)"""
        data = self.read(rel)
        if data is None:
            return None, "%s: missing" % rel
        try:
            return parse_json(data), None
        except (ValueError, UnicodeDecodeError) as e:
            return None, "%s: not valid JSON (%s)" % (rel, e)

    # --- run level -------------------------------------------------------------------------------------------------

    def run_level(self):
        errs = []
        if not RUN_ID_RE.match(self.run_id):
            errs.append("the run folder name `%s` is not a run id (1-64 of A-Z a-z 0-9 _ -)" % self.run_id)
        run, e = self.json_file("run.json")
        steps = None
        if e:
            errs.append(e)
        else:
            if not isinstance(run, dict):
                errs.append("run.json: expected an object")
            else:
                self.run_json = run
                errs += shape_errors(run, "run", "run.json")
                if run.get("schema") != 1 or not isint(run.get("schema")):
                    errs.append("run.json: schema is not 1")
                if run.get("run_id") != self.run_id:
                    errs.append("run.json: run_id %r is not the folder name %r" % (run.get("run_id"), self.run_id))
                if isint(run.get("steps")) and run["steps"] >= 1:
                    steps = run["steps"]
                else:
                    errs.append("run.json: steps is not an integer >= 1")
                if run.get("failed") is not False:
                    errs.append("run.json: failed is not false")
                if not (isint(run.get("left")) and run["left"] == 0):
                    errs.append("run.json: left is not 0 (%r)" % (run.get("left"),))
                if run.get("status") not in ("done", "quit", "closed"):
                    errs.append("run.json: status %r is not done, quit or closed" % (run.get("status"),))
        kd = self.read("keys.done")
        if kd is None:
            errs.append("keys.done: missing")
        else:
            lines = [ln.rstrip("\r") for ln in kd.decode("utf-8", "replace").split("\n")]
            runs = [ln[5:].strip() for ln in lines if ln.startswith("run: ")]
            if len(runs) != 1:
                errs.append("keys.done: expected exactly one `run: <id>` line, found %d" % len(runs))
            elif runs[0] != self.run_id:
                errs.append("keys.done: run %r is not the folder name %r" % (runs[0], self.run_id))
            nonempty = [ln.strip() for ln in lines if ln.strip()]
            if not nonempty or nonempty[-1] != "keys: done":
                errs.append("keys.done: the last line is not `keys: done`")
        if self.exists("blocked.txt"):
            errs.append("blocked.txt exists: a system dialog was in front")
        errs += self.trace_level(steps)
        return errs

    def trace_info(self):
        if self._trace is None:
            self._trace = {"lines": None, "errors": [], "by_step": {}}
            data = self.read("trace.jsonl")
            if data is None:
                self._trace["errors"].append("trace.jsonl: missing")
            else:
                pieces = data.split(b"\n")
                if pieces and pieces[-1] == b"":
                    pieces.pop()
                lines = []
                errs = self._trace["errors"]
                for i, p in enumerate(pieces, 1):
                    if not p:
                        errs.append("trace.jsonl: line %d is empty" % i)
                        continue
                    try:
                        obj = parse_json(p)
                    except (ValueError, UnicodeDecodeError) as e:
                        errs.append("trace.jsonl: line %d is not JSON (%s)" % (i, e))
                        continue
                    if not isinstance(obj, dict):
                        errs.append("trace.jsonl: line %d is not an object" % i)
                        continue
                    lines.append((i, obj))
                self._trace["lines"] = [o for _, o in lines]
                self._trace["numbered"] = lines
                for o in self._trace["lines"]:
                    if isint(o.get("step")) and o["step"] not in self._trace["by_step"]:
                        self._trace["by_step"][o["step"]] = o
        return self._trace

    def trace_level(self, steps):
        t = self.trace_info()
        errs = list(t["errors"])
        if t["lines"] is None:
            return errs
        seen = []
        for i, o in t["numbered"]:
            errs += shape_errors(o, "trace", "trace.jsonl line %d" % i)
            if o.get("schema_version") != 1:
                errs.append("trace.jsonl line %d: schema_version is not 1" % i)
            if o.get("run_id") != self.run_id:
                errs.append("trace.jsonl line %d: run_id %r is not the folder name" % (i, o.get("run_id")))
            if o.get("step") is None:
                errs.append("trace.jsonl line %d: step is null (%s: %s)" % (i, o.get("label"), o.get("why")))
            else:
                seen.append(o.get("step"))
            if o.get("outcome") != "done":
                errs.append("trace.jsonl line %d: outcome %r is not done (%s)" % (i, o.get("outcome"), o.get("label")))
        if steps is None:
            errs.append("trace.jsonl: cannot check the steps without a usable run.json steps")
        else:
            ok = sorted(s for s in seen if isint(s)) == list(range(1, steps + 1)) and len(seen) == steps
            if not ok:
                errs.append("trace.jsonl: the steps are not exactly 1..%d once each (got %s)" % (steps, seen))
        return errs

    def trace_tie(self, doc, label):
        t = self.trace_info()
        if t["lines"] is None:
            return ["trace.jsonl: unusable, cannot tie %s to its step" % label]
        hits = [o for o in t["lines"] if o.get("label") == label]
        if len(hits) != 1:
            return ["trace.jsonl: expected exactly one line labelled %s, found %d" % (label, len(hits))]
        a = doc["after_step"]
        want = 1 if a is None else a + 2
        errs = []
        if hits[0].get("step") != want:
            errs.append("trace.jsonl: %s is at step %r but the state says step %d (after_step %r)" % (label, hits[0].get("step"), want, a))
        if a is not None:
            prev = t["by_step"].get(a + 1)
            if prev is None or prev.get("label") != doc["after_label"]:
                errs.append(
                    "trace.jsonl: step %d is %r but the state's after_label is %r"
                    % (a + 1, None if prev is None else prev.get("label"), doc["after_label"])
                )
        return errs

    # --- state files -----------------------------------------------------------------------------------------------

    def state_doc(self, name, phase):
        """-> (doc or None, errors). `phase`: None for a `<State:>`, "before" / "after" for a snap file."""
        key = (name, phase)
        if key in self._states:
            return self._states[key]
        rel = "state/%s.json" % name
        doc, e = self.json_file(rel)
        if e:
            res = (None, [e])
        else:
            errs = shape_errors(doc, "state", rel) if isinstance(doc, dict) else ["%s: expected an object" % rel]
            if errs:
                res = (None, errs)
            else:
                if doc["schema_version"] != 1:
                    errs.append("%s: schema_version is not 1" % rel)
                if doc["name"] != name:
                    errs.append("%s: name %r is not the file stem" % (rel, doc["name"]))
                if doc["run_id"] != self.run_id:
                    errs.append("%s: run_id %r is not %r" % (rel, doc["run_id"], self.run_id))
                if doc["settled"] is not True:
                    errs.append("%s: settled is not true (%r)" % (rel, doc["settled"]))
                if doc["phase"] != phase:
                    errs.append("%s: phase is %r, expected %r" % (rel, doc["phase"], phase))
                if "ui" not in doc["capabilities"] or "window" not in doc["capabilities"]:
                    errs.append("%s: capabilities lack ui or window" % rel)
                if not doc["ui"]:
                    errs.append("%s: ui[] is empty (no test ids: AccessKit off?)" % rel)
                ids = [e["test_id"] for e in doc["ui"]]
                dup = sorted({i for i in ids if ids.count(i) > 1})
                if dup:
                    errs.append("%s: test_id appears twice in ui[]: %s" % (rel, ", ".join(dup)))
                res = (doc, errs)
        self._states[key] = res
        return res

    def snap(self, name):
        if name in self._snaps:
            return self._snaps[name]
        s = SnapData(name)
        self._snaps[name] = s
        files = ["state/%s-before.json" % name, "state/%s-after.json" % name, "shots/%s.json" % name, "shots/%s.png" % name]
        missing = [f for f in files if self.read(f) is None]
        for f in missing:
            s.errors.append("%s: missing" % f)
        if missing:
            return s
        s.before, e1 = self.state_doc(name + "-before", "before")
        s.after, e2 = self.state_doc(name + "-after", "after")
        s.errors += e1 + e2
        side, e = self.json_file(files[2])
        if e:
            s.errors.append(e)
        else:
            errs = shape_errors(side, "side", files[2]) if isinstance(side, dict) else ["%s: expected an object" % files[2]]
            s.errors += errs
            if not errs:
                s.side = side
        try:
            s.png = decode_png(self.read(files[3]))
        except PngError as e:
            s.errors.append("%s: %s" % (files[3], e))
        if s.before is None or s.after is None:
            return s
        b, a = s.before, s.after
        for which, d in (("before", b), ("after", a)):
            if d["capture_id"] is None:
                s.errors.append("state/%s-%s.json: capture_id is null" % (name, which))
        d = deep_diff(b, a, skip=("name", "phase", "frame"))
        if d:
            s.errors.append("the UI changed during the capture: %s differs between state/%s-before.json and -after.json" % (d, name))
        s.errors += self.trace_tie(b, "<Snap:%s>" % name)
        if s.side is not None:
            s.errors += self._side_errors(name, s, files)
        if s.side is not None and s.png is not None:
            iw, ih, _ = s.png
            if [iw, ih] != s.side["size_px"]:
                s.errors.append("%s: the PNG is %dx%d but size_px is %s" % (files[3], iw, ih, s.side["size_px"]))
            inner = b["window"]["inner_px"]
            if abs(s.side["size_px"][0] - inner[2]) > 1 or abs(s.side["size_px"][1] - inner[3]) > 1:
                s.errors.append("%s: size_px %s is not within 1 px of window.inner_px %sx%s" % (files[2], s.side["size_px"], inner[2], inner[3]))
        return s

    def _side_errors(self, name, s, files):
        side, b, a = s.side, s.before, s.after
        sj = files[2]
        errs = []

        def bad(msg):
            errs.append("%s: %s" % (sj, msg))

        if side["name"] != name:
            bad("name %r is not %r" % (side["name"], name))
        if side["run_id"] != self.run_id:
            bad("run_id %r is not the run's" % (side["run_id"],))
        if side["snapshot_seq"] is None or side["snapshot_seq"] != b["snapshot_seq"]:
            bad("snapshot_seq %r differs from the state's %r" % (side["snapshot_seq"], b["snapshot_seq"]))
        if side["capture_id"] != b["capture_id"]:
            bad("capture_id %r differs from the state's %r" % (side["capture_id"], b["capture_id"]))
        if side["png"] != name + ".png":
            bad("png is %r, expected %r" % (side["png"], name + ".png"))
        if side["png_sha256"] != self.sha.get(files[3]):
            bad("png_sha256 does not match shots/%s.png" % name)
        for which, d in (("before", b), ("after", a)):
            ref = side["state_" + which]
            rel = "state/%s-%s.json" % (name, which)
            if ref is None:
                bad("state_%s is null" % which)
                continue
            if ref["path"] != rel:
                bad("state_%s.path is %r, expected %r" % (which, ref["path"], rel))
            if ref["sha256"] != self.sha.get(rel):
                bad("state_%s.sha256 does not match %s (stale state)" % (which, rel))
            if deep_diff(ref["stable"], d["stable"]):
                bad("state_%s.stable differs from %s" % (which, rel))
        if side["state_unchanged"] is not True:
            bad("state_unchanged is not true")
        caps = set(b["capabilities"])
        if "theme" not in caps:
            bad("the state lacks the theme capability: the snap cannot be tied to it")
        else:
            if side["theme"] != b["theme"]["name"]:
                bad("theme %r differs from the state's %r" % (side["theme"], b["theme"]["name"]))
            if side["theme_requested"] != (b["theme"]["requested"] or ""):
                bad("theme_requested %r differs from the state's %r" % (side["theme_requested"], b["theme"]["requested"]))
            if side["env"]["theme"] != b["theme"]["name"]:
                bad("env.theme %r differs from the state's %r" % (side["env"]["theme"], b["theme"]["name"]))
        if "lang" not in caps:
            bad("the state lacks the lang capability: the snap cannot be tied to it")
        else:
            if side["lang"] != b["lang"]["in_force"]:
                bad("lang %r differs from the state's %r" % (side["lang"], b["lang"]["in_force"]))
            if side["lang_requested"] != (b["lang"]["requested"] or ""):
                bad("lang_requested %r differs from the state's %r" % (side["lang_requested"], b["lang"]["requested"]))
        ppp = b["window"]["pixels_per_point"]
        if abs(side["pixels_per_point"] - ppp) > 1e-3 or abs(side["env"]["pixels_per_point"] - ppp) > 1e-3:
            bad("pixels_per_point differs from the state's window.pixels_per_point %s" % ppp)
        if not (b["frame"] <= side["requested_frame"] <= side["received_frame"] <= a["frame"]):
            bad(
                "frames out of order: before %d, requested %d, received %d, after %d"
                % (b["frame"], side["requested_frame"], side["received_frame"], a["frame"])
            )
        return errs


# ---------------------------------------------------------------------------------------------------------------------
# Evaluate
# ---------------------------------------------------------------------------------------------------------------------


def outcome_of(errors, checks):
    if errors or any(c["outcome"] == "ERROR" for c in checks):
        return "ERROR"
    if any(c["outcome"] == "FAIL" for c in checks):
        return "FAIL"
    return "PASS"


def eval_point(ctx, spec, pt):
    errs = []
    snap = None
    doc = None
    if "snap" in pt:
        name = pt["snap"]
        snap = ctx.snap(name)
        errs += snap.errors
        doc = snap.before
        rels = ["state/%s-before.json" % name, "state/%s-after.json" % name, "shots/%s.json" % name, "shots/%s.png" % name]
    else:
        name = pt["state"]
        doc, e = ctx.state_doc(name, None)
        errs += e
        if doc is not None:
            errs += ctx.trace_tie(doc, "<State:%s>" % name)
        rels = ["state/%s.json" % name]
    inputs = [{"path": r, "sha256": ctx.sha[r]} for r in rels if r in ctx.sha]
    if doc is not None:
        win = spec["window"]
        ppp = win["pixels_per_point"]
        if abs(doc["window"]["pixels_per_point"] - ppp) > 1e-3:
            errs.append("window.pixels_per_point %s is not the spec's %s" % (doc["window"]["pixels_per_point"], ppp))
        inner = doc["window"]["inner_px"]
        if abs(inner[2] - win["size_pt"][0] * ppp) > 1 or abs(inner[3] - win["size_pt"][1] * ppp) > 1:
            errs.append("window.inner_px %sx%s is not size_pt x pixels_per_point (%sx%s)" % (inner[2], inner[3], win["size_pt"][0] * ppp, win["size_pt"][1] * ppp))
        if "theme" in spec and doc["theme"]["name"] != spec["theme"]:
            errs.append("theme %r is not the spec's %r" % (doc["theme"]["name"], spec["theme"]))
        if "lang" in spec and doc["lang"]["in_force"] != spec["lang"]:
            errs.append("lang %r is not the spec's %r" % (doc["lang"]["in_force"], spec["lang"]))
    checks = []
    pc = PointCtx(spec, pt, doc, snap) if doc is not None and not errs else None
    for cname, want in pt["expect"].items():
        c = CHECKS[cname]
        if pc is None:
            outcome, measured, why = "ERROR", None, "the evidence for this point is not valid (see errors)"
        else:
            try:
                if c.cap and c.cap not in pc.caps:
                    raise CheckError("needs capability %s" % c.cap)
                ok, measured, why = c.fn(pc, want)
                outcome = "PASS" if ok else "FAIL"
            except Fail as f:
                outcome, measured, why = "FAIL", f.measured, str(f)
            except CheckError as f:
                outcome, measured, why = "ERROR", None, str(f)
        checks.append({"check": cname, "outcome": outcome, "measured": measured, "expected": want, "why": why if outcome != "PASS" else None})
    return {
        "id": pt["id"], "snap": pt.get("snap"), "state": pt.get("state"), "target": pt.get("target"), "relative_to": pt.get("relative_to"),
        "outcome": outcome_of(errs, checks), "inputs": inputs, "checks": checks, "errors": errs,
    }


def evaluate(spec_path, run_dir):
    """-> (assertions dict, printed lines, exit code)."""
    ctx = Ctx(run_dir)
    spec_bytes = None
    try:
        with open(spec_path, "rb") as f:
            spec_bytes = f.read()
    except OSError:
        pass
    out = {
        "schema": "assertions.v1", "tool": dict(TOOL), "row": None, "tier": None,
        "spec": {"path": repo_path(spec_path), "sha256": sha256_hex(spec_bytes) if spec_bytes is not None else None},
        "run_id": ctx.run_id, "inputs": [], "overall": "ERROR", "points": [], "errors": [],
    }
    lines = []
    spec = None
    if spec_bytes is None:
        out["errors"].append("the spec %s cannot be read" % spec_path)
    else:
        try:
            raw = tomllib.loads(spec_bytes.decode("utf-8"))
        except (tomllib.TOMLDecodeError, UnicodeDecodeError) as e:
            raw = None
            out["errors"].append("the spec is not valid TOML: %s" % e)
        if raw is not None:
            if isinstance(raw, dict):
                if isinstance(raw.get("row"), str):
                    out["row"] = raw["row"]
                if isint(raw.get("tier")) and raw["tier"] in (1, 2, 3):
                    out["tier"] = raw["tier"]
            problems = validate_spec(raw)
            if problems:
                out["errors"] += ["spec: " + p for p in problems]
            else:
                spec = raw
    if spec is None:
        for e in out["errors"]:
            lines.append("ERROR %s" % e)
        lines.append("overall: ERROR")
        return out, lines, 2
    run_errors = ctx.run_level()
    out["errors"] += run_errors
    points = [eval_point(ctx, spec, pt) for pt in spec["point"]]
    out["points"] = points
    out["inputs"] = [{"path": r, "sha256": ctx.sha[r]} for r in ("run.json", "keys.done", "trace.jsonl") if r in ctx.sha]
    if out["errors"] or any(p["outcome"] == "ERROR" for p in points):
        out["overall"] = "ERROR"
    elif any(p["outcome"] == "FAIL" for p in points):
        out["overall"] = "FAIL"
    else:
        out["overall"] = "PASS"
    for p in points:
        if p["outcome"] == "PASS":
            lines.append("PASS %s" % p["id"])
            continue
        msgs = list(p["errors"])
        for c in p["checks"]:
            if c["outcome"] != "PASS":
                msgs.append("%s %s" % (c["check"], c["why"] if c["why"] else json.dumps(c["measured"], ensure_ascii=False)))
        lines.append("%s %s: %s" % (p["outcome"], p["id"], "; ".join(msgs)))
    for e in out["errors"]:
        lines.append("ERROR %s" % e)
    lines.append("overall: %s" % out["overall"])
    return out, lines, {"PASS": 0, "FAIL": 1, "ERROR": 2}[out["overall"]]


def write_assertions(run_dir, out):
    folder = os.path.join(run_dir, "machine")
    os.makedirs(folder, exist_ok=True)
    write_text_lf(os.path.join(folder, "assertions.json"), dumps(out))


def run_cli(spec_path, run_dir):
    if not os.path.isdir(run_dir):
        print("ERROR the run folder %s does not exist" % run_dir)
        print("overall: ERROR")
        return 2
    out, lines, code = evaluate(spec_path, run_dir)
    try:
        write_assertions(run_dir, out)
    except OSError as e:
        lines.append("ERROR cannot write machine/assertions.json: %s" % e)
        code = 2
    for ln in lines[:-1]:
        print(ln)
    print(lines[-1])
    return code


# ---------------------------------------------------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------------------------------------------------

SHARMA = """
50.0000 2.6772 -79.7751   50.0000 0.0000 -82.7485   2.0425
50.0000 3.1571 -77.2803   50.0000 0.0000 -82.7485   2.8615
50.0000 2.8361 -74.0200   50.0000 0.0000 -82.7485   3.4412
50.0000 -1.3802 -84.2814  50.0000 0.0000 -82.7485   1.0000
50.0000 -1.1848 -84.8006  50.0000 0.0000 -82.7485   1.0000
50.0000 -0.9009 -85.5211  50.0000 0.0000 -82.7485   1.0000
50.0000 0.0000 0.0000     50.0000 -1.0000 2.0000    2.3669
50.0000 -1.0000 2.0000    50.0000 0.0000 0.0000     2.3669
50.0000 2.4900 -0.0010    50.0000 -2.4900 0.0009    7.1792
50.0000 2.4900 -0.0010    50.0000 -2.4900 0.0010    7.1792
50.0000 2.4900 -0.0010    50.0000 -2.4900 0.0011    7.2195
50.0000 2.4900 -0.0010    50.0000 -2.4900 0.0012    7.2195
50.0000 -0.0010 2.4900    50.0000 0.0009 -2.4900    4.8045
50.0000 -0.0010 2.4900    50.0000 0.0010 -2.4900    4.8045
50.0000 -0.0010 2.4900    50.0000 0.0011 -2.4900    4.7461
50.0000 2.5000 0.0000     50.0000 0.0000 -2.5000    4.3065
50.0000 2.5000 0.0000     73.0000 25.0000 -18.0000  27.1492
50.0000 2.5000 0.0000     61.0000 -5.0000 29.0000   22.8977
50.0000 2.5000 0.0000     56.0000 -27.0000 -3.0000  31.9030
50.0000 2.5000 0.0000     58.0000 24.0000 15.0000   19.4535
50.0000 2.5000 0.0000     50.0000 3.1736 0.5854     1.0000
50.0000 2.5000 0.0000     50.0000 3.2972 0.0000     1.0000
50.0000 2.5000 0.0000     50.0000 1.8634 0.5757     1.0000
50.0000 2.5000 0.0000     50.0000 3.2592 0.3350     1.0000
60.2574 -34.0099 36.2677  60.4626 -34.1751 39.4387  1.2644
63.0109 -31.0961 -5.8663  62.8187 -29.7946 -4.0864  1.2630
61.2901 3.7196 -5.3901    61.4292 2.2480 -4.9620    1.8731
35.0831 -44.1164 3.7933   35.0232 -40.0716 1.5901   1.8645
22.7233 20.0904 -46.6940  23.0331 14.9730 -42.5619  2.0373
36.4612 47.8580 18.3852   36.2715 50.5065 21.2231   1.4146
90.8027 -2.0831 1.4410    91.1528 -1.6435 0.0447    1.4441
90.9257 -0.5406 -0.9208   88.6381 -0.8985 -0.7239   1.5381
6.7747 -0.2908 -2.4247    5.8714 -0.0985 -2.2286    0.6377
2.0776 0.0795 -1.1350     0.9033 -0.0636 -0.5514    0.9082
"""

THEME_TOKENS = {"bg": "#101418ff", "done": "#2ecc71ff", "ghost": "#ffffff80", "fg": "#e6e6e6ff"}
CAPS_ALL = list(CAPS)
GOOD_PIX = {"bg": (16, 20, 24), "done": (46, 204, 113)}
IMG_W, IMG_H = 300, 150


def ui_entry(tid, rect, **kw):
    e = {
        "test_id": tid, "role": "label", "text": None, "label": None, "value": None, "rect_px": list(rect), "clip_px": [0, 0, IMG_W, IMG_H],
        "visible": True, "elided": False, "enabled": True, "focused": False, "hidden": False, "layer": "base",
    }
    e.update(kw)
    return e


def base_ui():
    return [
        ui_entry("spot.panel", [20, 20, 280, 130], text="Panel", clip_px=[0, 0, 300, 150]),
        ui_entry("spot.scroll.thumb", [271, 40, 278.5, 90], clip_px=[20, 20, 280, 130], role="scrollbar"),
        ui_entry("spot.label", [30, 30, 120, 50], text="Hello world", label="hello", role="label"),
        ui_entry("spot.button", [30, 100, 100, 120], text="Go", role="button"),
    ]


def make_state(name, run_id, phase=None, after_step=0, after_label="<Wait:1>", frame=5, capture_id=None, seq=1):
    ui = base_ui()
    app_state = {"mode": "edit", "nested": {"a": 1, "b": [1, 2]}, "extra": 5}
    return {
        "schema_version": 1, "name": name, "run_id": run_id, "snapshot_seq": seq, "phase": phase, "capture_id": capture_id,
        "after_step": after_step, "after_label": after_label, "frame": frame, "settled": True,
        "app": {"name": "t", "version": "0.1.0"},
        "theme": {"requested": None, "name": "dark", "dark": True, "tokens": dict(THEME_TOKENS), "tokens_sha256": sha256_hex(b"tokens")},
        "lang": {"requested": None, "in_force": "ja"},
        "window": {"inner_px": [0, 0, IMG_W, IMG_H], "pixels_per_point": 1.5, "monitor_work_area_px": None},
        "screen": "main", "overlay": None, "toasts": [{"text": "Saved", "kind": "info"}], "app_state": app_state, "ui": ui,
        "capabilities": list(CAPS_ALL),
        "stable": {"projection": 1, "ui_sha256": sha256_hex(json.dumps(ui).encode()), "app_sha256": sha256_hex(json.dumps(app_state).encode())},
    }


def make_pix(paint=None):
    pix = bytearray(bytes(GOOD_PIX["bg"]) + b"\xff") * (IMG_W * IMG_H)
    return pix


def fill(pix, x0, y0, x1, y1, rgb, alpha=255):
    px = bytes(rgb) + bytes([alpha])
    for y in range(y0, y1):
        pix[(y * IMG_W + x0) * 4 : (y * IMG_W + x1) * 4] = px * (x1 - x0)


class Model:
    def __init__(self, run_id="run1"):
        self.run_id = run_id
        self.labels = ["<Wait:1>", "<State:main>", "<Snap:top>"]
        self.run = {
            "schema": 1, "runner": "t", "ito": {"version": "0", "rev": None, "rev_source": "unknown"}, "app": {"name": "t", "version": "0.1.0"},
            "run_id": run_id, "os": "windows", "arch": "x86_64", "steps": 3, "started_ms": 1, "ended_ms": 2, "status": "done",
            "failed": False, "step": None, "why": None, "left": 0,
        }
        self.keys_done = "run: %s\nkeys: done\n" % run_id
        self.trace = []
        for i, lab in enumerate(self.labels, 1):
            self.trace.append(
                {"schema_version": 1, "run_id": run_id, "step": i, "label": lab, "outcome": "done", "settled": True, "frame": i * 3,
                 "time_ms": i * 10, "end_frame": i * 3 + 1, "end_ms": i * 10 + 5, "why": None}
            )
        self.states = {
            "main": make_state("main", run_id, None, 0, self.labels[0], 6),
            "top-before": make_state("top-before", run_id, "before", 1, self.labels[1], 8, "cap-top", 2),
            "top-after": make_state("top-after", run_id, "after", 1, self.labels[1], 12, "cap-top", 2),
        }
        pix = make_pix()
        fill(pix, 271, 40, 279, 90, GOOD_PIX["done"])
        self.pix = {"top": pix}
        self.pngs = {"top": (IMG_W, IMG_H, 4)}
        self.png_bytes = {}
        self.side = {
            "top": {
                "name": "top", "app": "t", "version": "0.1.0", "theme": "dark", "theme_requested": "", "lang": "ja", "lang_requested": "",
                "size": [200, 100], "zoom": 1, "state": "", "component": "", "rect": [0, 0, 0, 0], "time": 0, "pixels_per_point": 1.5,
                "run_id": run_id, "snapshot_seq": 2, "capture_id": "cap-top", "png": "top.png", "png_sha256": None,
                "size_px": [IMG_W, IMG_H], "state_before": None, "state_after": None, "state_unchanged": True,
                "requested_frame": 9, "received_frame": 10,
                "env": {"os": "windows", "arch": "x86_64", "wgpu_backend": None, "font_set_sha256": None, "theme": "dark", "pixels_per_point": 1.5},
            }
        }
        self.blocked = False
        self.drop = set()

    def each_state(self, fn):
        for d in self.states.values():
            fn(d)


def ui_of(doc, tid):
    return next(e for e in doc["ui"] if e["test_id"] == tid)


class RunDir:
    def __init__(self, path):
        self.path = path

    def p(self, rel):
        return os.path.join(self.path, *rel.split("/"))

    def text(self, rel):
        with open(self.p(rel), "r", encoding="utf-8", newline="") as f:
            return f.read()

    def put_text(self, rel, text):
        os.makedirs(os.path.dirname(self.p(rel)), exist_ok=True)
        with open(self.p(rel), "wb") as f:
            f.write(text.encode("utf-8"))

    def put_bytes(self, rel, data):
        os.makedirs(os.path.dirname(self.p(rel)), exist_ok=True)
        with open(self.p(rel), "wb") as f:
            f.write(data)

    def json(self, rel):
        return json.loads(self.text(rel))

    def put_json(self, rel, obj):
        self.put_text(rel, dumps(obj))

    def remove(self, rel):
        os.remove(self.p(rel))


def write_run(m, root):
    d = RunDir(root)
    os.makedirs(root, exist_ok=True)
    for name, doc in m.states.items():
        d.put_text("state/%s.json" % name, dumps(doc))
    for s, (w, h, ch) in m.pngs.items():
        data = m.png_bytes.get(s) or encode_png(w, h, m.pix[s], ch)
        d.put_bytes("shots/%s.png" % s, data)
    for s, side in m.side.items():
        side = copy.deepcopy(side)
        side["png_sha256"] = sha256_hex(d.text("shots/%s.png" % s).encode("latin-1")) if False else sha256_hex(open(d.p("shots/%s.png" % s), "rb").read())
        for which in ("before", "after"):
            rel = "state/%s-%s.json" % (s, which)
            doc = m.states[s + "-" + which]
            side["state_" + which] = {
                "path": rel, "sha256": sha256_hex(open(d.p(rel), "rb").read()),
                "stable": {"projection": doc["stable"]["projection"], "ui_sha256": doc["stable"]["ui_sha256"], "app_sha256": doc["stable"]["app_sha256"]},
            }
        d.put_text("shots/%s.json" % s, dumps(side))
    d.put_text("trace.jsonl", "".join(json.dumps(t, ensure_ascii=False) + "\n" for t in m.trace))
    d.put_text("run.json", dumps(m.run))
    d.put_text("keys.done", m.keys_done)
    if m.blocked:
        d.put_text("blocked.txt", "a dialog\n")
    for rel in m.drop:
        d.remove(rel)
    return d


HEAD = """spec = 1
row = "41.15"
tier = 1
fixtures = "spot-long"
theme = "dark"
lang = "ja"
window = { size_pt = [200, 100], pixels_per_point = 1.5 }
"""


def snap_point(expect, pid="p1", target="spot.scroll.thumb", snap="top", rel="spot.panel"):
    return (
        '\n[[point]]\nid = "%s"\nsnap = "%s"\ntarget = "%s"\nrelative_to = "%s"\nfail_looks_like = "the thumb is off"\nexpect = { %s }\n'
        % (pid, snap, target, rel, expect)
    )


def state_point(expect, pid="s1", target="spot.label", state="main"):
    return '\n[[point]]\nid = "%s"\nstate = "%s"\ntarget = "%s"\nfail_looks_like = "the label is wrong"\nexpect = { %s }\n' % (pid, state, target, expect)


THUMB_OK = "right_gap_px = { min = 0, max = 2 }, width_px = { min = 4, max = 9 }, inside = true"
SPEC_THUMB = HEAD + snap_point(THUMB_OK)
SPEC_BIG = (
    HEAD
    + snap_point(
        "present = true, visible = true, right_gap_px = { min = 0, max = 2 }, left_gap_px = { min = 250, max = 252 }, "
        "top_gap_px = { min = 19, max = 21 }, bottom_gap_px = { min = 38, max = 42 }, width_px = { min = 4, max = 9 }, "
        "height_px = { min = 49, max = 51 }, inside = true, unclipped = true, in_window = true, "
        'aligned = { with = "spot.panel", edge = "center_y", tol_px = 10 }, no_overlap = ["spot.label", "spot.button"], '
        'gap = { to = "spot.label", side = "left", min = 150, max = 152 }, '
        'color_token = { token = "done", delta_e_max = 3, min_fraction = 1 }'
    )
    + state_point(
        'present = true, visible = true, text = "Hello world", label = { contains = "hell" }, role = "label", elided = false, '
        'enabled = true, focused = false, screen = "main", no_overlay = true, toast = { text = { contains = "Sav" }, kind = "info" }, '
        'app_state = { mode = "edit", nested = { a = 1 } }, theme = "dark", dark = true'
    )
)
SPEC_STATE = HEAD + state_point("present = true, text = \"Hello world\"")


def snap_color(color):
    return HEAD + snap_point("color_token = { %s }" % color)


class Case:
    def __init__(self, name, overall, spec=SPEC_THUMB, pre=None, post=None, contains=(), check=None, emit=True):
        self.name, self.overall, self.spec, self.pre, self.post = name, overall, spec, pre, post
        self.contains, self.check, self.emit = contains, check, emit


def move_thumb(dx):
    def pre(m):
        def f(doc):
            r = ui_of(doc, "spot.scroll.thumb")["rect_px"]
            r[0] += dx
            r[2] += dx

        m.each_state(f)

    return pre


def set_all(fn):
    def pre(m):
        m.each_state(fn)

    return pre


def check_of(a, pid, cname):
    p = next(p for p in a["points"] if p["id"] == pid)
    return next(c for c in p["checks"] if c["check"] == cname)


def no_trace(m):
    m.drop.add("trace.jsonl")


def retrace(fn):
    def pre(m):
        fn(m.trace)

    return pre


def paint_border(m):
    fill(m.pix["top"], 271, 40, 279, 41, (255, 0, 0))
    fill(m.pix["top"], 271, 89, 279, 90, (255, 0, 0))
    fill(m.pix["top"], 271, 40, 272, 90, (255, 0, 0))
    fill(m.pix["top"], 278, 40, 279, 90, (255, 0, 0))


def composited_ghost():
    r, g, b = GOOD_PIX["bg"]
    a = 128 / 255.0
    return tuple(int(s * a + d * (1 - a) + 0.5) for s, d in ((255, r), (255, g), (255, b)))


def build_cases():
    cases = []
    add = cases.append
    add(Case("good-all-checks", "PASS", SPEC_BIG, check=lambda a: None if all(c["outcome"] == "PASS" for p in a["points"] for c in p["checks"]) else "a check did not pass"))
    add(Case("moved-3px", "FAIL", pre=move_thumb(3), check=lambda a: None if check_of(a, "p1", "right_gap_px")["measured"] == -1.5 and check_of(a, "p1", "inside")["outcome"] == "FAIL" else "measured values are wrong"))
    # 5: present = false
    ghost = HEAD + state_point("present = false", target="spot.ghost")
    add(Case("present-false-absent", "PASS", ghost))
    add(Case("present-false-there", "FAIL", HEAD + state_point("present = false", target="spot.label")))
    add(Case("present-false-error-entry", "ERROR", HEAD + state_point("present = false", target="spot.broken"),
             pre=set_all(lambda d: d["ui"].append({"test_id": "spot.broken", "error": "duplicate test id"}))))
    add(Case("present-false-empty-ui", "ERROR", ghost, pre=set_all(lambda d: d.update(ui=[])), contains=["ui[] is empty"]))
    # 6, 7: stale and unequal states
    def stale(d):
        j = d.json("shots/top.json")
        j["state_before"]["sha256"] = "0" * 64
        d.put_json("shots/top.json", j)

    add(Case("stale-state-sha", "ERROR", post=stale, contains=["stale state"]))
    add(Case("before-after-ui-sha", "ERROR", pre=lambda m: m.states["top-before"]["stable"].update(ui_sha256="a" * 64), contains=["the UI changed during the capture"]))
    add(Case("before-after-seq", "ERROR", pre=lambda m: m.states["top-after"].update(snapshot_seq=9), contains=["snapshot_seq"]))
    add(Case("before-after-capture-id", "ERROR", pre=lambda m: m.states["top-after"].update(capture_id="other"), contains=["capture_id"]))
    # 8: run level
    add(Case("unsettled", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"].update(settled=False), contains=["settled is not true"]))
    add(Case("run-failed", "ERROR", pre=lambda m: m.run.update(status="failed"), contains=["status"]))
    add(Case("run-quit-ok", "PASS", pre=lambda m: m.run.update(status="quit")))
    add(Case("run-closed-ok", "PASS", pre=lambda m: m.run.update(status="closed")))
    add(Case("run-quit-left-1", "ERROR", pre=lambda m: m.run.update(status="quit", left=1), contains=["left"]))
    add(Case("keys-done-other-run", "ERROR", pre=lambda m: setattr(m, "keys_done", "run: other\nkeys: done\n"), contains=["keys.done"]))
    add(Case("keys-done-not-finished", "ERROR", pre=lambda m: setattr(m, "keys_done", "run: run1\n"), contains=["keys: done"]))
    add(Case("blocked", "ERROR", pre=lambda m: setattr(m, "blocked", True), contains=["blocked.txt"]))
    # 9: sizes
    def small_png(m):
        m.pix["top"] = make_pix()[: 301 * IMG_H * 4] if False else bytearray(bytes(GOOD_PIX["bg"]) + b"\xff") * (301 * IMG_H)
        m.pngs["top"] = (301, IMG_H, 4)

    add(Case("png-size-differs", "ERROR", pre=small_png, contains=["size_px"]))

    def off2(m):
        m.pix["top"] = bytearray(bytes(GOOD_PIX["bg"]) + b"\xff") * (302 * IMG_H)
        m.pngs["top"] = (302, IMG_H, 4)
        m.side["top"]["size_px"] = [302, IMG_H]

    add(Case("size-px-2-off", "ERROR", pre=off2, contains=["within 1 px"]))
    # 16: trace
    add(Case("trace-missing", "ERROR", pre=no_trace, contains=["trace.jsonl: missing"]))
    add(Case("trace-other-run", "ERROR", pre=retrace(lambda t: t[0].update(run_id="zzz")), contains=["run_id"]))
    add(Case("trace-cut-line", "ERROR", pre=retrace(lambda t: t[2].update(outcome="cut", why="cut")), contains=["outcome"]))
    add(Case("trace-step-missing", "ERROR", pre=retrace(lambda t: t.pop(1)), contains=["exactly 1..3"]))
    add(Case("trace-step-twice", "ERROR", pre=retrace(lambda t: t[1].update(step=3)), contains=["exactly 1..3"]))
    add(Case("trace-state-wrong-step", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"].update(after_step=1, after_label="<State:main>"), contains=["trace.jsonl: <State:main> is at step"]))
    add(Case("trace-after-label", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"].update(after_label="<Nope>"), contains=["after_label"]))
    add(Case("trace-no-step-line", "ERROR", pre=retrace(lambda t: t.append(dict(t[0], step=None, label="<run.json>", outcome="failed", why="x"))), contains=["step is null"]))
    # 17: before/after, side vs state
    def rect_after(m):
        ui_of(m.states["top-after"], "spot.panel")["rect_px"][0] = 21

    add(Case("equal-stable-different-rect", "ERROR", pre=rect_after, contains=["ui[0].rect_px[0]"]))
    add(Case("different-theme-name", "ERROR", pre=lambda m: m.states["top-after"]["theme"].update(name="light"), contains=["theme.name"]))
    add(Case("side-lang-differs", "ERROR", pre=lambda m: m.side["top"].update(lang="en"), contains=["lang"]))
    add(Case("requested-frame-early", "ERROR", pre=lambda m: m.side["top"].update(requested_frame=3), contains=["frames out of order"]))
    add(Case("state-unchanged-false", "ERROR", pre=lambda m: m.side["top"].update(state_unchanged=False), contains=["state_unchanged"]))
    add(Case("window-ppp-differs", "ERROR", pre=set_all(lambda d: d["window"].update(pixels_per_point=2.0)), contains=["pixels_per_point"]))
    add(Case("evidence-theme-not-spec", "ERROR", pre=set_all(lambda d: d["theme"].update(name="light")), contains=["not the spec's"]))
    # 18: shape (written files are schema-invalid on purpose: not emitted)
    def nan_frame(d):
        d.put_text("state/main.json", re.sub(r'"frame": \d+', '"frame": NaN', d.text("state/main.json"), count=1))

    add(Case("shape-l-gt-r", "ERROR", SPEC_STATE, pre=lambda m: ui_of(m.states["main"], "spot.label").update(rect_px=[10, 0, 5, 5]), contains=["rect_px"], emit=False))
    add(Case("shape-nan", "ERROR", SPEC_STATE, post=nan_frame, contains=["not valid JSON"], emit=False))
    add(Case("shape-bool-number", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"].update(frame=True), contains=["frame"], emit=False))
    add(Case("shape-short-hash", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"]["stable"].update(ui_sha256="a" * 63), contains=["ui_sha256"], emit=False))
    add(Case("shape-projection-2", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"]["stable"].update(projection=2), contains=["projection"], emit=False))
    add(Case("shape-unknown-capability", "ERROR", SPEC_STATE, pre=lambda m: m.states["main"]["capabilities"].append("bogus"), contains=["capabilities"], emit=False))
    add(Case("shape-ui-extra-key", "ERROR", SPEC_STATE, pre=lambda m: ui_of(m.states["main"], "spot.label").update(extra=1), contains=["unknown key"], emit=False))
    # 20: degenerate and protrusion
    zero = lambda m: m.each_state(lambda d: ui_of(d, "spot.scroll.thumb").update(rect_px=[271, 40, 271, 90]))
    add(Case("zero-width-target", "FAIL", snap_color('token = "done", delta_e_max = 3, min_fraction = 1') .replace("expect = { color_token", "expect = { inside = true, color_token"),
             pre=zero, contains=["has no area"], check=lambda a: None if check_of(a, "p1", "inside")["outcome"] == "FAIL" and check_of(a, "p1", "color_token")["outcome"] == "FAIL" else "expected FAIL on both"))
    add(Case("zero-width-width-check", "FAIL", pre=zero, check=lambda a: None if check_of(a, "p1", "width_px")["outcome"] == "FAIL" and check_of(a, "p1", "width_px")["measured"] == 0 else "width_px should measure 0"))

    def protrude(m):
        m.each_state(lambda d: ui_of(d, "spot.scroll.thumb")["rect_px"].__setitem__(2, 285))

    add(Case("protrusion-5px", "FAIL", pre=protrude, check=lambda a: None if check_of(a, "p1", "right_gap_px")["measured"] == -5 else "measured should be -5"))
    add(Case("gap-without-min", "ERROR", HEAD + snap_point("right_gap_px = { max = 2 }"), contains=["`min` is required"]))
    # 10: colour
    solid = 'token = "done", delta_e_max = 3, min_fraction = 1'
    add(Case("color-solid", "PASS", snap_color(solid)))
    add(Case("color-border-inset0", "FAIL", snap_color(solid + ", inset_px = 0"), pre=paint_border))
    add(Case("color-border-inset1", "PASS", snap_color(solid + ", inset_px = 1"), pre=paint_border))
    add(Case("color-translucent-no-over", "ERROR", snap_color('token = "ghost", delta_e_max = 3, min_fraction = 1'), contains=["`over` is required"]))

    def paint_ghost(m):
        fill(m.pix["top"], 271, 40, 279, 90, composited_ghost())

    add(Case("color-translucent-over", "PASS", snap_color('token = "ghost", delta_e_max = 1, min_fraction = 1, over = "bg"'), pre=paint_ghost))
    add(Case("color-translucent-over-hex", "PASS", snap_color('token = "ghost", delta_e_max = 1, min_fraction = 1, over = "#101418"'), pre=paint_ghost))
    add(Case("color-empty-region", "ERROR", snap_color(solid + ", inset_px = 10"), contains=["region to sample is empty"]))
    add(Case("color-unknown-token", "ERROR", snap_color('token = "nope", delta_e_max = 3, min_fraction = 1'), contains=["unknown token"]))
    add(Case("color-mask-over-half", "ERROR", snap_color(solid + ', mask = [[271, 40, 279, 65]], max_mask_fraction = 0.6'), contains=["max_mask_fraction"]))
    add(Case("color-mask-over-cap", "ERROR", snap_color(solid + ', mask = [[271, 40, 279, 65]], max_mask_fraction = 0.2'), contains=["more than max_mask_fraction"]))

    def paint_half(m):
        fill(m.pix["top"], 271, 65, 279, 90, (255, 0, 0))

    add(Case("color-mask-ok", "PASS", snap_color(solid + ', mask = [[271, 65, 279, 90]], max_mask_fraction = 0.5'), pre=paint_half,
             check=lambda a: None if check_of(a, "p1", "color_token")["measured"]["masked"] > 0 else "nothing masked"))
    add(Case("color-mask-by-id", "PASS", snap_color(solid + ', mask = ["spot.label"], max_mask_fraction = 0.1')))

    def semi(m):
        fill(m.pix["top"], 275, 60, 276, 61, GOOD_PIX["done"], alpha=128)

    add(Case("color-semi-transparent-pixel", "ERROR", snap_color(solid), pre=semi, contains=["not opaque"]))

    def rgb_png(m):
        m.pngs["top"] = (IMG_W, IMG_H, 3)
        m.pix["top"] = bytearray(m.pix["top"])
        # an RGB picture is written from the RGB bytes of the same scene
        rgb = bytearray()
        for i in range(0, len(m.pix["top"]), 4):
            rgb += m.pix["top"][i : i + 3]
        m.pix["top"] = rgb

    add(Case("color-rgb-png", "PASS", snap_color(solid), pre=rgb_png))
    # 11, 14, 15: capability, ERROR over FAIL, app_state
    nocap = lambda d: (d["capabilities"].remove("screen"), d.update(screen=None))
    add(Case("missing-capability", "ERROR", HEAD + state_point('screen = "main"'), pre=set_all(nocap), contains=["needs capability screen"]))
    add(Case("error-wins-over-fail", "ERROR", HEAD + state_point('screen = "main", text = "nope"', pid="s1") + state_point('text = "nope"', pid="s2"), pre=set_all(nocap),
             check=lambda a: None if [p["outcome"] for p in a["points"]] == ["ERROR", "FAIL"] else "points should be ERROR, FAIL"))
    add(Case("app-state-subset-ok", "PASS", HEAD + state_point('app_state = { nested = { b = [1, 2] } }')))
    add(Case("app-state-nested-differs", "FAIL", HEAD + state_point("app_state = { nested = { a = 2 } }")))
    add(Case("no-toast-fails", "FAIL", HEAD + state_point("no_toast = true")))
    add(Case("text-contains-null", "FAIL", HEAD + state_point('value = "x"'), check=lambda a: None if check_of(a, "s1", "value")["measured"] is None else "measured should be null"))
    add(Case("visible-null-error", "ERROR", HEAD + state_point("visible = true"), pre=set_all(lambda d: ui_of(d, "spot.label").update(visible=None)), contains=["visible"]))
    add(Case("unclipped-null-error", "ERROR", HEAD + state_point("unclipped = true"), pre=set_all(lambda d: ui_of(d, "spot.label").update(clip_px=None))))
    add(Case("relative-to-absent", "ERROR", HEAD + snap_point("inside = true", rel="spot.nope"), contains=["spot.nope"]))
    # 12, 13: spec errors
    def bad(extra_top="", point_extra="", expect=THUMB_OK, head=HEAD):
        return head + extra_top + (
            '\n[[point]]\nid = "p1"\nsnap = "top"\ntarget = "spot.scroll.thumb"\nrelative_to = "spot.panel"\n%sexpect = { %s }\n' % (point_extra, expect)
        )

    fl = 'fail_looks_like = "x"\n'
    add(Case("spec-unknown-key", "ERROR", bad("bogus = 1\n", fl), contains=["unknown key `bogus`"]))
    add(Case("spec-unknown-check", "ERROR", bad(point_extra=fl, expect="sparkle = true"), contains=["unknown check `sparkle`"]))
    add(Case("spec-empty-expect", "ERROR", bad(point_extra=fl, expect=""), contains=["at least one check"]))
    add(Case("spec-no-fail-looks-like", "ERROR", bad(expect=THUMB_OK), contains=["fail_looks_like"]))
    add(Case("spec-duplicate-id", "ERROR", bad(point_extra=fl) + bad(point_extra=fl).split(HEAD)[1], contains=["duplicate id"]))
    add(Case("spec-version-2", "ERROR", SPEC_THUMB.replace("spec = 1", "spec = 2"), contains=["`spec` must be 1"]))
    add(Case("spec-baseline", "ERROR", bad(point_extra=fl, expect='baseline = { path = "x.png" }'), contains=["unknown check `baseline`"]))
    add(Case("spec-no-points", "ERROR", HEAD, contains=["[[point]]"]))
    add(Case("spec-both-snap-state", "ERROR", bad(point_extra=fl + 'state = "main"\n'), contains=["exactly one of"]))
    add(Case("spec-target-missing", "ERROR", HEAD + '\n[[point]]\nid = "p1"\nsnap = "top"\nfail_looks_like = "x"\nexpect = { visible = true }\n', contains=["needs `target`"]))
    add(Case("spec-color-on-state", "ERROR", HEAD + '\n[[point]]\nid = "p1"\nstate = "main"\ntarget = "spot.label"\nfail_looks_like = "x"\nexpect = { color_token = { token = "done", delta_e_max = 3, min_fraction = 1 } }\n', contains=["needs `snap`"]))
    add(Case("spec-empty-range", "ERROR", HEAD + state_point("width_px = {}"), contains=["empty range"]))
    add(Case("spec-width-min-zero", "ERROR", HEAD + state_point("width_px = { min = 0 }"), contains=["> 0"]))
    add(Case("spec-min-gt-max", "ERROR", HEAD + snap_point("right_gap_px = { min = 3, max = 1 }"), contains=["min > max"]))
    add(Case("spec-bad-window", "ERROR", SPEC_THUMB.replace("[200, 100]", "[200, 0]"), contains=["size_pt"]))
    add(Case("spec-bad-snap-name", "ERROR", HEAD + snap_point(THUMB_OK, snap="a/b"), contains=["snap"]))
    return cases


def selftest_png(fails):
    def want(cond, msg):
        if not cond:
            fails.append("png: " + msg)

    w, h = 7, 6
    pix = bytearray((x * 37 + y * 11 + c * 5 + (x * y) % 13) & 255 for y in range(h) for x in range(w) for c in range(4))
    bpp, stride = 4, w * 4

    def paeth(a, b, c):
        p = a + b - c
        pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
        return a if pa <= pb and pa <= pc else (b if pb <= pc else c)

    def filt(ft, cur, prev):
        out = bytearray(len(cur))
        for i in range(len(cur)):
            a = cur[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            p = (0, a, b, (a + b) // 2, paeth(a, b, c))[ft]
            out[i] = (cur[i] - p) & 255
        return bytes(out)

    for ft in range(5):
        raw = bytearray()
        prev = bytes(stride)
        for y in range(h):
            cur = bytes(pix[y * stride : (y + 1) * stride])
            raw += bytes([ft]) + filt(ft, cur, prev)
            prev = cur
        try:
            dw, dh, got = decode_png(encode_png(w, h, pix, raw=bytes(raw)))
            want((dw, dh) == (w, h) and bytes(got) == bytes(pix), "filter %d does not round-trip" % ft)
        except PngError as e:
            fails.append("png: filter %d: %s" % (ft, e))
    # mixed filters per row, and RGB
    raw = bytearray()
    prev = bytes(stride)
    for y in range(h):
        cur = bytes(pix[y * stride : (y + 1) * stride])
        raw += bytes([y % 5]) + filt(y % 5, cur, prev)
        prev = cur
    try:
        want(bytes(decode_png(encode_png(w, h, pix, raw=bytes(raw)))[2]) == bytes(pix), "mixed filters do not round-trip")
        rgb = bytearray(b for i, b in enumerate(pix) if i % 4 != 3)
        dw, dh, got = decode_png(encode_png(w, h, rgb, channels=3))
        want(bytes(got[0::4]) == bytes(rgb[0::3]) and set(got[3::4]) == {255}, "RGB does not decode with alpha 255")
    except PngError as e:
        fails.append("png: %s" % e)
    good = encode_png(w, h, pix)

    def must_fail(name, data, text=None):
        try:
            decode_png(data)
            fails.append("png: %s was accepted" % name)
        except PngError as e:
            if text and text not in str(e):
                fails.append("png: %s gave %r, expected %r" % (name, str(e), text))

    bad_crc = bytearray(good)
    bad_crc[20] ^= 1
    must_fail("bad CRC", bytes(bad_crc), "CRC")
    must_fail("16 bit", encode_png(w, h, pix, depth=16), "unsupported PNG")
    must_fail("interlaced", encode_png(w, h, pix, interlace=1), "interlaced")
    must_fail("bad signature", b"x" + good[1:], "signature")
    raw = b"".join(b"\x00" + bytes(pix[y * stride : (y + 1) * stride]) for y in range(h))
    must_fail("truncated IDAT", encode_png(w, h, pix, idat_data=zlib.compress(raw)[:-9]))
    must_fail("extra decompressed data", encode_png(w, h, pix, idat_data=zlib.compress(raw + b"\x00" * 5)), "more data")
    must_fail("short decompressed data", encode_png(w, h, pix, idat_data=zlib.compress(raw[:-5])), "less data")
    must_fail("truncated file", good[:-20])
    must_fail("data after IEND", good + b"\x00")
    # big image, filter 0 and Paeth speed sanity is checked by the colour canaries


def selftest_color(fails):
    for i, ln in enumerate(l for l in SHARMA.strip().splitlines() if l.strip()):
        v = [float(x) for x in ln.split()]
        got = ciede2000(tuple(v[0:3]), tuple(v[3:6]))
        if abs(got - v[6]) > 1e-4:
            fails.append("ciede2000: pair %d expected %.4f got %.4f" % (i + 1, v[6], got))
    w = srgb_to_lab(255, 255, 255)
    if abs(w[0] - 100) > 0.01 or abs(w[1]) > 0.01 or abs(w[2]) > 0.01:
        fails.append("lab: white is %s" % (w,))
    k = srgb_to_lab(0, 0, 0)
    if abs(k[0]) > 1e-9:
        fails.append("lab: black L is %s" % k[0])


def run_case(case, base, index):
    root = os.path.join(base, "%02d-%s" % (index, case.name))
    run_dir = os.path.join(root, "run1")
    m = Model("run1")
    if case.pre:
        case.pre(m)
    d = write_run(m, run_dir)
    if case.post:
        case.post(d)
    spec_path = os.path.join(root, "spec.toml")
    with open(spec_path, "wb") as f:
        f.write(case.spec.encode("utf-8"))
    code = run_cli_quiet(spec_path, run_dir)
    return run_dir, code


def run_cli_quiet(spec_path, run_dir):
    out, lines, code = evaluate(spec_path, run_dir)
    write_assertions(run_dir, out)
    return code


def selftest(emit):
    fails = []
    selftest_png(fails)
    selftest_color(fails)
    base = tempfile.mkdtemp(prefix="visual-check-selftest-")
    try:
        cases = build_cases()
        names = [c.name for c in cases]
        if len(set(names)) != len(names):
            fails.append("self-test: duplicate case names")
        emitted = 0
        for i, case in enumerate(cases, 1):
            try:
                run_dir, code = run_case(case, base, i)
                with open(os.path.join(run_dir, "machine", "assertions.json"), "rb") as f:
                    a = json.loads(f.read().decode("utf-8"))
            except Exception as e:  # a bug in the checker: report it as a failure of this canary
                fails.append("%s: the checker crashed: %r" % (case.name, e))
                continue
            if a["overall"] != case.overall:
                fails.append("%s: overall %s, expected %s (%s)" % (case.name, a["overall"], case.overall, json.dumps(a["errors"] + [p["errors"] for p in a["points"]], ensure_ascii=False)[:600]))
            if code != {"PASS": 0, "FAIL": 1, "ERROR": 2}[case.overall]:
                fails.append("%s: exit code %d" % (case.name, code))
            text = json.dumps(a, ensure_ascii=False)
            for needle in case.contains:
                if needle not in text:
                    fails.append("%s: expected %r in the assertions" % (case.name, needle))
            if case.overall == "ERROR" and a["points"] == [] and not a["errors"]:
                fails.append("%s: a spec ERROR without a reason" % case.name)
            if case.name.startswith("spec-") and a["points"] != []:
                fails.append("%s: a spec ERROR must have points: []" % case.name)
            if case.check:
                try:
                    msg = case.check(a)
                except Exception as e:
                    msg = "check crashed: %r" % (e,)
                if msg:
                    fails.append("%s: %s" % (case.name, msg))
            if emit and case.emit:
                dest = os.path.join(emit, "visual-check", "%02d-%s" % (i, case.name), "run1")
                shutil.copytree(run_dir, dest, dirs_exist_ok=True)
                emitted += 1
        # CLI behaviour
        bogus = os.path.join(base, "does-not-exist")
        if run_cli(os.path.join(base, "spec.toml"), bogus) != 2:
            fails.append("cli: a missing run folder must exit 2")
        some = os.path.join(base, "01-good-all-checks")
        if run_cli(os.path.join(some, "nospec.toml"), os.path.join(some, "run1")) != 2:
            fails.append("cli: a missing spec must exit 2")
        else:
            with open(os.path.join(some, "run1", "machine", "assertions.json"), "rb") as f:
                miss = json.loads(f.read().decode("utf-8"))
            if miss["overall"] != "ERROR" or miss["spec"]["sha256"] is not None or not miss["errors"]:
                fails.append("cli: a missing spec must write an ERROR assertions.json with a null spec sha256")
        a = json.loads(open(os.path.join(base, "03-moved-3px", "run1", "machine", "assertions.json"), "rb").read().decode("utf-8")) if False else None
        # key order of the written file
        with open(os.path.join(some, "spec.toml"), "rb") as f:
            pass
        run_cli(os.path.join(some, "spec.toml"), os.path.join(some, "run1"))
        with open(os.path.join(some, "run1", "machine", "assertions.json"), "rb") as f:
            raw = f.read()
        if not raw.endswith(b"}\n") or b"\r" in raw:
            fails.append("assertions.json must end with one LF and use LF only")
        keys = list(json.loads(raw.decode("utf-8")))
        if keys != ["schema", "tool", "row", "tier", "spec", "run_id", "inputs", "overall", "points", "errors"]:
            fails.append("assertions.json key order is %s" % keys)
        if os.path.exists(os.path.join(some, "run1", "machine", "assertions.json.part")):
            fails.append("a .part file was left behind")
    finally:
        shutil.rmtree(base, ignore_errors=True)
    if fails:
        for f in fails:
            print("visual-check: FAIL " + f)
        print("visual-check: self-test FAILED (%d)" % len(fails))
        return 1
    print("visual-check: self-test OK")
    return 0


def main(argv=None):
    try:
        sys.stdout.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass
    ap = argparse.ArgumentParser(prog="visual-check.py", description="Check one run folder against a check spec; write machine/assertions.json.")
    ap.add_argument("--spec", help="the check spec (tests/visual/<row>.toml)")
    ap.add_argument("--run", help="the run folder")
    ap.add_argument("--self-test", action="store_true", help="run the canaries")
    ap.add_argument("--emit", help="with --self-test: leave the sample outputs in this folder")
    args = ap.parse_args(argv)
    if args.self_test:
        if args.spec or args.run:
            ap.error("--self-test takes no --spec / --run")
        if args.emit:
            os.makedirs(args.emit, exist_ok=True)
        return selftest(args.emit)
    if args.emit:
        ap.error("--emit needs --self-test")
    if not args.spec or not args.run:
        ap.error("--spec and --run are required (or --self-test)")
    return run_cli(args.spec, args.run)


if __name__ == "__main__":
    sys.exit(main())
