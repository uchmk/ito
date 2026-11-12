//! A listener on the tailnet only, where the caller's identity comes from Tailscale (`whois`), not from a token.
//!
//! - **Opt-in:** set [`Config::tailnet`](crate::Config::tailnet). The listener binds only tailnet addresses
//!   (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`), plain HTTP (WireGuard encrypts). Any other address is refused at start.
//! - **Who:** for a request with no good bearer token, the peer is looked up with tailscaled's LocalAPI
//!   (`/localapi/v0/whois`). A tagged node is matched by its tags, any other by its login name. The map from login or
//!   tag to the `who` the handler sees is in [`Tailnet`]. No match, a failed lookup, or tailscaled being down is a 401.
//!   A bearer token still works here.
//! - **Browsers:** a request let in by whois is treated like a cookie: a change (not `GET`/`HEAD`/`OPTIONS`) must send
//!   `X-Ito-Csrf: 1`, and `Host` must be a tailnet name or address of this PC (DNS rebinding).
//! - [`LocalApi`] reaches tailscaled: a named pipe on Windows, a Unix socket elsewhere. Tests give their own [`Whois`].

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

/// Who Tailscale says is at the other end.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identity {
    /// The login name (`alice@example.com`); `None` for a tagged node, which has no user.
    pub login: Option<String>,
    /// The tags (`tag:koyomi-ha`); empty for a user's own device.
    pub tags: Vec<String>,
    /// The node's name in the tailnet (`laptop.tail1234.ts.net`), for the log or a person; not used to decide.
    pub node: String,
}

/// Looks up who is at `peer`. `Ok(None)`: Tailscale does not know it. `Err`: it could not ask (a 401 as well).
pub trait Whois: Send + Sync {
    fn whois(&self, peer: SocketAddr) -> io::Result<Option<Identity>>;

    /// This PC's tailnet addresses and names, to bind and to check `Host` against. The default cannot tell.
    fn status(&self) -> io::Result<Status> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "this Whois cannot give the status"))
    }
}

/// The tailnet listener's settings.
#[derive(Clone)]
pub struct Tailnet {
    /// Tailnet addresses to bind. Empty: this PC's own, from tailscaled's status (an error if it has none).
    pub addrs: Vec<IpAddr>,
    /// The port (`0`: the system picks, see [`Server::tailnet_port`](crate::Server::tailnet_port)).
    pub port: u16,
    /// Allowed login names → the `who` the handler sees (`("alice@example.com", "owner")`). Compared without case.
    pub logins: Vec<(String, String)>,
    /// Allowed tags → `who` (`("tag:koyomi-ha", "standby")`). A tagged node is judged by tags only.
    pub tags: Vec<(String, String)>,
    /// More names the `Host` header may carry (without the port). This PC's addresses and its MagicDNS name (from
    /// tailscaled, when it answers) are always allowed.
    pub hosts: Vec<String>,
    /// How to ask. Default [`LocalApi::default`].
    pub whois: Arc<dyn Whois>,
}

impl Default for Tailnet {
    fn default() -> Self {
        Tailnet {
            addrs: Vec::new(),
            port: 0,
            logins: Vec::new(),
            tags: Vec::new(),
            hosts: Vec::new(),
            whois: Arc::new(LocalApi::default()),
        }
    }
}

impl std::fmt::Debug for Tailnet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tailnet")
            .field("addrs", &self.addrs)
            .field("port", &self.port)
            .field("logins", &self.logins)
            .field("tags", &self.tags)
            .finish_non_exhaustive()
    }
}

impl Tailnet {
    pub fn new(port: u16) -> Self {
        Tailnet { port, ..Self::default() }
    }

    /// Let `login` in as `who`.
    pub fn login(mut self, login: impl Into<String>, who: impl Into<String>) -> Self {
        self.logins.push((login.into(), who.into()));
        self
    }

    /// Let nodes tagged `tag` in as `who`.
    pub fn tag(mut self, tag: impl Into<String>, who: impl Into<String>) -> Self {
        self.tags.push((tag.into(), who.into()));
        self
    }

    /// The `who` for `id`, or `None` when it is not on the list.
    pub fn who(&self, id: &Identity) -> Option<String> {
        who_of(&self.logins, &self.tags, id)
    }
}

