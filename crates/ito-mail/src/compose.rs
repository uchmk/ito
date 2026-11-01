//! Build an outgoing message as RFC 5322 bytes: CRLF lines, `Date`, `Message-ID`, RFC 2047 words in the subject and in
//! display names, a base64 UTF-8 body (plain text, or plain + HTML as `multipart/alternative`), and attachments as
//! `multipart/mixed` with RFC 2231 + RFC 2047 file names. Nothing here sends: hand the bytes to SMTP or to an API (Gmail's
//! `messages/send` takes them base64url in `raw`).

use std::time::SystemTime;

/// What to send. Everything is borrowed; header values may not carry a line break (any CR or LF is dropped).
#[derive(Clone, Copy, Debug, Default)]
pub struct Outgoing<'a> {
    /// `"Name <addr>"` or `addr`.
    pub from: &'a str,
    /// Comma-separated; each is `addr` or `"Name <addr>"` (a comma in a quoted name is kept).
    pub to: &'a str,
    pub cc: &'a str,
    /// Written as a `Bcc:` header (an API such as Gmail's delivers to it, then drops it); empty for none.
    pub bcc: &'a str,
    pub subject: &'a str,
    /// The plain-text body (LF or CRLF).
    pub text: &'a str,
    /// An HTML body; with it the message is `multipart/alternative` (text first).
    pub html: Option<&'a str>,
    /// The `Message-ID` of the mail this answers, with its angle brackets.
    pub in_reply_to: Option<&'a str>,
    /// The `References` chain (the original's, then its `Message-ID`); empty for none.
    pub references: &'a str,
    pub files: &'a [Attachment<'a>],
}

/// A file to attach.
#[derive(Clone, Copy, Debug)]
pub struct Attachment<'a> {
    pub name: &'a str,
    /// The media type; `None` is guessed from the extension (`application/octet-stream` when unknown).
    pub mime: Option<&'a str>,
    pub bytes: &'a [u8],
    /// A `Content-ID` (without the angle brackets) makes this an inline part of the HTML body (`<img src="cid:…">`)
    /// instead of an attachment. Without `html`, or with an empty id, it is attached as usual.
    pub cid: Option<&'a str>,
}

/// The message as bytes, ready to send. `now` is the `Date` (in UTC).
pub fn compose(m: &Outgoing, now: SystemTime) -> Vec<u8> {
    let mut h = String::new();
    let from = clean(m.from);
    let domain = domain_of(&from);
    header(&mut h, "From", &address_list(&from));
    for (name, list) in [("To", m.to), ("Cc", m.cc), ("Bcc", m.bcc)] {
        let list = address_list(&clean(list));
        if !list.is_empty() {
            header(&mut h, name, &list);
        }
    }
    header(&mut h, "Subject", &subject(&clean(m.subject)));
    header(&mut h, "Date", &chrono::DateTime::<chrono::Utc>::from(now).format("%a, %d %b %Y %H:%M:%S +0000").to_string());
    header(&mut h, "Message-ID", &format!("<{}@{}>", hex(16), domain));
    if let Some(r) = m.in_reply_to.map(clean).filter(|r| !r.is_empty()) {
        header(&mut h, "In-Reply-To", &r);
    }
    let refs = clean(m.references);
    if !refs.is_empty() {
        header(&mut h, "References", &refs);
    }
    header(&mut h, "MIME-Version", "1.0");

    let mut out = Vec::new();
    let body_kind = if m.html.is_some() { Kind::Alternative } else { Kind::Text };
    let inline = |f: &Attachment| m.html.is_some() && f.cid.map(clean).is_some_and(|c| !c.is_empty());
    if m.files.iter().all(inline) {
        out.extend(part_headers_and_body(m, body_kind, &mut h).into_bytes());
        return finish(h, out);
    }
    let outer = boundary();
    header(&mut h, "Content-Type", &format!("multipart/mixed; boundary=\"{outer}\""));
    let mut tmp = String::new();
    let body = part_headers_and_body(m, body_kind, &mut tmp);
    let mut b = format!("--{outer}\r\n{tmp}\r\n{body}");
    for f in m.files.iter().filter(|f| !inline(f)) {
        let mime = f.mime.map(clean).filter(|s| !s.is_empty()).unwrap_or_else(|| guess_mime(f.name).to_owned());
        let name = clean(f.name);
        b.push_str(&format!("--{outer}\r\n"));
        b.push_str(&format!("Content-Type: {mime}; {}\r\n", name_params("name", &name)));
        b.push_str("Content-Transfer-Encoding: base64\r\n");
        b.push_str(&format!("Content-Disposition: attachment; {}\r\n\r\n", name_params("filename", &name)));
        b.push_str(&wrap76(&encode64(f.bytes)));
    }
    b.push_str(&format!("--{outer}--\r\n"));
    out.extend(b.into_bytes());
    finish(h, out)
}

