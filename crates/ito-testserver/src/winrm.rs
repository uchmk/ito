//! A fake WinRM server (the WS-Management cmd shell over HTTP). See [`WinrmServer`].

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::stream::{b64_decode, b64_encode, Conn, Reader};
use crate::tls::{self, Cert, Fingerprint};
use crate::{unique, Acceptor};

/// How the connection is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Plain HTTP, like port 5985.
    Http,
    /// HTTPS with a self-signed certificate, like port 5986.
    Https,
}

/// What a scripted command does.
#[derive(Clone, Debug, Default)]
pub struct Output {
    /// Pieces of output, in the order they are sent: `(is_stderr, bytes)`. Each is its own `Stream` element.
    pub chunks: Vec<(bool, Vec<u8>)>,
    /// The exit code the command ends with.
    pub exit: u32,
    /// How long after the command starts it ends (the output is sent at the first `Receive`).
    pub delay: Duration,
    /// Never end by itself: only a `Signal` (or deleting the shell) ends it.
    pub hang: bool,
}

impl Output {
    pub fn new() -> Output {
        Output::default()
    }

    pub fn stdout(mut self, text: impl AsRef<[u8]>) -> Output {
        self.chunks.push((false, text.as_ref().to_vec()));
        self
    }

    pub fn stderr(mut self, text: impl AsRef<[u8]>) -> Output {
        self.chunks.push((true, text.as_ref().to_vec()));
        self
    }

    pub fn exit(mut self, code: u32) -> Output {
        self.exit = code;
        self
    }

    pub fn delay(mut self, delay: Duration) -> Output {
        self.delay = delay;
        self
    }

    pub fn hang(mut self) -> Output {
        self.hang = true;
        self
    }
}

/// A request the server received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The WS-Man action's last part: `Create`, `Command`, `Receive`, `Signal`, `Delete`.
    pub action: String,
    /// The shell the request names (none for `Create`).
    pub shell: Option<String>,
    /// For `Command`, the command line; for `Signal`, the signal code URI.
    pub detail: Option<String>,
    /// The whole SOAP envelope.
    pub body: String,
}

struct Cmd {
    script: Output,
    started: Instant,
    sent: bool,
    signalled: bool,
}

#[derive(Default)]
struct Shell {
    commands: HashMap<String, Cmd>,
}

struct State {
    mode: Mode,
    credentials: Option<(String, String)>,
    cert: Option<Cert>,
    scripts: Mutex<Vec<(String, Output)>>,
    shells: Mutex<HashMap<String, Shell>>,
    requests: Mutex<Vec<Request>>,
    log: Mutex<Vec<String>>,
    commands_run: Mutex<Vec<String>>,
    deleted: Mutex<u32>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A fake WinRM service on `127.0.0.1` at `/wsman`, good enough for `ito-winrm`'s `exec`: Basic authentication, and
/// the cmd shell's `Create`, `Command`, `Receive` (stdout and stderr chunks, the exit code, a wait that ends in the
/// "nothing yet" fault), `Signal` and `Delete`. What a command prints and returns is scripted with
/// [`on_command`](Self::on_command); every request is recorded. NTLM is not offered. It stops when it is dropped.
pub struct WinrmServer {
    state: Arc<State>,
    acceptor: Acceptor,
}

impl WinrmServer {
    /// Start a server in `mode` that takes any Basic login.
    pub fn start(mode: Mode) -> io::Result<WinrmServer> {
        Self::start_with(mode, None)
    }

    /// Start a server that wants this Basic user and password (HTTP 401 for any other).
    pub fn start_with_login(mode: Mode, user: &str, password: &str) -> io::Result<WinrmServer> {
        Self::start_with(mode, Some((user.to_owned(), password.to_owned())))
    }

    fn start_with(mode: Mode, credentials: Option<(String, String)>) -> io::Result<WinrmServer> {
        let cert = (mode == Mode::Https).then(Cert::generate);
        let state = Arc::new(State {
            mode,
            credentials,
            cert,
            scripts: Mutex::new(Vec::new()),
            shells: Mutex::new(HashMap::new()),
            requests: Mutex::new(Vec::new()),
            log: Mutex::new(Vec::new()),
            commands_run: Mutex::new(Vec::new()),
            deleted: Mutex::new(0),
        });
        let st = state.clone();
        let acceptor = Acceptor::start(move |stream, stop| {
            let _ = session(&st, stream, &stop);
        })?;
        Ok(WinrmServer { state, acceptor })
    }

