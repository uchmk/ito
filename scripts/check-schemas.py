#!/usr/bin/env python3
"""Checks docs/schemas/*.v1.json against the examples and against what the lane scripts emit.

Stdlib only. One validator for exactly the JSON Schema subset the schemas use; a keyword outside the subset is an error
(in a schema), so a schema cannot quietly say something this script does not check.

  1. every docs/schemas/*.v1.json is a valid schema of the subset;
  2. every docs/schemas/examples/<schema>[-variant].json validates against <schema>.v1.json;
  3. `visual-check.py --self-test --emit T` and `evidence.py --self-test --emit T` run, and every file they emit
     (assertions, manifest, verdict, intent, run, trace, shots, state) validates against its schema.

Prints `check-schemas: OK` last and exits 0, or the failures and exits 1.
"""
import json
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
SCHEMAS = os.path.join(ROOT, "docs", "schemas")
EXAMPLES = os.path.join(SCHEMAS, "examples")
LANES = os.path.join(HERE, "lanes")

KEYWORDS = {
    "$schema", "$id", "title", "description", "type", "properties", "required", "additionalProperties", "items",
    "enum", "const", "minItems", "maxItems", "minimum", "$defs", "$ref", "oneOf",
}
TYPES = {"string", "number", "integer", "boolean", "object", "array", "null"}
NAMES = ["state", "shot", "trace", "run", "intent", "manifest", "assertions", "verdict", "vision", "vision-key"]


def read_text(path):
    with open(path, encoding="utf-8", newline="") as f:
        return f.read()


def load_json(path):
    return json.loads(read_text(path))


def type_ok(value, t):
    if t == "null":
        return value is None
    if t == "boolean":
        return isinstance(value, bool)
    if t == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if t == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if t == "string":
        return isinstance(value, str)
    if t == "array":
        return isinstance(value, list)
    if t == "object":
        return isinstance(value, dict)
    return False


def same(a, b):
    """JSON equality that does not confuse 1 with true."""
    if isinstance(a, bool) != isinstance(b, bool):
        return False
    return a == b


def resolve(root, ref):
    prefix = "#/$defs/"
    if not ref.startswith(prefix):
        return None
    return (root.get("$defs") or {}).get(ref[len(prefix):])


def check_schema(schema, root, path, errors):
    """Checks that `schema` only uses the subset (and says what it means)."""
    if not isinstance(schema, dict):
        errors.append("%s: a schema must be an object" % path)
        return
    for k in schema:
        if k not in KEYWORDS:
            errors.append("%s: keyword %r is outside the subset" % (path, k))
    if "type" in schema:
        t = schema["type"]
        ts = t if isinstance(t, list) else [t]
        if not ts or any(not isinstance(x, str) or x not in TYPES for x in ts):
            errors.append("%s: bad type %r" % (path, t))
    for k in ("minItems", "maxItems"):
        if k in schema and (not isinstance(schema[k], int) or isinstance(schema[k], bool) or schema[k] < 0):
            errors.append("%s: %s must be a non-negative integer" % (path, k))
    if "minimum" in schema and (not isinstance(schema["minimum"], (int, float)) or isinstance(schema["minimum"], bool)):
        errors.append("%s: minimum must be a number" % path)
    if "enum" in schema and (not isinstance(schema["enum"], list) or not schema["enum"]):
        errors.append("%s: enum must be a non-empty array" % path)
    if "required" in schema:
        req = schema["required"]
        if not isinstance(req, list) or any(not isinstance(x, str) for x in req):
            errors.append("%s: required must be an array of strings" % path)
        else:
            props = schema.get("properties", {})
            for x in req:
                if x not in props:
                    errors.append("%s: required key %r is not in properties" % (path, x))
    if "properties" in schema:
        props = schema["properties"]
        if not isinstance(props, dict):
            errors.append("%s: properties must be an object" % path)
        else:
            for k, v in props.items():
                check_schema(v, root, "%s.properties.%s" % (path, k), errors)
    if "additionalProperties" in schema:
        ap = schema["additionalProperties"]
        if ap is not False:
            check_schema(ap, root, path + ".additionalProperties", errors)
    if "items" in schema:
        check_schema(schema["items"], root, path + ".items", errors)
    if "oneOf" in schema:
        one = schema["oneOf"]
        if not isinstance(one, list) or not one:
            errors.append("%s: oneOf must be a non-empty array" % path)
        else:
            for i, v in enumerate(one):
                check_schema(v, root, "%s.oneOf[%d]" % (path, i), errors)
    if "$ref" in schema:
        ref = schema["$ref"]
        if not isinstance(ref, str) or resolve(root, ref) is None:
            errors.append("%s: $ref %r does not name a $defs entry" % (path, ref))
    if "$defs" in schema:
        if path != "$":
            errors.append("%s: $defs only at the top" % path)
        elif not isinstance(schema["$defs"], dict):
            errors.append("$defs must be an object")
        else:
            for k, v in schema["$defs"].items():
                check_schema(v, root, "$defs.%s" % k, errors)
    # The strict style: an object schema that lists properties closes itself and requires them all, except a free-form
    # value (no type) and a map (additionalProperties is a schema).
    t = schema.get("type")
    if t == "object" and "properties" in schema:
        if schema.get("additionalProperties") is not False:
            errors.append("%s: an object with properties must set additionalProperties false" % path)
        if sorted(schema.get("required", [])) != sorted(schema["properties"]):
            errors.append("%s: required must list every key of properties" % path)