/// The `who` for `id` under these logins and tags: a node with tags is judged by tags only, the first match in list
/// order wins, names are compared without case.
pub(crate) fn who_of(logins: &[(String, String)], tags: &[(String, String)], id: &Identity) -> Option<String> {
    if !id.tags.is_empty() {
        return tags.iter().find(|(t, _)| id.tags.iter().any(|x| x.eq_ignore_ascii_case(t))).map(|(_, w)| w.clone());
    }
    let login = id.login.as_deref()?;
    logins.iter().find(|(l, _)| l.eq_ignore_ascii_case(login)).map(|(_, w)| w.clone())
}

/// Is `ip` in the tailnet's ranges: `100.64.0.0/10` (CGNAT) or `fd7a:115c:a1e0::/48`?
pub fn is_tailnet_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64,
        IpAddr::V6(v6) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

/// Whether `host` (the `Host` header: `name`, `name:port`, `1.2.3.4:80`, `[fd7a::1]:80`) is one of `allowed`.
pub(crate) fn host_allowed(host: &str, allowed: &[String]) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split_once(']').map(|(a, _)| a).unwrap_or("")
    } else {
        host.rsplit_once(':').filter(|(_, p)| p.bytes().all(|b| b.is_ascii_digit())).map(|(h, _)| h).unwrap_or(host)
    };
    let name = name.trim_end_matches('.');
    !name.is_empty() && allowed.iter().any(|a| a.trim_end_matches('.').eq_ignore_ascii_case(name))
}

/// What tailscaled says about itself.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// This PC's tailnet addresses.
    pub addrs: Vec<IpAddr>,
    /// Its MagicDNS name without the final dot, and the short host name.
    pub names: Vec<String>,
}

/// Parse tailscaled's `/status` JSON.
pub fn parse_status(json: &[u8]) -> Option<Status> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let addrs = v["TailscaleIPs"].as_array()?.iter().filter_map(|a| a.as_str()?.parse::<IpAddr>().ok()).collect();
    let mut names = Vec::new();
    if let Some(n) = v["Self"]["DNSName"].as_str() {
        let n = n.trim_end_matches('.');
        if !n.is_empty() {
            names.push(n.to_string());
            if let Some((short, _)) = n.split_once('.') {
                names.push(short.to_string());
            }
        }
    }
    Some(Status { addrs, names })
}

/// Parse tailscaled's `/whois` JSON. `None` when it names no node.
pub fn parse_whois(json: &[u8]) -> Option<Identity> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let node = v.get("Node").filter(|n| n.is_object())?;
    let tags: Vec<String> =
        node["Tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default();
    let login = v["UserProfile"]["LoginName"].as_str().filter(|l| !l.is_empty() && *l != "tagged-devices").map(String::from);
    let name = node["Name"].as_str().unwrap_or("").trim_end_matches('.').to_string();
    Some(Identity { login, tags, node: name })
}

/// tailscaled's LocalAPI over its local socket (a named pipe on Windows).
#[derive(Debug, Clone)]
pub struct LocalApi {
    path: Option<std::path::PathBuf>,
}

impl Default for LocalApi {
    /// The usual place for this OS.
    fn default() -> Self {
        LocalApi { path: None }
    }
}

impl LocalApi {
    /// At an explicit socket path (or pipe name on Windows).
    pub fn at(path: impl Into<std::path::PathBuf>) -> Self {
        LocalApi { path: Some(path.into()) }
    }

    /// `GET target`, the body of a 200. The request is HTTP/1.0 so the answer has no chunks.
    pub fn get(&self, target: &str) -> io::Result<Vec<u8>> {
        let req = format!("GET {target} HTTP/1.0\r\nHost: local-tailscaled.sock\r\nConnection: close\r\n\r\n");
        let raw = self.roundtrip(req.as_bytes())?;
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no head"))?;
        let head = String::from_utf8_lossy(&raw[..split]);
        let status: u16 = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        match status {
            200 => Ok(raw[split + 4..].to_vec()),
            404 => Err(io::Error::new(io::ErrorKind::NotFound, "unknown to tailscaled")),
            403 => Err(io::Error::new(io::ErrorKind::PermissionDenied, "tailscaled refused this account (LocalAPI)")),
            s => {
                let body = String::from_utf8_lossy(&raw[split + 4..]);
                Err(io::Error::other(format!("tailscaled answered {s}: {}", body.trim().chars().take(200).collect::<String>())))
            }
        }
    }

    /// This PC's tailnet addresses and names.
    pub fn status(&self) -> io::Result<Status> {
        let body = self.get("/localapi/v0/status?peers=false")?;
        parse_status(&body).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status"))
    }

