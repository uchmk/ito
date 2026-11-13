//! Local tests against `ito-testserver` and a small scripted listener. Everything talks to 127.0.0.1 with `Proxy::None`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ito_fetch::{get, request, Body, Error, Header, Method, Options, Over, Proxy, Redirect, Reply, Request};
use ito_testserver::http::{HttpServer, Reply as Route};

fn opts() -> Options {
    Options { proxy: Proxy::None, deadline: Duration::from_secs(10), ..Options::default() }
}

fn no_stop() -> AtomicBool {
    AtomicBool::new(false)
}

fn server() -> HttpServer {
    HttpServer::start().expect("server")
}

fn fetch(url: &str) -> Result<Reply, Error> {
    get(url, &opts(), &no_stop())
}

/// A listener that answers one connection: read the request head, then run `script` on the stream.
fn raw(script: impl FnOnce(&mut TcpStream) + Send + 'static) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        let Ok((mut s, _)) = l.accept() else { return };
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && s.read(&mut b).is_ok_and(|n| n == 1) {
            head.push(b[0]);
        }
        script(&mut s);
    });
    url
}

#[test]
fn statuses_come_back_as_replies() {
    let s = server();
    s.route("GET", "/ok", Route::new(200).header("Content-Type", "text/plain; charset=utf-8").body("hello"));
    s.route("GET", "/missing", Route::new(404).body("nope"));
    s.route("GET", "/boom", Route::new(500).body("oops"));
    let r = fetch(&format!("{}/ok", s.url())).unwrap();
    assert!(r.success() && r.status == 200);
    assert_eq!(r.text(), Some("hello"));
    assert_eq!(r.content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert!(r.url.ends_with("/ok") && !r.cut);
    let r = fetch(&format!("{}/missing", s.url())).unwrap();
    assert_eq!((r.status, r.success(), r.text()), (404, false, Some("nope")));
    let r = fetch(&format!("{}/boom", s.url())).unwrap();
    assert_eq!((r.status, r.text()), (500, Some("oops")));
    let r = fetch(&format!("{}/unrouted", s.url())).unwrap();
    assert_eq!(r.status, 404);
}

#[test]
fn every_method_reaches_the_server_with_its_headers_and_body() {
    let s = server();
    for (m, name, has_body) in [
        (Method::Get, "GET", false),
        (Method::Head, "HEAD", false),
        (Method::Post, "POST", true),
        (Method::Put, "PUT", true),
        (Method::Patch, "PATCH", true),
        (Method::Delete, "DELETE", true),
    ] {
        s.route(name, "/m", Route::new(200).body("done"));
        let req = Request {
            method: m,
            url: format!("{}/m?x=1", s.url()),
            headers: vec![Header::new("X-One", "1"), Header::secret("Authorization", "Bearer abc")],
            body: has_body.then(|| Body { bytes: "{\"a\":1}".into(), content_type: "application/json".into() }),
        };
        let r = request(&req, &opts(), &no_stop()).unwrap();
        assert_eq!(r.status, 200, "{name}");
        assert_eq!(r.body, if m == Method::Head { Vec::new() } else { b"done".to_vec() }, "{name}");
        let got = s.requests().into_iter().rev().find(|q| q.method == name).expect(name);
        assert_eq!((got.path.as_str(), got.query.as_str()), ("/m", "x=1"));
        assert_eq!(got.header("X-One"), Some("1"));
        assert_eq!(got.header("Authorization"), Some("Bearer abc"));
        assert!(got.header("User-Agent").unwrap().starts_with("ito-fetch/"));
        assert!(got.header("Accept-Encoding").is_none());
        if has_body {
            assert_eq!(got.body_text(), "{\"a\":1}", "{name}");
            assert_eq!(got.header("Content-Type"), Some("application/json"));
            assert_eq!(got.header("Content-Length"), Some("7"));
        } else {
            assert!(got.body.is_empty());
        }
    }
}

#[test]
fn a_302_becomes_a_get_without_the_body() {
    let s = server();
    s.route("POST", "/start", Route::new(302).header("Location", "/final"));
    s.route("GET", "/final", Route::new(200).body("landed"));
    let req = Request {
        method: Method::Post,
        url: format!("{}/start", s.url()),
        headers: vec![],
        body: Some(Body { bytes: b"data".to_vec(), content_type: "text/plain".into() }),
    };
    let r = request(&req, &opts(), &no_stop()).unwrap();
    assert_eq!((r.status, r.text()), (200, Some("landed")));
    assert!(r.url.ends_with("/final"));
    let last = s.requests().pop().unwrap();
    assert_eq!(last.method, "GET");
    assert!(last.body.is_empty() && last.header("Content-Type").is_none());
}

#[test]
fn a_dropped_body_takes_the_callers_content_headers_with_it() {
    let s = server();
    s.route("POST", "/start", Route::new(302).header("Location", "/final"));
    s.route("GET", "/final", Route::new(200));
    let req = Request {
        method: Method::Post,
        url: format!("{}/start", s.url()),
        headers: vec![
            Header::new("Content-Type", "application/custom"),
            Header::new("content-encoding", "identity"),
            Header::new("Content-Language", "ja"),
            Header::new("CONTENT-LOCATION", "/somewhere"),
            Header::new("X-Keep", "1"),
        ],
        body: Some(Body { bytes: b"data".to_vec(), content_type: String::new() }),
    };
    request(&req, &opts(), &no_stop()).unwrap();
    let all = s.requests();
    assert_eq!(all[0].header("Content-Type"), Some("application/custom"));
    let last = &all[1];
    assert_eq!(last.method, "GET");
    for gone in ["Content-Type", "Content-Encoding", "Content-Language", "Content-Location"] {
        assert!(last.header(gone).is_none(), "{gone}");
    }
    assert_eq!(last.header("X-Keep"), Some("1"));
}

#[test]
fn an_empty_body_content_type_keeps_the_callers() {
    let s = server();
    s.route("POST", "/p", Route::new(200));
    let req = Request {
        method: Method::Post,
        url: format!("{}/p", s.url()),
        headers: vec![Header::new("Content-Type", "application/custom")],
        body: Some(Body { bytes: b"data".to_vec(), content_type: String::new() }),
    };
    request(&req, &opts(), &no_stop()).unwrap();
    assert_eq!(s.requests()[0].header("Content-Type"), Some("application/custom"));
    let req = Request { body: Some(Body { bytes: b"data".to_vec(), content_type: "text/plain".into() }), ..req };
    request(&req, &opts(), &no_stop()).unwrap();
    assert_eq!(s.requests()[1].header("Content-Type"), Some("text/plain"));
}

#[test]
fn a_307_keeps_method_and_body() {
    let s = server();
    s.route("PUT", "/start", Route::new(307).header("Location", "/again"));
    s.route("PUT", "/again", Route::new(200).body("ok"));
    let req = Request {
        method: Method::Put,
        url: format!("{}/start", s.url()),
        headers: vec![],
        body: Some(Body { bytes: b"keep me".to_vec(), content_type: "text/plain".into() }),
    };
    let r = request(&req, &opts(), &no_stop()).unwrap();
    assert_eq!(r.status, 200);
    let last = s.requests().pop().unwrap();
    assert_eq!((last.method.as_str(), last.path.as_str(), last.body_text().as_str()), ("PUT", "/again", "keep me"));
    assert_eq!(last.header("Content-Type"), Some("text/plain"));
}

#[test]
fn a_307_with_a_body_to_another_origin_is_not_followed() {
    let (a, b) = (server(), server());
    a.route("POST", "/x", Route::new(307).header("Location", &format!("{}/y", b.url())));
    let req = Request {
        method: Method::Post,
        url: format!("{}/x", a.url()),
        headers: vec![],
        body: Some(Body { bytes: b"secret body".to_vec(), content_type: "text/plain".into() }),
    };
    let e = request(&req, &opts(), &no_stop()).unwrap_err();
    assert_eq!(e, Error::Redirect(Redirect::BodyElsewhere));
    assert!(b.requests().is_empty());
}

#[test]
fn secret_headers_are_dropped_on_another_origin() {
    let (a, b) = (server(), server());
    a.route("GET", "/go", Route::new(302).header("Location", &format!("{}/there", b.url())));
    b.route("GET", "/there", Route::new(200));
    let req = Request {
        method: Method::Get,
        url: format!("{}/go", a.url()),
        headers: vec![
            Header::new("X-Plain", "p"),
            Header::secret("X-Token", "t"),
            Header::new("Authorization", "Bearer z"),
            Header::new("cookie", "c=1"),
        ],
        body: None,
    };
    request(&req, &opts(), &no_stop()).unwrap();
    let first = &a.requests()[0];
    assert!(first.header("X-Token").is_some() && first.header("Authorization").is_some() && first.header("Cookie").is_some());
    let second = &b.requests()[0];
    assert_eq!(second.header("X-Plain"), Some("p"));
    for dropped in ["X-Token", "Authorization", "Cookie"] {
        assert!(second.header(dropped).is_none(), "{dropped}");
    }
}

#[test]
fn secret_headers_are_dropped_from_127_0_0_1_to_localhost() {
    let s = server();
    let port = s.port();
    s.route("GET", "/go", Route::new(302).header("Location", &format!("http://localhost:{port}/there")));
    s.route("GET", "/there", Route::new(200));
    let req = Request { method: Method::Get, url: format!("{}/go", s.url()), headers: vec![Header::secret("X-Token", "t")], body: None };
    request(&req, &opts(), &no_stop()).unwrap();
    let all = s.requests();
    assert_eq!(all.len(), 2);
    assert!(all[0].header("X-Token").is_some() && all[1].header("X-Token").is_none());
}

#[test]
fn a_dropped_header_stays_dropped_when_the_chain_returns() {
    let (a, b) = (server(), server());
    a.route("GET", "/one", Route::new(302).header("Location", &format!("{}/two", b.url())));
    b.route("GET", "/two", Route::new(302).header("Location", &format!("{}/three", a.url())));
    a.route("GET", "/three", Route::new(200));
    let req = Request { method: Method::Get, url: format!("{}/one", a.url()), headers: vec![Header::secret("X-Token", "t")], body: None };
    request(&req, &opts(), &no_stop()).unwrap();
    let on_a = a.requests();
    assert_eq!(on_a.len(), 2);
    assert!(on_a[0].header("X-Token").is_some());
    assert!(on_a[1].header("X-Token").is_none());
}

#[test]
fn redirect_limits_and_zero() {
    let s = server();
    s.route("GET", "/loop", Route::new(302).header("Location", "/loop"));
    let o = Options { max_redirects: 3, ..opts() };
    assert_eq!(get(&format!("{}/loop", s.url()), &o, &no_stop()).unwrap_err(), Error::Redirect(Redirect::TooMany));
    assert_eq!(s.requests().len(), 4);
    let o = Options { max_redirects: 0, ..opts() };
    let r = get(&format!("{}/loop", s.url()), &o, &no_stop()).unwrap();
    assert_eq!(r.status, 302);
    assert!(r.headers.iter().any(|(n, v)| n.eq_ignore_ascii_case("location") && v == "/loop"));
}

#[test]
fn a_redirect_without_location_is_a_reply() {
    let s = server();
    s.route("GET", "/x", Route::new(302).body("moved"));
    let r = fetch(&format!("{}/x", s.url())).unwrap();
    assert_eq!((r.status, r.text()), (302, Some("moved")));
}

#[test]
fn a_location_that_is_not_text_is_a_bad_location() {
    let head: &'static [u8] = b"HTTP/1.1 302 Found\r\nLocation: /\xFF\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let url = raw(move |s| {
        let _ = s.write_all(head);
    });
    assert_eq!(fetch(&url).unwrap_err(), Error::Redirect(Redirect::BadLocation));
    // With redirects off the 302 is returned as it is.
    let url = raw(move |s| {
        let _ = s.write_all(head);
    });
    let o = Options { max_redirects: 0, ..opts() };
    assert_eq!(get(&url, &o, &no_stop()).unwrap().status, 302);
}

