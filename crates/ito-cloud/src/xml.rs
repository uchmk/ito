//! The few bits of XML that S3 answers with. No general parser: the tags are known and flat.

/// The text inside the first `<tag>…</tag>` of `xml`, entities decoded.
pub(crate) fn first(xml: &str, tag: &str) -> Option<String> {
    blocks(xml, tag).into_iter().next().map(unescape)
}

/// The text inside every `<tag>…</tag>`, in order, entities decoded.
#[cfg(test)]
pub(crate) fn all(xml: &str, tag: &str) -> Vec<String> {
    blocks(xml, tag).into_iter().map(unescape).collect()
}

/// The inner XML of every `<tag>…</tag>` (not decoded), to look inside with [`first`].
pub(crate) fn blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(&close) else { break };
        out.push(&after[..j]);
        rest = &after[j + close.len()..];
    }
    out
}

pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

pub(crate) fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flat_tags() {
        let x = "<R><Contents><Key>a&amp;b.txt</Key><Size>3</Size></Contents><Contents><Key>c</Key><Size>4</Size></Contents></R>";
        let b = blocks(x, "Contents");
        assert_eq!(b.len(), 2);
        assert_eq!(first(b[0], "Key").as_deref(), Some("a&b.txt"));
        assert_eq!(all(x, "Size"), ["3", "4"]);
        assert_eq!(first(x, "Nope"), None);
    }

    #[test]
    fn decodes_and_encodes_entities() {
        assert_eq!(unescape("&lt;&#65;&#x42;&gt; &unknown; &"), "<AB> &unknown; &");
        assert_eq!(unescape(&escape("a<b>&\"'")), "a<b>&\"'");
    }
}