    #[cfg(windows)]
    fn roundtrip(&self, req: &[u8]) -> io::Result<Vec<u8>> {
        const PIPE: &str = r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled";
        let path = self.path.clone().unwrap_or_else(|| PIPE.into());
        use std::os::windows::fs::OpenOptionsExt;
        // The pipe has few instances: one of ours at a time, and a short wait when another program holds them all
        // (ERROR_PIPE_BUSY, 231).
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let mut tries = 0;
        let mut f = loop {
            // tailscaled impersonates the caller to learn who is asking, which needs SECURITY_IMPERSONATION (0x20000).
            match std::fs::OpenOptions::new().read(true).write(true).security_qos_flags(0x0002_0000).open(&path) {
                Err(e) if e.raw_os_error() == Some(231) && tries < 40 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(25));
                }
                other => break other?,
            }
        };
        f.write_all(req)?;
        let mut out = Vec::new();
        f.read_to_end(&mut out)?;
        Ok(out)
    }

    #[cfg(unix)]
    fn roundtrip(&self, req: &[u8]) -> io::Result<Vec<u8>> {
        use std::os::unix::net::UnixStream;
        let candidates: Vec<std::path::PathBuf> = match &self.path {
            Some(p) => vec![p.clone()],
            None => ["/var/run/tailscale/tailscaled.sock", "/run/tailscale/tailscaled.sock", "/var/run/tailscaled.socket"]
                .iter()
                .map(Into::into)
                .collect(),
        };
        let mut last = io::Error::new(io::ErrorKind::NotFound, "tailscaled has no socket here");
        for p in candidates {
            match UnixStream::connect(&p) {
                Ok(mut s) => {
                    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
                    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
                    s.write_all(req)?;
                    let mut out = Vec::new();
                    s.read_to_end(&mut out)?;
                    return Ok(out);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    #[cfg(not(any(windows, unix)))]
    fn roundtrip(&self, _req: &[u8]) -> io::Result<Vec<u8>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "tailscaled's LocalAPI is not reachable on this OS"))
    }
}

impl Whois for LocalApi {
    fn status(&self) -> io::Result<Status> {
        LocalApi::status(self)
    }

