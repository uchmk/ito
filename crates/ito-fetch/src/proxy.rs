//! Which proxy a hop uses, from the environment variables, as pure functions over a map.

use std::collections::HashMap;
use std::net::IpAddr;

use url::Url;

const NAMES: [&str; 8] = ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy", "NO_PROXY", "no_proxy"];

/// The variables that matter, read once from the process environment.
pub(crate) fn capture() -> HashMap<String, String> {
    std::env::vars().filter(|(k, _)| NAMES.contains(&k.as_str())).collect()
}

fn get<'a>(env: &'a HashMap<String, String>, names: &[&str]) -> Option<&'a str> {
    names.iter().find_map(|n| env.get(*n).map(|v| v.trim()).filter(|v| !v.is_empty()))
}

/// Does one `NO_PROXY` entry cover `host:port`? `*` covers all; `example.com` covers the host and its subdomains;
/// `.example.com` and `*.example.com` cover only the subdomains; `host:port` also needs the port.
fn covers(entry: &str, host: &str, port: Option<u16>) -> bool {
    let entry = entry.trim().to_ascii_lowercase();
    if entry.is_empty() {
        return false;
    }
    if entry == "*" {
        return true;
    }
    let mut name = entry.as_str();
    if let Some(rest) = entry.strip_prefix('[') {
        // `[addr]` or `[addr]:port`
        let Some((addr, tail)) = rest.split_once(']') else { return false };
        if !tail.is_empty() {
            let Some(p) = tail.strip_prefix(':').filter(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit())) else {
                return false;
            };
            if p.parse::<u16>().ok() != port {
                return false;
            }
        }
        name = addr;
    } else if let Some((n, p)) = entry.rsplit_once(':') {
        if !n.contains(':') && !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()) {
            if p.parse::<u16>().ok() != port {
                return false;
            }
            name = n;
        }
    }
    if let (Ok(a), Ok(b)) = (name.parse::<IpAddr>(), host.parse::<IpAddr>()) {
        return a == b;
    }
    let (suffix_only, bare) = match name.strip_prefix("*.").or_else(|| name.strip_prefix('.')) {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let host = host.to_ascii_lowercase();
    (!suffix_only && host == bare) || host.ends_with(&format!(".{bare}"))
}

/// The proxy URL text for a hop to `url`, or `None` for a direct connection (no variable, or `NO_PROXY` covers it).
///
/// https uses `HTTPS_PROXY` then `ALL_PROXY`; http uses `HTTP_PROXY` then `ALL_PROXY`. The lower-case names work too.
pub(crate) fn choose(env: &HashMap<String, String>, url: &Url) -> Option<String> {
    let names: [&str; 4] = if url.scheme() == "https" {
        ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
    } else {
        ["HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
    };
    let proxy = get(env, &names)?;
    let host = url.host_str()?.trim_matches(['[', ']']);
    if let Some(list) = get(env, &["NO_PROXY", "no_proxy"]) {
        if list.split(',').any(|e| covers(e, host, url.port_or_known_default())) {
            return None;
        }
    }
    Some(proxy.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    fn pick(e: &HashMap<String, String>, url: &str) -> Option<String> {
        choose(e, &Url::parse(url).unwrap())
    }

    #[test]
    fn scheme_picks_its_own_variable() {
        let e = env(&[("HTTPS_PROXY", "http://s:1"), ("HTTP_PROXY", "http://p:2")]);
        assert_eq!(pick(&e, "https://x.example/").as_deref(), Some("http://s:1"));
        assert_eq!(pick(&e, "http://x.example/").as_deref(), Some("http://p:2"));
    }

    #[test]
    fn https_does_not_use_the_http_variable() {
        let e = env(&[("HTTP_PROXY", "http://p:2")]);
        assert_eq!(pick(&e, "https://x.example/"), None);
        let e = env(&[("HTTPS_PROXY", "http://s:1")]);
        assert_eq!(pick(&e, "http://x.example/"), None);
    }

    #[test]
    fn all_proxy_is_the_fallback_and_lower_case_works() {
        let e = env(&[("all_proxy", "http://a:3")]);
        assert_eq!(pick(&e, "https://x.example/").as_deref(), Some("http://a:3"));
        assert_eq!(pick(&e, "http://x.example/").as_deref(), Some("http://a:3"));
        let e = env(&[("https_proxy", "http://s:1"), ("ALL_PROXY", "http://a:3")]);
        assert_eq!(pick(&e, "https://x.example/").as_deref(), Some("http://s:1"));
        let e = env(&[("HTTPS_PROXY", " "), ("ALL_PROXY", "http://a:3")]);
        assert_eq!(pick(&e, "https://x.example/").as_deref(), Some("http://a:3"));
    }

    #[test]
    fn no_proxy() {
        let e = env(&[("HTTPS_PROXY", "http://s:1"), ("NO_PROXY", "intra.example, .corp.example,*.lan,localhost:8443,10.0.0.1")]);
        assert_eq!(pick(&e, "https://intra.example/"), None);
        assert_eq!(pick(&e, "https://a.intra.example/"), None);
        assert_eq!(pick(&e, "https://a.corp.example/"), None);
        assert!(pick(&e, "https://corp.example/").is_some());
        assert_eq!(pick(&e, "https://x.lan/"), None);
        assert_eq!(pick(&e, "https://localhost:8443/"), None);
        assert!(pick(&e, "https://localhost/").is_some());
        assert_eq!(pick(&e, "https://10.0.0.1/"), None);
        assert!(pick(&e, "https://notintra.example/").is_some());
        let e = env(&[("HTTPS_PROXY", "http://s:1"), ("no_proxy", "*")]);
        assert_eq!(pick(&e, "https://x.example/"), None);
    }

    #[test]
    fn no_proxy_ipv6_and_v4_literals() {
        let with = |list: &str| env(&[("HTTPS_PROXY", "http://s:1"), ("NO_PROXY", list)]);
        let e = with("[::1]:8000");
        assert_eq!(pick(&e, "https://[::1]:8000/"), None);
        assert!(pick(&e, "https://[::1]:9000/").is_some());
        assert_eq!(pick(&with("[::1]"), "https://[::1]:8000/"), None);
        assert_eq!(pick(&with("::1"), "https://[::1]/"), None);
        assert_eq!(pick(&with("::0:1"), "https://[::1]/"), None);
        assert_eq!(pick(&with("[::0:1]"), "https://[::1]/"), None);
        assert!(pick(&with("[::2]"), "https://[::1]/").is_some());
        let e = with("127.0.0.1:8000");
        assert_eq!(pick(&e, "https://127.0.0.1:8000/"), None);
        assert!(pick(&e, "https://127.0.0.1:9000/").is_some());
    }

    #[test]
    fn nothing_set_is_direct() {
        assert_eq!(pick(&env(&[]), "https://x.example/"), None);
    }
}
