//! The WS-Management messages of the cmd shell: the envelopes we send and the small readers for what comes back.
//! No XML library: the replies are read by tag name (the namespace prefix is ignored).

pub(crate) const SHELL_URI: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd";
const ACTION: &str = "http://schemas.xmlsoap.org/ws/2004/09/transfer";
const SHELL_ACTION: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell";

pub(crate) const SIGNAL_TERMINATE: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/signal/terminate";

/// Text safe inside an element or an attribute.
pub(crate) fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // XML 1.0 cannot carry other control characters at all.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

fn envelope(url: &str, action: &str, shell: Option<&str>, options: &str, body: &str, timeout_s: u32) -> String {
    let id = uuid();
    let selector =
        shell.map_or_else(String::new, |s| format!("<w:SelectorSet><w:Selector Name=\"ShellId\">{}</w:Selector></w:SelectorSet>", esc(s)));
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><s:Envelope xmlns:s=\"http://www.w3.org/2003/05/soap-envelope\" \
xmlns:a=\"http://schemas.xmlsoap.org/ws/2004/08/addressing\" xmlns:w=\"http://schemas.dmtf.org/wbem/wsman/1/wsman.xsd\" \
xmlns:p=\"http://schemas.microsoft.com/wbem/wsman/1/wsman.xsd\" xmlns:rsp=\"{SHELL_URI}\"><s:Header>\
<a:To>{}</a:To><w:ResourceURI s:mustUnderstand=\"true\">{SHELL_URI}</w:ResourceURI>\
<a:ReplyTo><a:Address s:mustUnderstand=\"true\">http://schemas.xmlsoap.org/ws/2004/08/addressing/role/anonymous</a:Address></a:ReplyTo>\
<w:MaxEnvelopeSize s:mustUnderstand=\"true\">512000</w:MaxEnvelopeSize><a:MessageID>uuid:{id}</a:MessageID>\
<w:Locale xml:lang=\"en-US\" s:mustUnderstand=\"false\"/><p:DataLocale xml:lang=\"en-US\" s:mustUnderstand=\"false\"/>\
<w:OperationTimeout>PT{timeout_s}S</w:OperationTimeout><a:Action s:mustUnderstand=\"true\">{action}</a:Action>{selector}{options}</s:Header>\
<s:Body>{body}</s:Body></s:Envelope>",
        esc(url)
    )
}

/// A random version 4 UUID in text.
pub(crate) fn uuid() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub(crate) fn create(url: &str, env: &[(String, String)]) -> String {
    let mut vars = String::new();
    if !env.is_empty() {
        vars.push_str("<rsp:Environment>");
        for (k, v) in env {
            vars.push_str(&format!("<rsp:Variable Name=\"{}\">{}</rsp:Variable>", esc(k), esc(v)));
        }
        vars.push_str("</rsp:Environment>");
    }
    let options =
        "<w:OptionSet><w:Option Name=\"WINRS_NOPROFILE\">FALSE</w:Option><w:Option Name=\"WINRS_CODEPAGE\">65001</w:Option></w:OptionSet>";
    let body = format!(
        "<rsp:Shell><rsp:InputStreams>stdin</rsp:InputStreams><rsp:OutputStreams>stdout stderr</rsp:OutputStreams>{vars}</rsp:Shell>"
    );
    envelope(url, &format!("{ACTION}/Create"), None, options, &body, 60)
}

pub(crate) fn command(url: &str, shell: &str, command: &str) -> String {
    let options = "<w:OptionSet><w:Option Name=\"WINRS_CONSOLEMODE_STDIN\">TRUE</w:Option><w:Option Name=\"WINRS_SKIP_CMD_SHELL\">FALSE</w:Option></w:OptionSet>";
    let body = format!("<rsp:CommandLine><rsp:Command>{}</rsp:Command></rsp:CommandLine>", esc(command));
    envelope(url, &format!("{SHELL_ACTION}/Command"), Some(shell), options, &body, 60)
}

pub(crate) fn receive(url: &str, shell: &str, cmd: &str, wait_s: u32) -> String {
    let body = format!("<rsp:Receive><rsp:DesiredStream CommandId=\"{}\">stdout stderr</rsp:DesiredStream></rsp:Receive>", esc(cmd));
    envelope(url, &format!("{SHELL_ACTION}/Receive"), Some(shell), "", &body, wait_s)
}

pub(crate) fn signal(url: &str, shell: &str, cmd: &str, code: &str) -> String {
    let body = format!("<rsp:Signal CommandId=\"{}\"><rsp:Code>{}</rsp:Code></rsp:Signal>", esc(cmd), esc(code));
    envelope(url, &format!("{SHELL_ACTION}/Signal"), Some(shell), "", &body, 20)
}

pub(crate) fn delete(url: &str, shell: &str) -> String {
    envelope(url, &format!("{ACTION}/Delete"), Some(shell), "", "", 20)
}

/// `(attributes, inner)` of the first element called `name` (any prefix).
fn find<'a>(xml: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let mut from = 0;
    while let Some(i) = xml[from..].find('<') {
        let start = from + i + 1;
        let rest = &xml[start..];
        let name_end = rest.find(['>', ' ', '/', '\t', '\r', '\n'])?;
        let tag = &rest[..name_end];
        let local = tag.rsplit(':').next().unwrap_or(tag);
        if local == name && !tag.starts_with('/') && !tag.starts_with('?') {
            let close = rest.find('>')?;
            let attrs = &rest[name_end..close];
            if attrs.ends_with('/') {
                // An empty slice at the right place, so `all` can tell where this element ended.
                return Some((attrs, &rest[close + 1..close + 1]));
            }
            let body = &rest[close + 1..];
            let stop = body.find(&format!("</{tag}>")).unwrap_or(body.len());
            return Some((attrs, &body[..stop]));
        }
        from = start;
    }
    None
}