#[test]
fn https_is_never_left_for_http() {
    // The refusal needs an https hop; the pure rule is unit tested in `redirect`. Here only the http side.
    let s = server();
    s.route("GET", "/x", Route::new(302).header("Location", "ftp://example.test/f"));
    assert_eq!(fetch(&format!("{}/x", s.url())).unwrap_err(), Error::Redirect(Redirect::BadLocation));
}

#[test]
fn max_reply_fail_cut_exact_and_zero() {
    let s = server();
    s.route("GET", "/ten", Route::new(200).body("0123456789"));
    let url = format!("{}/ten", s.url());
    let with = |max, over| Options { max_reply: max, over, ..opts() };
    assert_eq!(get(&url, &with(4, Over::Fail), &no_stop()).unwrap_err(), Error::TooLarge { limit: 4 });
    let r = get(&url, &with(4, Over::Cut), &no_stop()).unwrap();
    assert_eq!((r.body.as_slice(), r.cut), (&b"0123"[..], true));
    let r = get(&url, &with(10, Over::Fail), &no_stop()).unwrap();
    assert_eq!((r.body.len(), r.cut), (10, false));
    let r = get(&url, &with(10, Over::Cut), &no_stop()).unwrap();
    assert_eq!((r.body.len(), r.cut), (10, false));
    assert!(matches!(get(&url, &with(9, Over::Fail), &no_stop()), Err(Error::TooLarge { limit: 9 })));
    let r = get(&url, &with(0, Over::Cut), &no_stop()).unwrap();
    assert_eq!((r.body.len(), r.cut), (0, true));
    assert!(matches!(get(&url, &with(0, Over::Fail), &no_stop()), Err(Error::TooLarge { limit: 0 })));
    s.route("GET", "/empty", Route::new(200));
    let r = get(&format!("{}/empty", s.url()), &with(0, Over::Fail), &no_stop()).unwrap();
    assert!(r.body.is_empty() && !r.cut);
}