fn finish(headers: String, body: Vec<u8>) -> Vec<u8> {
    let mut out = headers.into_bytes();
    out.extend_from_slice(b"\r\n");
    out.extend(body);
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Alternative,
}

/// The `Content-*` headers of the body part go into `hdrs` (each ending CRLF); the body itself is returned.
fn part_headers_and_body(m: &Outgoing, kind: Kind, hdrs: &mut String) -> String {
    let text = crlf(m.text);
    match (kind, m.html) {
        (Kind::Alternative, Some(html)) => {
            let b = boundary();
            hdrs.push_str(&format!("Content-Type: multipart/alternative; boundary=\"{b}\"\r\n"));
            let mut s = String::new();
            s.push_str(&format!(
                "--{b}\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
                wrap76(&encode64(text.as_bytes()))
            ));
            let html_part = |s: &mut String| {
                s.push_str(&format!(
                    "Content-Type: text/html; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
                    wrap76(&encode64(crlf(html).as_bytes()))
                ));
            };
            let pics: Vec<&Attachment> = m.files.iter().filter(|f| f.cid.map(clean).is_some_and(|c| !c.is_empty())).collect();
            if pics.is_empty() {
                s.push_str(&format!("--{b}\r\n"));
                html_part(&mut s);
            } else {
                let r = boundary();
                s.push_str(&format!("--{b}\r\nContent-Type: multipart/related; type=\"text/html\"; boundary=\"{r}\"\r\n\r\n--{r}\r\n"));
                html_part(&mut s);
                for f in pics {
                    let mime = f.mime.map(clean).filter(|s| !s.is_empty()).unwrap_or_else(|| guess_mime(f.name).to_owned());
                    let name = clean(f.name);
                    s.push_str(&format!("--{r}\r\nContent-Type: {mime}; {}\r\n", name_params("name", &name)));
                    s.push_str("Content-Transfer-Encoding: base64\r\n");
                    s.push_str(&format!("Content-ID: <{}>\r\n", clean(f.cid.unwrap_or(""))));
                    s.push_str(&format!("Content-Disposition: inline; {}\r\n\r\n", name_params("filename", &name)));
                    s.push_str(&wrap76(&encode64(f.bytes)));
                }
                s.push_str(&format!("--{r}--\r\n"));
            }
            s.push_str(&format!("--{b}--\r\n"));
            s
        }
        _ => {
            hdrs.push_str("Content-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n");
            wrap76(&encode64(text.as_bytes()))
        }
    }
}

/// Without CR or LF (a header value must be one line).
fn clean(s: &str) -> String {
    s.chars().filter(|c| *c != '\r' && *c != '\n').collect::<String>().trim().to_owned()
}