/// The text of the first element called `name`, unescaped.
pub(crate) fn text_of(xml: &str, name: &str) -> Option<String> {
    find(xml, name).map(|(_, inner)| unesc(inner))
}

/// Every element called `name`: `(attributes, inner)`. Does not look inside one for another of the same name.
pub(crate) fn all<'a>(xml: &'a str, name: &str) -> Vec<(&'a str, &'a str)> {
    let mut out = vec![];
    let mut rest = xml;
    while let Some((attrs, inner)) = find(rest, name) {
        out.push((attrs, inner));
        let end = (inner.as_ptr() as usize - rest.as_ptr() as usize) + inner.len();
        rest = &rest[end.min(rest.len())..];
    }
    out
}

/// The value of attribute `name` in an attribute string.
pub(crate) fn attr(attrs: &str, name: &str) -> Option<String> {
    let key = format!("{name}=\"");
    let i = attrs.find(&key)? + key.len();
    let j = attrs[i..].find('"')?;
    Some(unesc(&attrs[i..i + j]))
}

pub(crate) fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// What a `Receive` brought: output chunks (`true` = stderr) and, when the command ended, its exit code.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Received {
    pub chunks: Vec<(bool, Vec<u8>)>,
    pub done: Option<u32>,
}

pub(crate) fn parse_receive(xml: &str) -> Received {
    use base64::Engine;
    let mut r = Received::default();
    for (attrs, inner) in all(xml, "Stream") {
        let err = attr(attrs, "Name").as_deref() == Some("stderr");
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(inner.trim()) {
            if !bytes.is_empty() {
                r.chunks.push((err, bytes));
            }
        }
    }
    if let Some((attrs, inner)) = find(xml, "CommandState") {
        if attr(attrs, "State").is_some_and(|s| s.ends_with("/CommandState/Done")) {
            let code = text_of(inner, "ExitCode").and_then(|c| c.trim().parse::<i64>().ok()).unwrap_or(0);
            r.done = Some(code as u32);
        }
    }
    r
}

/// A SOAP fault: its WS-Man error code and the text.
pub(crate) fn fault(xml: &str) -> Option<(Option<u32>, String)> {
    find(xml, "Fault")?;
    let text = text_of(xml, "Text").or_else(|| text_of(xml, "Message")).unwrap_or_else(|| "the server sent a fault".to_owned());
    let code = find(xml, "WSManFault").and_then(|(attrs, _)| attr(attrs, "Code")).and_then(|c| c.parse().ok());
    Some((code, text.trim().to_owned()))
}

/// The Receive wait ran out with nothing to say (not an error).
pub(crate) const FAULT_TIMEOUT: u32 = 2_150_858_793;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_escaped_going_out_and_read_back() {
        assert_eq!(esc("a<b&\"c\u{1}"), "a&lt;b&amp;&quot;c");
        assert_eq!(text_of("<x:A><x:B>1 &amp; 2</x:B></x:A>", "B").as_deref(), Some("1 & 2"));
    }

    #[test]
    fn receive_reads_streams_and_the_exit_code() {
        let xml = "<s:Body><rsp:ReceiveResponse><rsp:Stream Name=\"stdout\" CommandId=\"C\">aGk=</rsp:Stream>\
<rsp:Stream Name=\"stderr\" CommandId=\"C\">ZXJy</rsp:Stream><rsp:Stream Name=\"stdout\" CommandId=\"C\" End=\"true\"/>\
<rsp:CommandState CommandId=\"C\" State=\"http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Done\">\
<rsp:ExitCode>3</rsp:ExitCode></rsp:CommandState></rsp:ReceiveResponse></s:Body>";
        let r = parse_receive(xml);
        assert_eq!(r.chunks, vec![(false, b"hi".to_vec()), (true, b"err".to_vec())]);
        assert_eq!(r.done, Some(3));
    }

    #[test]
    fn a_running_command_has_no_exit_code_yet() {
        let xml =
            "<rsp:CommandState CommandId=\"C\" State=\"http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState/Running\"/>";
        assert_eq!(parse_receive(xml).done, None);
    }

    #[test]
    fn a_fault_gives_its_code_and_text() {
        let xml = "<s:Fault><s:Detail><f:WSManFault Code=\"2150858793\"><f:Message>timed out</f:Message></f:WSManFault></s:Detail>\
<s:Reason><s:Text xml:lang=\"en\">The WS-Management service cannot</s:Text></s:Reason></s:Fault>";
        assert_eq!(fault(xml), Some((Some(FAULT_TIMEOUT), "The WS-Management service cannot".to_owned())));
    }

    #[test]
    fn the_envelopes_carry_the_environment_and_escape_the_command() {
        let c = create("http://h:5985/wsman", &[("A".into(), "1<2".into())]);
        assert!(c.contains("<rsp:Variable Name=\"A\">1&lt;2</rsp:Variable>"));
        assert!(command("u", "S", "echo \"a\" & b").contains("echo &quot;a&quot; &amp; b"));
    }
}