    pub fn addr(&self) -> SocketAddr {
        self.acceptor.addr
    }

    pub fn port(&self) -> u16 {
        self.acceptor.addr.port()
    }

    pub fn mode(&self) -> Mode {
        self.state.mode
    }

    /// The SHA-256 of the certificate (`None` over HTTP). Pin it with `Target::https(.., Some(fingerprint))`.
    pub fn fingerprint(&self) -> Option<Fingerprint> {
        self.state.cert.as_ref().map(Cert::fingerprint)
    }

    /// [`fingerprint`](Self::fingerprint) as `AB:CD:...`.
    pub fn fingerprint_text(&self) -> Option<String> {
        self.fingerprint().map(|f| tls::format(&f))
    }

    /// Script a command: when a command line contains `needle`, it does `output`. The first script (in the order
    /// they were added) that matches wins; a command no script matches prints `'<cmd>' is not recognized` on stderr
    /// and exits 1.
    pub fn on_command(&self, needle: &str, output: Output) {
        lock(&self.state.scripts).push((needle.to_owned(), output));
    }

    /// Every request, in order.
    pub fn requests(&self) -> Vec<Request> {
        lock(&self.state.requests).clone()
    }

    /// The `action`s of the requests, in order (`Create`, `Command`, `Receive`, ..., `Delete`).
    pub fn actions(&self) -> Vec<String> {
        lock(&self.state.requests).iter().map(|r| r.action.clone()).collect()
    }

    /// The command lines that were started, in order.
    pub fn commands(&self) -> Vec<String> {
        lock(&self.state.commands_run).clone()
    }

    /// Whether a `Signal` was received for some command.
    pub fn signalled(&self) -> bool {
        lock(&self.state.requests).iter().any(|r| r.action == "Signal")
    }

    /// How many shells were deleted.
    pub fn shells_deleted(&self) -> u32 {
        *lock(&self.state.deleted)
    }

    /// How many shells are open now (created, not deleted).
    pub fn shells_open(&self) -> usize {
        lock(&self.state.shells).len()
    }

    /// What was said: `C: <action> ...` for a request (a password is never logged) and `S: <status>` for an answer,
    /// plus `! tls`, `! 401`.
    pub fn log(&self) -> Vec<String> {
        lock(&self.state.log).clone()
    }