    fn whois(&self, peer: SocketAddr) -> io::Result<Option<Identity>> {
        let addr = peer.to_string().replace(':', "%3A").replace(['[', ']'], "");
        match self.get(&format!("/localapi/v0/whois?addr={addr}")) {
            Ok(body) => Ok(parse_whois(&body)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// `whois` with a deadline: the lookup runs on its own thread (a pipe read cannot time out), and a stuck tailscaled is
/// an `Err`, not a stuck connection.
pub(crate) fn whois_within(w: &Arc<dyn Whois>, peer: SocketAddr, limit: Duration) -> io::Result<Option<Identity>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let w = w.clone();
    std::thread::spawn(move || {
        let _ = tx.send(w.whois(peer));
    });
    rx.recv_timeout(limit).unwrap_or_else(|_| Err(io::Error::new(io::ErrorKind::TimedOut, "tailscaled did not answer")))
}

/// The addresses this listener may bind: the ones given (all must be in the tailnet), else this PC's.
pub(crate) fn bind_addrs(t: &Tailnet, status: &io::Result<Status>) -> io::Result<Vec<IpAddr>> {
    let addrs = if t.addrs.is_empty() {
        match status {
            Ok(s) if !s.addrs.is_empty() => s.addrs.clone(),
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "this PC has no tailnet address")),
            Err(e) => return Err(io::Error::new(e.kind(), format!("cannot learn the tailnet address: {e}"))),
        }
    } else {
        t.addrs.clone()
    };
    if let Some(bad) = addrs.iter().find(|a| !is_tailnet_ip(**a)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("{bad} is not a tailnet address")));
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        for ok in ["100.64.0.1", "100.104.162.51", "100.127.255.255", "fd7a:115c:a1e0::1", "fd7a:115c:a1e0:ab12::9"] {
            assert!(is_tailnet_ip(ok.parse().unwrap()), "{ok}");
        }
        for no in ["100.63.255.255", "100.128.0.0", "0.0.0.0", "127.0.0.1", "192.168.1.1", "::1", "fd7a:115c:a1e1::1", "fd00::1"] {
            assert!(!is_tailnet_ip(no.parse().unwrap()), "{no}");
        }
    }

    #[test]
    fn hosts() {
        let a = vec!["pc.tail1.ts.net".to_string(), "100.1.2.3".to_string(), "fd7a:115c:a1e0::1".to_string()];
        for ok in
            ["pc.tail1.ts.net", "PC.tail1.ts.net:8443", "pc.tail1.ts.net.", "100.1.2.3:80", "[fd7a:115c:a1e0::1]:80", "[fd7a:115c:a1e0::1]"]
        {
            assert!(host_allowed(ok, &a), "{ok}");
        }
        for no in ["evil.example", "pc.tail1.ts.net.evil.example", "localhost", "", "127.0.0.1:80", "100.1.2.4"] {
            assert!(!host_allowed(no, &a), "{no}");
        }
    }

    #[test]
    fn who_by_login_or_tag() {
        let t = Tailnet::new(0).login("Alice@Example.com", "owner").tag("tag:koyomi-ha", "standby");
        let user = Identity { login: Some("alice@example.com".into()), ..Default::default() };
        assert_eq!(t.who(&user).as_deref(), Some("owner"));
        let tagged = Identity { login: None, tags: vec!["tag:koyomi-ha".into()], node: String::new() };
        assert_eq!(t.who(&tagged).as_deref(), Some("standby"));
        // A tagged node is judged by its tags even if some login were listed.
        let both = Identity { login: Some("alice@example.com".into()), tags: vec!["tag:other".into()], node: String::new() };
        assert_eq!(t.who(&both), None);
        let stranger = Identity { login: Some("bob@example.com".into()), ..Default::default() };
        assert_eq!(t.who(&stranger), None);
        assert_eq!(t.who(&Identity::default()), None);
    }

    #[test]
    fn parses_whois_and_status() {
        let user = br#"{"Node":{"Name":"laptop.tail1.ts.net.","Tags":null},"UserProfile":{"LoginName":"alice@example.com"}}"#;
        let id = parse_whois(user).unwrap();
        assert_eq!((id.login.as_deref(), id.tags.len(), id.node.as_str()), (Some("alice@example.com"), 0, "laptop.tail1.ts.net"));
        let tagged = br#"{"Node":{"Name":"srv.tail1.ts.net.","Tags":["tag:koyomi-ha"]},"UserProfile":{"LoginName":"tagged-devices"}}"#;
        let id = parse_whois(tagged).unwrap();
        assert_eq!((id.login, id.tags), (None, vec!["tag:koyomi-ha".to_string()]));
        assert!(parse_whois(b"{}").is_none() && parse_whois(b"nope").is_none());
        let s = parse_status(br#"{"TailscaleIPs":["100.1.2.3","fd7a:115c:a1e0::1"],"Self":{"DNSName":"pc.tail1.ts.net."}}"#).unwrap();
        assert_eq!(s.addrs.len(), 2);
        assert_eq!(s.names, ["pc.tail1.ts.net", "pc"]);
    }

    #[test]
    fn refuses_addresses_outside_the_tailnet() {
        let ok: io::Result<Status> = Ok(Status { addrs: vec!["100.64.1.2".parse().unwrap()], names: vec![] });
        let mut t = Tailnet::new(0);
        assert_eq!(bind_addrs(&t, &ok).unwrap(), ["100.64.1.2".parse::<IpAddr>().unwrap()]);
        t.addrs = vec!["0.0.0.0".parse().unwrap()];
        assert_eq!(bind_addrs(&t, &ok).unwrap_err().kind(), io::ErrorKind::InvalidInput);
        t.addrs = vec!["192.168.0.5".parse().unwrap()];
        assert!(bind_addrs(&t, &ok).is_err());
        t.addrs.clear();
        assert!(bind_addrs(&t, &Err(io::Error::other("down"))).is_err());
    }

    /// Against the real tailscaled, when this PC has one: it knows its own address, and an unknown one is `None`.
    #[test]
    fn the_real_local_api_answers() {
        let api = LocalApi::default();
        let st = match api.status() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no tailscaled: {e}");
                return;
            }
        };
        let me = SocketAddr::new(st.addrs[0], 4000);
        let id = api.whois(me).unwrap().expect("tailscaled knows its own node");
        eprintln!("whois of self: {id:?}");
        assert!(id.login.is_some() || !id.tags.is_empty());
        assert_eq!(api.whois("100.64.0.9:1".parse().unwrap()).unwrap(), None);
    }
}
