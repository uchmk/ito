//! A forgiving HTML reader: tags, attributes, entities, comments, raw `script`/`style`,
//! and the implied end tags that copied fragments rely on (`li`, `td`, `tr`, `p`...).

pub(crate) enum Node {
    Text(String),
    El(El),
}

pub(crate) struct El {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub kids: Vec<Node>,
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// Tags whose start closes an open `<p>`.
const CLOSES_P: &[&str] = &[
    "p",
    "div",
    "ul",
    "ol",
    "dl",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "pre",
    "blockquote",
    "hr",
    "form",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "aside",
    "address",
    "fieldset",
    "figure",
    "main",
];

/// Deeper than this, a start tag no longer opens anything (keeps the tree and the recursion small).
const MAX_DEPTH: usize = 200;

pub(crate) fn parse(html: &str) -> Vec<Node> {
    let mut b = Builder { stack: Vec::new(), root: Vec::new() };
    let mut i = 0;
    while i < html.len() {
        let Some(rel) = html[i..].find('<') else {
            b.text(&html[i..]);
            break;
        };
        if rel > 0 {
            b.text(&html[i..i + rel]);
        }
        i += rel;
        let rest = &html[i..];
        let after = rest[1..].chars().next();
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |p| p + 3);
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map_or(rest.len(), |p| p + 1);
        } else if rest.starts_with("</") && rest[2..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            let end = rest.find('>').map_or(rest.len(), |p| p + 1);
            let name = rest[2..end].trim_end_matches('>').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
            b.end(&name.to_ascii_lowercase());
            i += end;
        } else if after.is_some_and(|c| c.is_ascii_alphabetic()) {
            let (tag, attrs, selfclose, used) = start_tag(rest);
            i += used;
            if tag == "script" || tag == "style" {
                i += raw_text_len(&html[i..], &tag);
            } else {
                b.start(tag, attrs, selfclose);
            }
        } else {
            b.text("<");
            i += 1;
        }
    }
    b.finish()
}

/// Bytes up to the closing tag of a raw-text element (the closing tag itself is left to the reader).
fn raw_text_len(rest: &str, tag: &str) -> usize {
    let bytes = rest.as_bytes();
    for (p, _) in rest.match_indices("</") {
        let name = bytes.get(p + 2..p + 2 + tag.len());
        if name.is_some_and(|n| n.eq_ignore_ascii_case(tag.as_bytes())) {
            return p;
        }
    }
    rest.len()
}

type Attrs = Vec<(String, String)>;

/// Reads `<tag a=b c="d" />` at the start of `rest`; the last value is the bytes used.
fn start_tag(rest: &str) -> (String, Attrs, bool, usize) {
    let b = rest.as_bytes();
    let mut i = 1;
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
        i += 1;
    }
    let tag = rest[1..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    let mut selfclose = false;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        match b[i] {
            b'>' => {
                i += 1;
                break;
            }
            b'/' => {
                if b.get(i + 1) == Some(&b'>') {
                    selfclose = true;
                    i += 2;
                    break;
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == ns {
            i += 1;
            continue;
        }
        let name = rest[ns..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = decode(&rest[vs..i]);
                if i < b.len() {
                    i += 1;
                }
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                val = decode(&rest[vs..i]);
            }
        }
        attrs.push((name, val));
    }
    (tag, attrs, selfclose, i)
}

struct Builder {
    stack: Vec<El>,
    root: Vec<Node>,
}

impl Builder {
    fn push_node(&mut self, n: Node) {
        match self.stack.last_mut() {
            Some(top) => top.kids.push(n),
            None => self.root.push(n),
        }
    }

    fn text(&mut self, s: &str) {
        if !s.is_empty() {
            self.push_node(Node::Text(decode(s)));
        }
    }

    fn pop(&mut self) {
        if let Some(e) = self.stack.pop() {
            self.push_node(Node::El(e));
        }
    }

    /// Closes the nearest open element named in `targets`, unless a `bounds` element comes first.
    fn close_through(&mut self, targets: &[&str], bounds: &[&str]) {
        let mut found = None;
        for (idx, e) in self.stack.iter().enumerate().rev() {
            let t = e.tag.as_str();
            if targets.contains(&t) {
                found = Some(idx);
                break;
            }
            if bounds.contains(&t) {
                break;
            }
        }
        if let Some(idx) = found {
            while self.stack.len() > idx {
                self.pop();
            }
        }
    }

    fn start(&mut self, tag: String, attrs: Attrs, selfclose: bool) {
        match tag.as_str() {
            "li" => self.close_through(&["li"], &["ul", "ol"]),
            "td" | "th" => self.close_through(&["td", "th"], &["tr", "table"]),
            "tr" => self.close_through(&["tr"], &["table"]),
            "thead" | "tbody" | "tfoot" => self.close_through(&["thead", "tbody", "tfoot"], &["table"]),
            "dt" | "dd" => self.close_through(&["dt", "dd"], &["dl"]),
            _ => {}
        }
        if CLOSES_P.contains(&tag.as_str()) {
            self.close_through(&["p"], &["div", "td", "th", "li", "blockquote", "table", "section", "article"]);
        }
        let leaf = VOID.contains(&tag.as_str()) || selfclose || self.stack.len() >= MAX_DEPTH;
        let el = El { tag, attrs, kids: Vec::new() };
        if leaf {
            self.push_node(Node::El(el));
        } else {
            self.stack.push(el);
        }
    }

    fn end(&mut self, name: &str) {
        if let Some(idx) = self.stack.iter().rposition(|e| e.tag == name) {
            while self.stack.len() > idx {
                self.pop();
            }
        }
    }

    fn finish(mut self) -> Vec<Node> {
        while !self.stack.is_empty() {
            self.pop();
        }
        self.root
    }
}

/// Replaces `&amp;`, `&#38;`, `&#x26;`... with their characters. Unknown names stay as written.
pub(crate) fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        match entity(rest) {
            Some((c, used)) => {
                out.push(c);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `rest` starts with `&`; returns the character and the bytes used.
fn entity(rest: &str) -> Option<(char, usize)> {
    let semi = rest.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i)?;
    let name = &rest[1..semi];
    let c = if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        if n == 0 {
            '\u{fffd}'
        } else {
            char::from_u32(n)?
        }
    } else {
        match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            "nbsp" => '\u{a0}',
            "copy" => '©',
            "reg" => '®',
            "trade" => '™',
            "hellip" => '…',
            "mdash" => '—',
            "ndash" => '–',
            "lsquo" => '‘',
            "rsquo" => '’',
            "ldquo" => '“',
            "rdquo" => '”',
            "bull" => '•',
            "middot" => '·',
            "times" => '×',
            "laquo" => '«',
            "raquo" => '»',
            "yen" => '¥',
            "euro" => '€',
            "deg" => '°',
            "plusmn" => '±',
            _ => return None,
        }
    };
    Some((c, semi + 1))
}