#[test]
fn a_big_content_length_fails_before_reading() {
    // Headers only: a reader would wait for the (never sent) body until the deadline.
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000000\r\n\r\n");
        std::thread::sleep(Duration::from_secs(3));
    });
    let o = Options { max_reply: 1000, ..opts() };
    let t = Instant::now();
    assert_eq!(get(&url, &o, &no_stop()).unwrap_err(), Error::TooLarge { limit: 1000 });
    assert!(t.elapsed() < Duration::from_secs(2));
}

#[test]
fn head_ignores_a_big_content_length() {
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000000\r\nContent-Type: text/csv\r\n\r\n");
        std::thread::sleep(Duration::from_secs(1));
    });
    let req = Request { method: Method::Head, ..Request::get(url) };
    let o = Options { max_reply: 10, ..opts() };
    let r = request(&req, &o, &no_stop()).unwrap();
    assert_eq!((r.status, r.body.len(), r.cut), (200, 0, false));
    assert_eq!(r.content_type.as_deref(), Some("text/csv"));
}

#[test]
fn chunked_bodies_are_joined() {
    let url = raw(|s| {
        let _ = s
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n");
    });
    assert_eq!(fetch(&url).unwrap().text(), Some("hello world"));
}

#[test]
fn a_chunked_reply_cut_before_the_last_chunk_is_broken() {
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n");
    });
    assert_eq!(fetch(&url).unwrap_err(), Error::Broken { status: 200 });
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhel");
    });
    assert_eq!(fetch(&url).unwrap_err(), Error::Broken { status: 200 });
}