fn crlf(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

/// `Name: value` folded at spaces so no line passes 78 characters where it can be helped.
fn header(out: &mut String, name: &str, value: &str) {
    let mut line = format!("{name}:");
    for (i, word) in value.split(' ').enumerate() {
        if i == 0 && word.is_empty() {
            continue;
        }
        if line.len() + 1 + word.len() > 76 && line.trim_end().len() > name.len() + 1 {
            out.push_str(&line);
            out.push_str("\r\n");
            line = String::from(" ");
            line.push_str(word);
        } else {
            line.push(' ');
            line.push_str(word);
        }
    }
    out.push_str(&line);
    out.push_str("\r\n");
}

fn subject(s: &str) -> String {
    if s.is_ascii() {
        s.to_owned()
    } else {
        encoded_words(s)
    }
}

/// RFC 2047 `=?UTF-8?B?…?=` words (each at most 75 characters) joined by a space, which the folding then breaks at.
fn encoded_words(s: &str) -> String {
    const CHUNK: usize = 45; // 60 base64 characters + 12 of framing = 72
    let mut words = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if cur.len() + c.len_utf8() > CHUNK {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", encode64(w.as_bytes()))).collect::<Vec<_>>().join(" ")
}

/// A comma-separated address list, each name encoded or quoted as it needs.
fn address_list(s: &str) -> String {
    split_addresses(s).iter().map(|a| one_address(a)).collect::<Vec<_>>().join(", ")
}

/// Split at the commas that are outside quotes and angle brackets.
fn split_addresses(s: &str) -> Vec<String> {
    let (mut out, mut cur) = (Vec::new(), String::new());
    let (mut quoted, mut angle, mut escaped) = (false, false, false);
    for c in s.chars() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' if !quoted && !angle => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out.into_iter().map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect()
}

fn one_address(a: &str) -> String {
    let (Some(l), Some(r)) = (a.rfind('<'), a.rfind('>')) else { return a.to_owned() };
    if l > r {
        return a.to_owned();
    }
    let addr = a[l + 1..r].trim();
    let name = a[..l].trim();
    let name =
        if name.starts_with('"') && name.ends_with('"') && name.len() >= 2 { unquote(&name[1..name.len() - 1]) } else { name.to_owned() };
    if name.is_empty() {
        return format!("<{addr}>");
    }
    let shown = if !name.is_ascii() {
        encoded_words(&name)
    } else if name.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ' || "!#$%&'*+-/=?^_`{|}~".contains(c)) {
        name
    } else {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    };
    format!("{shown} <{addr}>")
}

fn unquote(s: &str) -> String {
    let mut out = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc || c != '\\' {
            out.push(c);
            esc = false;
        } else {
            esc = true;
        }
    }
    out
}

/// The part after `@` of the address in a From value, as a safe host name.
fn domain_of(from: &str) -> String {
    let addr = match (from.rfind('<'), from.rfind('>')) {
        (Some(l), Some(r)) if l < r => &from[l + 1..r],
        _ => from,
    };
    let d: String = addr
        .rsplit_once('@')
        .map(|(_, d)| d)
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    if d.is_empty() {
        "localhost".to_owned()
    } else {
        d
    }
}

/// `name="…"` for a plain name, and for any other both `name*=UTF-8''…` (RFC 2231) and `name="=?UTF-8?B?…?="` (RFC 2047).
fn name_params(key: &str, name: &str) -> String {
    if name.is_ascii() {
        return format!("{key}=\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""));
    }
    let mut pct = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&b) {
            pct.push(b as char);
        } else {
            pct.push_str(&format!("%{b:02X}"));
        }
    }
    format!("{key}*=UTF-8''{pct}; {key}=\"{}\"", encoded_words(name).replace(' ', ""))
}

fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ics" => "text/calendar",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn boundary() -> String {
    format!("=_ito_{}", hex(12))
}

/// `n` random bytes as hex.
fn hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    if getrandom::fill(&mut b).is_err() {
        // No system randomness: the clock's nanoseconds, spread, are still unique enough for an id.
        let t = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t >> ((i % 16) * 8)) as u8 ^ (i as u8).wrapping_mul(97);
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn encode64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Lines of 76 characters, each ending CRLF.
fn wrap76(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 38 + 2);
    for line in s.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(line).expect("base64 is ASCII"));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn when() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_500_000)
    }

    /// The header block as unfolded `(name, value)` pairs, and the body after the blank line.
    fn parse(raw: &[u8]) -> (Vec<(String, String)>, String) {
        let s = String::from_utf8(raw.to_vec()).expect("ASCII or UTF-8");
        assert!(!s.replace("\r\n", "").contains('\n') && !s.replace("\r\n", "").contains('\r'), "CRLF only");
        let (head, body) = s.split_once("\r\n\r\n").expect("a blank line");
        let mut hs: Vec<(String, String)> = Vec::new();
        for line in head.split("\r\n") {
            assert!(line.len() <= 998 && line.is_ascii(), "{line}");
            if line.starts_with(' ') || line.starts_with('\t') {
                hs.last_mut().expect("a header before a fold").1.push_str(line);
            } else {
                let (n, v) = line.split_once(':').expect("name: value");
                hs.push((n.to_owned(), v.trim_start().to_owned()));
            }
        }
        (hs, body.to_owned())
    }

    fn get<'a>(hs: &'a [(String, String)], n: &str) -> &'a str {
        &hs.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).unwrap_or_else(|| panic!("no {n}")).1
    }

    fn unb64(s: &str) -> Vec<u8> {
        crate::base64(&s.replace("\r\n", "")).expect("base64")
    }

    #[test]
    fn a_japanese_message_reads_back() {
        let long = "とても長い件名です。".repeat(8);
        let m = Outgoing {
            from: "山田 太郎 <taro@example.co.jp>",
            to: "\"Doe, Jane\" <jane@example.com>, 佐藤 花子 <hanako@example.jp>, bare@example.org",
            cc: "cc@example.com",
            bcc: "hidden@example.com",
            subject: &long,
            text: "こんにちは\n二行目\n",
            html: None,
            in_reply_to: Some("<orig@example.com>"),
            references: "<a@x> <orig@example.com>",
            files: &[],
        };
        let (hs, body) = parse(&compose(&m, when()));
        assert_eq!(get(&hs, "MIME-Version"), "1.0");
        assert!(get(&hs, "Date").ends_with("+0000"), "{}", get(&hs, "Date"));
        assert!(get(&hs, "Message-ID").ends_with("@example.co.jp>"));
        assert_eq!(get(&hs, "In-Reply-To"), "<orig@example.com>");
        assert_eq!(crate::decode_words(get(&hs, "From")), "山田 太郎 <taro@example.co.jp>");
        assert_eq!(
            crate::decode_words(get(&hs, "To")),
            "\"Doe, Jane\" <jane@example.com>, 佐藤 花子 <hanako@example.jp>, bare@example.org"
        );
        assert_eq!(get(&hs, "Bcc"), "hidden@example.com");
        assert_eq!(crate::decode_words(get(&hs, "Subject")), long);
        // Every encoded word fits in 75 characters.
        for w in get(&hs, "Subject").split(' ') {
            assert!(w.len() <= 75, "{w}");
        }
        assert!(get(&hs, "Content-Type").starts_with("text/plain; charset=UTF-8"));
        assert_eq!(String::from_utf8(unb64(&body)).unwrap(), "こんにちは\r\n二行目\r\n");
        assert!(body.split("\r\n").all(|l| l.len() <= 76));
    }

    #[test]
    fn attachments_and_html_nest() {
        let m = Outgoing {
            from: "a@example.com",
            to: "b@example.com",
            subject: "Report",
            text: "see attached",
            html: Some("<p>see attached</p>"),
            files: &[
                Attachment { name: "報告 書.pdf", mime: None, bytes: b"%PDF-1 \x00\xff data", cid: None },
                Attachment { name: "plain.csv", mime: Some("text/csv"), bytes: b"a,b\n", cid: None },
            ],
            ..Default::default()
        };
        let raw = compose(&m, when());
        let (hs, body) = parse(&raw);
        let ct = get(&hs, "Content-Type");
        assert!(ct.starts_with("multipart/mixed; boundary=\""), "{ct}");
        let outer = ct.split('"').nth(1).unwrap().to_owned();
        let parts: Vec<&str> = body.split(&format!("--{outer}")).collect();
        // preamble, body part, two files, closing "--".
        assert_eq!(parts.len(), 5, "{body}");
        assert!(parts[1].contains("multipart/alternative"));
        assert!(parts[1].contains("text/html"));
        assert!(parts[2].contains("application/pdf"), "{}", parts[2]);
        assert!(parts[2].contains("filename*=UTF-8''%E5%A0%B1%E5%91%8A%20%E6%9B%B8.pdf"));
        let enc = parts[2].split("name=\"").nth(1).unwrap().split('"').next().unwrap();
        assert_eq!(crate::decode_words(enc), "報告 書.pdf");
        let data = parts[2].split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(unb64(data), b"%PDF-1 \x00\xff data");
        assert!(parts[3].contains("Content-Type: text/csv; name=\"plain.csv\""));
        assert!(parts[4].starts_with("--"));
    }

    #[test]
    fn inline_pictures_go_beside_the_html() {
        let m = Outgoing {
            from: "a@example.com",
            to: "b@example.com",
            text: "hi",
            html: Some("<img src=\"cid:logo@x\">"),
            files: &[Attachment { name: "logo.png", mime: None, bytes: b"PNG1", cid: Some("logo@x") }],
            ..Default::default()
        };
        let (hs, body) = parse(&compose(&m, when()));
        assert!(get(&hs, "Content-Type").starts_with("multipart/alternative"));
        assert!(body.contains("multipart/related; type=\"text/html\""), "{body}");
        assert!(body.contains("Content-ID: <logo@x>"));
        assert!(body.contains("Content-Disposition: inline; "));
        assert!(!body.contains("multipart/mixed"));
        // Without html the id is ignored and the file is a plain attachment.
        let m = Outgoing { html: None, ..m };
        let (hs, body) = parse(&compose(&m, when()));
        assert!(get(&hs, "Content-Type").starts_with("multipart/mixed"));
        assert!(body.contains("Content-Disposition: attachment; ") && !body.contains("Content-ID"));
    }

    #[test]
    fn headers_cannot_be_injected_and_odd_input_is_safe() {
        let m = Outgoing { from: "a@x.com\r\nBcc: evil@x.com", to: "b@x.com", subject: "hi\r\nX: y", text: "t", ..Default::default() };
        let (hs, _) = parse(&compose(&m, when()));
        assert!(hs.iter().all(|(k, _)| k != "X" && k != "Bcc"));
        // No From domain, no recipients, nothing at all.
        let (hs, _) = parse(&compose(&Outgoing::default(), when()));
        assert!(get(&hs, "Message-ID").ends_with("@localhost>"));
        assert_eq!(split_addresses("a@x, \"B, C\" <b@x>,, d@x").len(), 3);
        assert_eq!(encode64(b"Ma"), "TWE=");
        assert_eq!(encode64(b"Man"), "TWFu");
    }
}