def validate(value, schema, root, path, errors):
    if "$ref" in schema:
        target = resolve(root, schema["$ref"])
        validate(value, target, root, path, errors)
    if "type" in schema:
        t = schema["type"]
        ts = t if isinstance(t, list) else [t]
        if not any(type_ok(value, x) for x in ts):
            errors.append("%s: expected %s, got %s" % (path, "/".join(ts), json.dumps(value)[:60]))
            return
    if "const" in schema and not same(value, schema["const"]):
        errors.append("%s: expected the constant %s, got %s" % (path, json.dumps(schema["const"]), json.dumps(value)[:60]))
    if "enum" in schema and not any(same(value, e) for e in schema["enum"]):
        errors.append("%s: %s is not one of %s" % (path, json.dumps(value)[:60], json.dumps(schema["enum"])))
    if "minimum" in schema and isinstance(value, (int, float)) and not isinstance(value, bool) and value < schema["minimum"]:
        errors.append("%s: %s is below the minimum %s" % (path, value, schema["minimum"]))
    if "oneOf" in schema:
        hits = 0
        for sub in schema["oneOf"]:
            sub_errors = []
            validate(value, sub, root, path, sub_errors)
            if not sub_errors:
                hits += 1
        if hits != 1:
            errors.append("%s: matches %d of the oneOf alternatives, not exactly 1" % (path, hits))
    if isinstance(value, dict):
        props = schema.get("properties", {})
        for k in schema.get("required", []):
            if k not in value:
                errors.append("%s: missing key %r" % (path, k))
        ap = schema.get("additionalProperties", None)
        for k, v in value.items():
            if k in props:
                validate(v, props[k], root, "%s.%s" % (path, k), errors)
            elif ap is False:
                errors.append("%s: unexpected key %r" % (path, k))
            elif isinstance(ap, dict):
                validate(v, ap, root, "%s.%s" % (path, k), errors)
    if isinstance(value, list):
        if "minItems" in schema and len(value) < schema["minItems"]:
            errors.append("%s: %d items, at least %d wanted" % (path, len(value), schema["minItems"]))
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append("%s: %d items, at most %d allowed" % (path, len(value), schema["maxItems"]))
        if "items" in schema:
            for i, v in enumerate(value):
                validate(v, schema["items"], root, "%s[%d]" % (path, i), errors)


def load_schemas(errors):
    out = {}
    for n in NAMES:
        p = os.path.join(SCHEMAS, n + ".v1.json")
        try:
            s = load_json(p)
        except (OSError, ValueError) as e:
            errors.append("%s: cannot read: %s" % (p, e))
            continue
        before = len(errors)
        check_schema(s, s, "$", errors)
        for i in range(before, len(errors)):
            errors[i] = "%s.v1.json %s" % (n, errors[i])
        if s.get("$id", "").rsplit("/", 1)[-1] != n + ".v1.json":
            errors.append("%s.v1.json: $id does not end with the file name" % n)
        out[n] = s
    return out


