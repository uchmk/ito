//! The redirect decision, as a pure function.

use url::Url;

use crate::error::{Error, Redirect};
use crate::Method;

/// What to do with a response.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Hop {
    /// Not a redirect to follow (another status, or no `Location`): return it.
    Done,
    /// Ask `url` next.
    Follow { url: Url, method: Method, keep_body: bool, other_origin: bool },
}

pub(crate) fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn origin(u: &Url) -> (&str, Option<&str>, Option<u16>) {
    (u.scheme(), u.host_str(), u.port_or_known_default())
}

/// A URL that may be asked: http or https, a host, no user information, no fragment.
pub(crate) fn clean(mut u: Url) -> Option<Url> {
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none_or(str::is_empty) {
        return None;
    }
    if !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    u.set_fragment(None);
    Some(u)
}

/// Decide the next hop after `status` from `current`. `location` is the `Location` header, if any.
pub(crate) fn next_hop(method: Method, has_body: bool, status: u16, current: &Url, location: Option<&str>) -> Result<Hop, Error> {
    if !is_redirect(status) {
        return Ok(Hop::Done);
    }
    let Some(location) = location.map(str::trim).filter(|l| !l.is_empty()) else { return Ok(Hop::Done) };
    let Ok(joined) = current.join(location) else { return Err(Error::Redirect(Redirect::BadLocation)) };
    let Some(url) = clean(joined) else { return Err(Error::Redirect(Redirect::BadLocation)) };
    if current.scheme() == "https" && url.scheme() == "http" {
        return Err(Error::Redirect(Redirect::ToHttp));
    }
    let other_origin = origin(current) != origin(&url);
    let keep = matches!(status, 307 | 308);
    if keep && has_body && other_origin {
        return Err(Error::Redirect(Redirect::BodyElsewhere));
    }
    let method = if keep || method == Method::Head { method } else { Method::Get };
    Ok(Hop::Follow { url, method, keep_body: keep && has_body, other_origin })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn follow(m: Method, body: bool, status: u16, cur: &str, loc: &str) -> Result<Hop, Error> {
        next_hop(m, body, status, &u(cur), Some(loc))
    }

    #[test]
    fn get_after_301_302_303() {
        for s in [301, 302, 303] {
            let h = follow(Method::Post, true, s, "https://a.example/x", "/y").unwrap();
            assert_eq!(h, Hop::Follow { url: u("https://a.example/y"), method: Method::Get, keep_body: false, other_origin: false });
        }
        let h = follow(Method::Head, false, 301, "https://a.example/x", "y").unwrap();
        assert_eq!(h, Hop::Follow { url: u("https://a.example/y"), method: Method::Head, keep_body: false, other_origin: false });
    }

    #[test]
    fn same_method_and_body_after_307_308() {
        for s in [307, 308] {
            let h = follow(Method::Put, true, s, "https://a.example/x", "https://a.example/z?q=1").unwrap();
            assert_eq!(h, Hop::Follow { url: u("https://a.example/z?q=1"), method: Method::Put, keep_body: true, other_origin: false });
        }
    }

    #[test]
    fn body_is_not_sent_to_another_origin_on_307() {
        let e = follow(Method::Post, true, 307, "https://a.example/x", "https://b.example/").unwrap_err();
        assert_eq!(e, Error::Redirect(Redirect::BodyElsewhere));
        // Without a body it is fine.
        let h = follow(Method::Get, false, 308, "https://a.example/x", "https://b.example/");
        assert!(matches!(h, Ok(Hop::Follow { other_origin: true, .. })));
    }

    #[test]
    fn https_to_http_is_refused_but_not_the_reverse() {
        let e = follow(Method::Get, false, 302, "https://a.example/", "http://a.example/").unwrap_err();
        assert_eq!(e, Error::Redirect(Redirect::ToHttp));
        let h = follow(Method::Get, false, 302, "http://a.example/", "https://a.example/");
        assert!(matches!(h, Ok(Hop::Follow { other_origin: true, .. })));
    }

    #[test]
    fn origin_means_scheme_host_and_port() {
        let other = |cur, loc| matches!(follow(Method::Get, false, 302, cur, loc), Ok(Hop::Follow { other_origin: true, .. }));
        assert!(other("http://127.0.0.1:1/", "http://localhost:1/"));
        assert!(other("http://127.0.0.1:1/", "http://127.0.0.1:2/"));
        assert!(!other("https://a.example/", "https://a.example:443/p"));
        assert!(!other("http://a.example/", "http://a.example/p"));
    }

    #[test]
    fn no_location_or_other_status_is_returned() {
        let cur = u("https://a.example/");
        assert_eq!(next_hop(Method::Get, false, 302, &cur, None).unwrap(), Hop::Done);
        assert_eq!(next_hop(Method::Get, false, 302, &cur, Some("  ")).unwrap(), Hop::Done);
        assert_eq!(next_hop(Method::Get, false, 304, &cur, Some("/x")).unwrap(), Hop::Done);
        assert_eq!(next_hop(Method::Get, false, 300, &cur, Some("/x")).unwrap(), Hop::Done);
    }

    #[test]
    fn bad_locations() {
        for loc in ["ftp://a.example/", "https://user:pw@a.example/", "javascript:alert(1)", "http://"] {
            let e = follow(Method::Get, false, 302, "https://a.example/", loc).unwrap_err();
            assert_eq!(e, Error::Redirect(Redirect::BadLocation), "{loc}");
        }
    }

    #[test]
    fn fragment_is_dropped() {
        let h = follow(Method::Get, false, 302, "https://a.example/", "/p#frag").unwrap();
        assert!(matches!(h, Hop::Follow { url, .. } if url.fragment().is_none()));
    }
}