#[test]
fn a_body_shorter_than_its_length_is_broken_with_the_status() {
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort");
    });
    assert_eq!(fetch(&url).unwrap_err(), Error::Broken { status: 200 });
}

#[test]
fn a_stalled_body_ends_at_the_deadline() {
    let url = raw(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nsome");
        std::thread::sleep(Duration::from_secs(5));
    });
    let o = Options { deadline: Duration::from_millis(400), ..opts() };
    let t = Instant::now();
    assert_eq!(get(&url, &o, &no_stop()).unwrap_err(), Error::Timeout);
    assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
}

#[test]
fn shift_jis_is_not_text() {
    let s = server();
    s.route("GET", "/sjis", Route::new(200).header("Content-Type", "text/csv").body(vec![0x93, 0xFA, 0x95, 0x74, 0x2C, 0x96, 0xBC]));
    let r = fetch(&format!("{}/sjis", s.url())).unwrap();
    assert_eq!(r.text(), None);
    assert_eq!(r.body.len(), 7);
    assert!(r.text_lossy().contains('\u{FFFD}'));
}

#[test]
fn deadline_passes_while_the_server_is_slow() {
    let s = server();
    s.route("GET", "/slow", Route::new(200).delay(Duration::from_secs(5)));
    let o = Options { deadline: Duration::from_millis(300), ..opts() };
    let t = Instant::now();
    let e = get(&format!("{}/slow", s.url()), &o, &no_stop()).unwrap_err();
    assert_eq!(e, Error::Timeout);
    assert!(e.retryable());
    assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
}