    /// Wait until a request with this action (`Delete`, ...) arrived, at most `timeout`.
    pub fn wait_for_action(&self, action: &str, timeout: Duration) -> bool {
        let end = Instant::now() + timeout;
        loop {
            if self.actions().iter().any(|a| a == action) {
                return true;
            }
            if Instant::now() >= end {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct Http {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

fn read_request(r: &mut Reader, never: &dyn Fn() -> bool) -> Option<Http> {
    let first = r.line(never)?;
    let mut parts = first.split_whitespace();
    let (method, path) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let (mut length, mut authorization) = (0usize, None);
    loop {
        let l = r.line(never)?;
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "authorization" => authorization = Some(v.trim().to_owned()),
                _ => {}
            }
        }
    }
    let body = if length > 0 { r.take(length, never)? } else { Vec::new() };
    Some(Http { method, path, authorization, body })
}

fn respond(conn: &mut Conn, status: u16, extra: &str, body: &str) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/soap+xml;charset=UTF-8\r\nContent-Length: {}\r\n{extra}Connection: Keep-Alive\r\n\r\n",
        body.len()
    );
    conn.write_all(head.as_bytes())?;
    conn.write_all(body.as_bytes())
}

fn session(st: &State, stream: TcpStream, stop: &AtomicBool) -> io::Result<()> {
    let conn = Conn::new(stream, st.cert.as_ref())?;
    let mut r = Reader::new(conn, stop);
    if st.cert.is_some() {
        lock(&st.log).push("! tls".into());
    }
    while let Some(req) = read_request(&mut r, &|| false) {
        let conn = &mut r.conn;
        if req.method != "POST" {
            respond(conn, 405, "", "")?;
            continue;
        }
        if !req.path.eq_ignore_ascii_case("/wsman") && !req.path.eq_ignore_ascii_case("/wsman/") {
            respond(conn, 404, "", "")?;
            continue;
        }
        if !authorized(st, req.authorization.as_deref()) {
            lock(&st.log).push("! 401".into());
            respond(conn, 401, "WWW-Authenticate: Basic realm=\"WSMAN\"\r\n", "")?;
            continue;
        }
        let xml = String::from_utf8_lossy(&req.body).into_owned();
        let action = inner(&xml, "Action").and_then(|a| a.rsplit('/').next().map(str::to_owned)).unwrap_or_default();
        let shell = selector(&xml);
        let detail = match action.as_str() {
            "Command" => inner(&xml, "Command").map(|c| unesc(&c)),
            "Signal" => inner(&xml, "Code").map(|c| unesc(&c)),
            _ => None,
        };
        lock(&st.log).push(format!("C: {action}{}", shell.as_deref().map(|s| format!(" shell={s}")).unwrap_or_default()));
        lock(&st.requests).push(Request { action: action.clone(), shell: shell.clone(), detail: detail.clone(), body: xml.clone() });
        let (status, body) = handle(st, &action, &xml, shell.as_deref(), detail.as_deref(), stop);
        lock(&st.log).push(format!("S: {status}"));
        respond(&mut r.conn, status, "", &body)?;
    }
    Ok(())
}

fn authorized(st: &State, header: Option<&str>) -> bool {
    let Some((user, password)) = &st.credentials else {
        return true;
    };
    let Some(v) = header.and_then(|h| h.strip_prefix("Basic ").or_else(|| h.strip_prefix("basic "))) else {
        return false;
    };
    b64_decode(v).is_some_and(|b| String::from_utf8_lossy(&b) == format!("{user}:{password}"))
}

fn envelope(body: &str) -> String {
    format!(
        "<s:Envelope xmlns:s=\"http://www.w3.org/2003/05/soap-envelope\" xmlns:rsp=\"http://schemas.microsoft.com/wbem/wsman/1/windows/shell/cmd\">\
<s:Header/><s:Body>{body}</s:Body></s:Envelope>"
    )
}

fn fault(code: u32, text: &str) -> String {
    envelope(&format!(
        "<s:Fault><s:Code><s:Value>s:Receiver</s:Value></s:Code><s:Reason><s:Text xml:lang=\"en-US\">{text}</s:Text></s:Reason>\
<s:Detail><f:WSManFault xmlns:f=\"http://schemas.microsoft.com/wbem/wsman/1/wsmanfault\" Code=\"{code}\"><f:Message>{text}</f:Message></f:WSManFault></s:Detail></s:Fault>"
    ))
}

/// WS-Man's "the wait ran out, nothing to say" code (what a real Receive sends).
const FAULT_TIMEOUT: u32 = 2_150_858_793;
const STATE: &str = "http://schemas.microsoft.com/wbem/wsman/1/windows/shell/CommandState";

fn handle(st: &State, action: &str, xml: &str, shell: Option<&str>, detail: Option<&str>, stop: &AtomicBool) -> (u16, String) {
    match action {
        "Create" => {
            let id = format!("SHELL-{}", unique().to_ascii_uppercase());
            lock(&st.shells).insert(id.clone(), Shell::default());
            (200, envelope(&format!("<rsp:Shell><rsp:ShellId>{id}</rsp:ShellId></rsp:Shell>")))
        }
        "Command" => {
            let Some(shell) = shell else { return (500, fault(1, "no shell")) };
            let line = detail.unwrap_or_default().to_owned();
            let script = lock(&st.scripts).iter().find(|(n, _)| line.contains(n.as_str())).map(|(_, o)| o.clone()).unwrap_or_else(|| {
                Output::new().stderr(format!("'{}' is not recognized as an internal or external command.\r\n", line)).exit(1)
            });
            lock(&st.commands_run).push(line);
            let id = format!("CMD-{}", unique().to_ascii_uppercase());
            match lock(&st.shells).get_mut(shell) {
                Some(s) => {
                    s.commands.insert(id.clone(), Cmd { script, started: Instant::now(), sent: false, signalled: false });
                    (200, envelope(&format!("<rsp:CommandResponse><rsp:CommandId>{id}</rsp:CommandId></rsp:CommandResponse>")))
                }
                None => (500, fault(2_150_858_843, "The shell was not found")),
            }
        }
        "Receive" => {
            let (Some(shell), Some(cmd)) = (shell, attr_of(xml, "DesiredStream", "CommandId")) else {
                return (500, fault(1, "no shell or command"));
            };
            let wait = inner(xml, "OperationTimeout")
                .and_then(|t| t.trim().trim_start_matches("PT").trim_end_matches('S').parse::<f64>().ok())
                .unwrap_or(3.0)
                .clamp(0.0, 30.0);
            let end = Instant::now() + Duration::from_secs_f64(wait);
            loop {
                let step = {
                    let mut shells = lock(&st.shells);
                    let Some(c) = shells.get_mut(shell).and_then(|s| s.commands.get_mut(&cmd)) else {
                        return (500, fault(2_150_858_843, "The command was not found"));
                    };
                    let done = c.signalled || (!c.script.hang && c.started.elapsed() >= c.script.delay);
                    if !c.sent || done {
                        let mut body = String::from("<rsp:ReceiveResponse>");
                        if !c.sent {
                            c.sent = true;
                            for (err, bytes) in &c.script.chunks {
                                body.push_str(&format!(
                                    "<rsp:Stream Name=\"{}\" CommandId=\"{cmd}\">{}</rsp:Stream>",
                                    if *err { "stderr" } else { "stdout" },
                                    b64_encode(bytes)
                                ));
                            }
                        }
                        if done {
                            let code = if c.signalled && c.script.hang { 1 } else { c.script.exit };
                            body.push_str(&format!(
                                "<rsp:CommandState CommandId=\"{cmd}\" State=\"{STATE}/Done\"><rsp:ExitCode>{code}</rsp:ExitCode></rsp:CommandState>"
                            ));
                        } else {
                            body.push_str(&format!("<rsp:CommandState CommandId=\"{cmd}\" State=\"{STATE}/Running\"/>"));
                        }
                        body.push_str("</rsp:ReceiveResponse>");
                        Some((200, envelope(&body)))
                    } else {
                        None
                    }
                };
                if let Some(reply) = step {
                    return reply;
                }
                if Instant::now() >= end || stop.load(Ordering::Relaxed) {
                    return (
                        500,
                        fault(FAULT_TIMEOUT, "The WS-Management service cannot complete the operation within the time specified"),
                    );
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        "Signal" => {
            if let (Some(shell), Some(cmd)) = (shell, attr_of(xml, "Signal", "CommandId")) {
                if let Some(c) = lock(&st.shells).get_mut(shell).and_then(|s| s.commands.get_mut(&cmd)) {
                    c.signalled = true;
                }
            }
            (200, envelope("<rsp:SignalResponse/>"))
        }
        "Delete" => {
            if let Some(shell) = shell {
                if lock(&st.shells).remove(shell).is_some() {
                    *lock(&st.deleted) += 1;
                }
            }
            (200, envelope(""))
        }
        _ => (500, fault(1, "unsupported action")),
    }
}

/// The text inside the first element whose local name is `name` (any prefix, no attributes needed).
fn inner(xml: &str, name: &str) -> Option<String> {
    let (_, body) = find(xml, name)?;
    Some(body.to_owned())
}

/// `(attributes, inner)` of the first element with this local name.
fn find<'a>(xml: &'a str, name: &str) -> Option<(&'a str, &'a str)> {
    let mut from = 0;
    while let Some(i) = xml[from..].find('<') {
        let start = from + i + 1;
        let rest = &xml[start..];
        let end = rest.find(['>', ' ', '/', '\t', '\r', '\n'])?;
        let tag = &rest[..end];
        if !tag.starts_with(['/', '?']) && tag.rsplit(':').next() == Some(name) {
            let close = rest.find('>')?;
            let attrs = &rest[end..close];
            if attrs.ends_with('/') {
                return Some((attrs, ""));
            }
            let body = &rest[close + 1..];
            let stop = body.find(&format!("</{tag}>")).unwrap_or(body.len());
            return Some((attrs, &body[..stop]));
        }
        from = start;
    }
    None
}

fn attr_of(xml: &str, element: &str, attr: &str) -> Option<String> {
    let (attrs, _) = find(xml, element)?;
    let key = format!("{attr}=\"");
    let i = attrs.find(&key)? + key.len();
    let j = attrs[i..].find('"')?;
    Some(unesc(&attrs[i..i + j]))
}

fn selector(xml: &str) -> Option<String> {
    let i = xml.find("Name=\"ShellId\">")? + "Name=\"ShellId\">".len();
    let j = xml[i..].find('<')?;
    Some(unesc(&xml[i..i + j]))
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}
