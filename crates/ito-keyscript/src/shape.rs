//! Test-only: holds the JSON a writer produces to an example document in `docs/schemas/examples/`.
//!
//! `serde_json::Value` may or may not keep key order (`preserve_order` is switched on by other crates of the workspace,
//! and features unify), so the comparison reads the written text with a small order-keeping parser instead.

/// A JSON value with object keys in the order they were written.
#[derive(Debug)]
enum J {
    Null,
    Bool,
    Num,
    Str,
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn kind(&self) -> &'static str {
        match self {
            J::Null => "null",
            J::Bool => "bool",
            J::Num => "number",
            J::Str => "string",
            J::Arr(_) => "array",
            J::Obj(_) => "object",
        }
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        self.ws();
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at byte {}", c as char, self.i))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            match self.s.get(self.i) {
                None => return Err("unterminated string".into()),
                Some(b'"') => {
                    self.i += 1;
                    return String::from_utf8(out).map_err(|e| e.to_string());
                }
                Some(b'\\') => {
                    // Escapes are only skipped: keys never use them and values are compared by type.
                    out.push(b'\\');
                    out.push(*self.s.get(self.i + 1).ok_or("unterminated escape")?);
                    self.i += 2;
                }
                Some(&c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }

    fn word(&mut self, w: &str, v: J) -> Result<J, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(format!("unexpected token at byte {}", self.i))
        }
    }

    fn value(&mut self) -> Result<J, String> {
        self.ws();
        match self.s.get(self.i).copied() {
            None => Err("unexpected end".into()),
            Some(b'{') => {
                self.i += 1;
                let mut kv: Vec<(String, J)> = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(J::Obj(kv));
                }
                loop {
                    let k = self.string()?;
                    if kv.iter().any(|(o, _)| *o == k) {
                        return Err(format!("duplicate key {k}"));
                    }
                    self.eat(b':')?;
                    kv.push((k, self.value()?));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(J::Obj(kv));
                        }
                        _ => return Err(format!("expected a comma or a closing brace at byte {}", self.i)),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(J::Arr(v));
                }
                loop {
                    v.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(J::Arr(v));
                        }
                        _ => return Err(format!("expected a comma or a closing bracket at byte {}", self.i)),
                    }
                }
            }
            Some(b'"') => self.string().map(|_| J::Str),
            Some(b't') => self.word("true", J::Bool),
            Some(b'f') => self.word("false", J::Bool),
            Some(b'n') => self.word("null", J::Null),
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let t = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                t.parse::<f64>().map(|_| J::Num).map_err(|_| format!("bad number at byte {start}"))
            }
        }
    }
}

fn parse(text: &str) -> Result<J, String> {
    let mut p = P { s: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("trailing text at byte {}", p.i));
    }
    Ok(v)
}

fn walk(path: &str, actual: &J, example: &J) -> Result<(), String> {
    match (actual, example) {
        // The example has null: it says nothing about the type (a nullable key has one example with null, one with a value).
        (_, J::Null) => Ok(()),
        (J::Null, e) => Err(format!("{path}: null where the example has a {}", e.kind())),
        (J::Bool, J::Bool) | (J::Num, J::Num) | (J::Str, J::Str) => Ok(()),
        (J::Arr(a), J::Arr(e)) => match (a.first(), e.first()) {
            (Some(a), Some(e)) => walk(&format!("{path}[0]"), a, e),
            _ => Ok(()),
        },
        (J::Obj(a), J::Obj(e)) => {
            let ak: Vec<&str> = a.iter().map(|(k, _)| k.as_str()).collect();
            let ek: Vec<&str> = e.iter().map(|(k, _)| k.as_str()).collect();
            if ak != ek {
                let missing: Vec<&&str> = ek.iter().filter(|k| !ak.contains(k)).collect();
                let extra: Vec<&&str> = ak.iter().filter(|k| !ek.contains(k)).collect();
                return Err(format!(
                    "{path}: keys differ (not in the writer's output: {missing:?}; not in the example: {extra:?}; order: {ak:?} vs {ek:?})"
                ));
            }
            for ((k, a), (_, e)) in a.iter().zip(e) {
                walk(&format!("{path}.{k}"), a, e)?;
            }
            Ok(())
        }
        (a, e) => Err(format!("{path}: a {} where the example has a {}", a.kind(), e.kind())),
    }
}

/// `Ok` when `actual` (JSON text a writer produced) has the shape of `example` (JSON text): the same object keys in the same
/// order, arrays compared by their first elements (when both have one), scalars of the same JSON type. `null` in the example
/// matches anything; `null` in `actual` matches only a `null` in the example.
pub(crate) fn same_shape(actual: &str, example: &str) -> Result<(), String> {
    walk("$", &parse(actual).map_err(|e| format!("actual: {e}"))?, &parse(example).map_err(|e| format!("example: {e}"))?)
}

/// `ito_theme` and `ito_i18n` hold process-wide state; the tests that write or depend on it take turns.
pub(crate) static GLOBALS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Checks `actual` against the example and panics with the written text when the shapes differ.
pub(crate) fn check(name: &str, actual: &str, example: &str) {
    if let Err(e) = same_shape(actual, example) {
        panic!(
            "{name}: {e}
--- written:
{actual}"
        );
    }
}

/// The text of `docs/schemas/examples/<name>.json`.
macro_rules! example {
    ($name:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/schemas/examples/", $name, ".json"))
    };
}
pub(crate) use example;

#[cfg(test)]
mod tests {
    use super::same_shape;

    #[test]
    fn keys_order_types_and_nulls() {
        assert!(same_shape(r#"{"a":1,"b":[{"x":"s"}]}"#, r#"{"a":2.5,"b":[{"x":""},{"x":"t"}]}"#).is_ok());
        assert!(same_shape(r#"{"b":1,"a":1}"#, r#"{"a":1,"b":1}"#).unwrap_err().contains("order"));
        assert!(same_shape(r#"{"a":1}"#, r#"{"a":1,"b":1}"#).unwrap_err().contains("not in the writer's output"));
        assert!(same_shape(r#"{"a":1,"b":1}"#, r#"{"a":1}"#).unwrap_err().contains("not in the example"));
        assert!(same_shape(r#"{"a":"1"}"#, r#"{"a":1}"#).is_err());
        assert!(same_shape(r#"{"a":1}"#, r#"{"a":null}"#).is_ok());
        assert!(same_shape(r#"{"a":null}"#, r#"{"a":null}"#).is_ok());
        assert!(same_shape(r#"{"a":null}"#, r#"{"a":1}"#).unwrap_err().contains("null where"));
        assert!(same_shape(r#"{"a":[]}"#, r#"{"a":[1]}"#).is_ok());
        assert!(same_shape(r#"{"a":[true]}"#, r#"{"a":[1]}"#).is_err());
        assert!(same_shape(r#"{"a":1,"a":2}"#, r#"{"a":1}"#).is_err());
    }
}
