use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use ito_testserver::http::{HttpServer, Reply};

/// One raw request; the whole answer as text.
fn ask(server: &HttpServer, request: &str) -> String {
    let mut s = TcpStream::connect(server.addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn a_route_answers_and_everything_else_is_404() {
    let server = HttpServer::start().unwrap();
    server.route("get", "/hello", Reply::new(200).header("X-Test", "1").body("hi")).route(
        "POST",
        "/echo",
        Reply::new(201).json("{\"ok\":true}"),
    );
    let a = ask(&server, "GET /hello?x=1 HTTP/1.1\r\nHost: t\r\nX-Token: abc\r\n\r\n");
    assert!(a.starts_with("HTTP/1.1 200 OK\r\n") && a.contains("X-Test: 1\r\n") && a.ends_with("\r\n\r\nhi"), "{a}");
    let b = ask(&server, "POST /echo HTTP/1.1\r\nHost: t\r\nContent-Length: 5\r\n\r\nhello");
    assert!(b.starts_with("HTTP/1.1 201 ") && b.ends_with("{\"ok\":true}") && b.contains("application/json"), "{b}");
    assert!(ask(&server, "GET /nope HTTP/1.1\r\nHost: t\r\n\r\n").starts_with("HTTP/1.1 404 "));
    assert!(ask(&server, "POST /hello HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\n\r\n").starts_with("HTTP/1.1 404 "));

    let seen = server.requests();
    assert_eq!(seen.len(), 4);
    assert_eq!((seen[0].method.as_str(), seen[0].path.as_str(), seen[0].query.as_str()), ("GET", "/hello", "x=1"));
    assert_eq!(seen[0].header("x-token"), Some("abc"));
    assert_eq!(seen[1].body_text(), "hello");
}

#[test]
fn a_chunked_body_is_joined_and_a_later_route_replaces_an_earlier_one() {
    let server = HttpServer::start().unwrap();
    server.route("POST", "/up", Reply::new(500));
    server.route("POST", "/up", Reply::new(204));
    let a = ask(&server, "POST /up HTTP/1.1\r\nHost: t\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n");
    assert!(a.starts_with("HTTP/1.1 204 "), "{a}");
    assert_eq!(server.requests()[0].body_text(), "abcde");
}

#[test]
fn a_delay_holds_the_answer_back() {
    let server = HttpServer::start().unwrap();
    server.route("GET", "/slow", Reply::new(200).delay(Duration::from_millis(300)));
    let t = Instant::now();
    assert!(ask(&server, "GET /slow HTTP/1.1\r\nHost: t\r\n\r\n").starts_with("HTTP/1.1 200"));
    assert!(t.elapsed() >= Duration::from_millis(290));
}

#[test]
fn dropping_the_server_stops_it() {
    let server = HttpServer::start().unwrap();
    let addr = server.addr();
    assert!(server.url().starts_with("http://127.0.0.1:"));
    drop(server);
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}