def check_value(schemas, name, value, label, errors):
    schema = schemas.get(name)
    if schema is None:
        errors.append("%s: no schema %r" % (label, name))
        return
    found = []
    validate(value, schema, schema, "$", found)
    for e in found:
        errors.append("%s (%s.v1): %s" % (label, name, e))


def check_file(schemas, name, path, label, errors, lines=False):
    try:
        text = read_text(path)
    except OSError as e:
        errors.append("%s: cannot read: %s" % (label, e))
        return
    if "\r" in text:
        errors.append("%s: has CR (LF only)" % label)
    try:
        if lines:
            rows = [x for x in text.split("\n") if x != ""]
            if not rows:
                errors.append("%s: empty" % label)
            for i, row in enumerate(rows):
                check_value(schemas, name, json.loads(row), "%s line %d" % (label, i + 1), errors)
        else:
            check_value(schemas, name, json.loads(text), label, errors)
    except ValueError as e:
        errors.append("%s: not JSON: %s" % (label, e))


def check_examples(schemas, errors):
    seen = set()
    for f in sorted(os.listdir(EXAMPLES)):
        if not f.endswith(".json"):
            continue
        stem = f[:-5]
        name = max((n for n in NAMES if stem == n or stem.startswith(n + "-")), key=len, default=stem.split("-", 1)[0])
        seen.add(name)
        check_file(schemas, name, os.path.join(EXAMPLES, f), "examples/" + f, errors, lines=(name == "trace"))
    for n in NAMES:
        if n not in seen:
            errors.append("examples/: no example for %s.v1" % n)


def emitted_kind(rel):
    """Which schema an emitted file (path relative to the emit folder, with / separators) follows; None for others."""
    parts = rel.split("/")
    base = parts[-1]
    parent = parts[-2] if len(parts) > 1 else ""
    if base == "assertions.json":
        return "assertions"
    if base == "manifest.json":
        return "manifest"
    if base.startswith("verdict-") and base.endswith(".json"):
        return "verdict"
    if base == "intent.json" and parent == "pre":
        return "intent"
    if base == "run.json":
        return "run"
    if base == "trace.jsonl":
        return "trace"
    if base.endswith(".json") and parent == "shots":
        return "shot"
    if base == "key.json" and parent == "vision":
        return "vision-key"
    if base.endswith(".json") and parent == "answers" and len(parts) > 2 and parts[-3] == "vision":
        return "vision"
    if base.endswith(".json") and parent == "state":
        return "state"
    return None


def run_self_test(script, errors):
    path = os.path.join(LANES, script)
    if not os.path.isfile(path):
        errors.append("%s: not found" % script)
        return []
    tmp = tempfile.mkdtemp(prefix="check-schemas-")
    r = subprocess.run([sys.executable, path, "--self-test", "--emit", tmp], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        errors.append("%s --self-test --emit failed (exit %d): %s" % (script, r.returncode, (r.stdout + r.stderr).strip()[-600:]))
        return []
    files = []
    for d, _, names in os.walk(tmp):
        for n in names:
            p = os.path.join(d, n)
            rel = os.path.relpath(p, tmp).replace(os.sep, "/")
            kind = emitted_kind(rel)
            if kind:
                files.append((kind, p, "%s emit/%s" % (script, rel)))
    if not files:
        errors.append("%s --self-test --emit wrote nothing to validate" % script)
    return files


def main():
    errors = []
    schemas = load_schemas(errors)
    check_examples(schemas, errors)
    counts = {}
    for script in ("visual-check.py", "evidence.py"):
        for kind, p, label in run_self_test(script, errors):
            counts[kind] = counts.get(kind, 0) + 1
            check_file(schemas, kind, p, label, errors, lines=(kind == "trace"))
    if errors:
        for e in errors:
            print("check-schemas: " + e)
        print("check-schemas: FAILED (%d problems)" % len(errors))
        return 1
    print("check-schemas: validated emitted files: " + ", ".join("%s %d" % (k, counts[k]) for k in sorted(counts)))
    print("check-schemas: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