#[test]
fn the_stop_flag_ends_the_call_fast() {
    let s = server();
    s.route("GET", "/slow", Route::new(200).delay(Duration::from_secs(5)));
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        flag.store(true, Ordering::Relaxed);
    });
    let t = Instant::now();
    let e = get(&format!("{}/slow", s.url()), &opts(), &stop).unwrap_err();
    assert_eq!(e, Error::Stopped);
    assert!(!e.retryable());
    assert!(t.elapsed() < Duration::from_secs(1), "{:?}", t.elapsed());
}

#[test]
fn a_flag_already_set_stops_before_any_request() {
    let s = server();
    let stop = AtomicBool::new(true);
    assert_eq!(get(&s.url(), &opts(), &stop).unwrap_err(), Error::Stopped);
    assert!(s.requests().is_empty());
}

#[test]
fn urls_and_requests_are_refused_up_front() {
    let s = server();
    let https = Options { https_only: true, ..opts() };
    assert!(matches!(get(&s.url(), &https, &no_stop()), Err(Error::BadUrl(_))));
    assert!(s.requests().is_empty());
    for u in ["ftp://127.0.0.1/x", "garbage", "http://"] {
        assert!(matches!(fetch(u), Err(Error::BadUrl(_))), "{u}");
    }
    let userinfo = format!("http://user:pw@{}/", s.addr());
    assert!(matches!(fetch(&userinfo), Err(Error::InvalidRequest(_))));
    assert!(s.requests().is_empty());
}

#[test]
fn errors_and_debug_never_show_secrets() {
    // A port nobody listens on.
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let req = Request {
        method: Method::Post,
        url: format!("http://127.0.0.1:{port}/very/private/path?apikey=QUERYSECRET"),
        headers: vec![Header::secret("Authorization", "Bearer HEADERSECRET")],
        body: Some(Body { bytes: b"BODYSECRET".to_vec(), content_type: "text/plain".into() }),
    };
    let e = request(&req, &opts(), &no_stop()).unwrap_err();
    assert!(matches!(e, Error::Connect(_)), "{e:?}");
    assert!(e.retryable());
    let shown = format!("{e} {e:?} {req:?}");
    for hidden in ["private", "QUERYSECRET", "HEADERSECRET", "BODYSECRET", "apikey"] {
        assert!(!shown.contains(hidden), "{hidden} in {shown}");
    }
    assert!(shown.contains("127.0.0.1"));
}
