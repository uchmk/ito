use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, SystemTime};

use ito_mail::{compose, Attachment, Outgoing};
use ito_testserver::smtp::{Mode, SmtpServer};
use ito_testserver::tls::pinned_client_config;
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};

type Tls = StreamOwned<ClientConnection, TcpStream>;

fn connect(server: &SmtpServer) -> TcpStream {
    let s = TcpStream::connect(server.addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s
}

fn tls(server: &SmtpServer, tcp: TcpStream) -> Tls {
    let conn =
        ClientConnection::new(pinned_client_config(server.fingerprint().unwrap()), ServerName::try_from("localhost").unwrap()).unwrap();
    StreamOwned::new(conn, tcp)
}

/// A client that reads one reply (all its lines) at a time.
struct Client<S: Read + Write> {
    io: BufReader<S>,
}

impl<S: Read + Write> Client<S> {
    fn new(s: S) -> Self {
        Client { io: BufReader::new(s) }
    }

    fn reply(&mut self) -> String {
        let mut all = String::new();
        loop {
            let mut l = String::new();
            if self.io.read_line(&mut l).unwrap() == 0 {
                return all;
            }
            all.push_str(&l);
            if l.as_bytes().get(3) != Some(&b'-') {
                return all;
            }
        }
    }

    fn send(&mut self, line: &str) -> String {
        self.io.get_mut().write_all(format!("{line}\r\n").as_bytes()).unwrap();
        self.reply()
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.io.get_mut().write_all(bytes).unwrap();
    }

    /// The whole envelope and data; the last reply.
    fn deliver(&mut self, from: &str, to: &[&str], data: &[u8]) -> String {
        assert!(self.send(&format!("MAIL FROM:<{from}>")).starts_with("250"));
        for t in to {
            assert!(self.send(&format!("RCPT TO:<{t}>")).starts_with("250"));
        }
        assert!(self.send("DATA").starts_with("354"));
        self.raw(data);
        self.raw(b"\r\n.\r\n");
        self.reply()
    }
}

fn b64(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[test]
fn plain_mode_takes_a_message_as_memory_and_as_an_eml_file() {
    let server = SmtpServer::start(Mode::Plain).unwrap();
    assert!(server.fingerprint().is_none());
    let mut c = Client::new(connect(&server));
    assert!(c.reply().starts_with("220"));
    let ehlo = c.send("EHLO test");
    assert!(!ehlo.contains("STARTTLS") && !ehlo.contains("AUTH"));
    let done = c.deliver("a@x.test", &["b@y.test", "c@y.test"], b"Subject: hi\r\n\r\nline\r\n..dot");
    assert!(done.starts_with("250"), "{done}");
    assert!(c.send("QUIT").starts_with("221"));

    let got = server.wait_for_messages(1, Duration::from_secs(5));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].from, "a@x.test");
    assert_eq!(got[0].to, ["b@y.test", "c@y.test"]);
    assert_eq!(got[0].raw, b"Subject: hi\r\n\r\nline\r\n.dot\r\n");
    assert_eq!(std::fs::read(&got[0].path).unwrap(), got[0].raw);
    assert_eq!(got[0].path.parent().unwrap(), server.eml_dir());
    assert!(server.log().iter().any(|l| l == "C: RCPT TO:<c@y.test>"));
}

#[test]
fn starttls_offers_auth_only_after_the_upgrade() {
    let server = SmtpServer::start_with_login(Mode::StartTls, "bot", "s3cret").unwrap();
    let mut c = Client::new(connect(&server));
    c.reply();
    let ehlo = c.send("EHLO test");
    assert!(ehlo.contains("STARTTLS") && !ehlo.contains("AUTH"), "{ehlo}");
    assert!(c.send("AUTH PLAIN").starts_with("530"));
    assert!(c.send("STARTTLS").starts_with("220"));
    let tcp = c.io.into_inner();
    let mut c = Client::new(tls(&server, tcp));
    let ehlo = c.send("EHLO test");
    assert!(ehlo.contains("AUTH PLAIN LOGIN") && !ehlo.contains("STARTTLS"), "{ehlo}");
    assert!(c.send("MAIL FROM:<a@x.test>").starts_with("530"));
    assert!(c.send(&format!("AUTH PLAIN {}", b64("\0bot\0s3cret"))).starts_with("235"));
    assert!(c.deliver("a@x.test", &["b@y.test"], b"Subject: s\r\n\r\nbody").starts_with("250"));
    assert_eq!(server.wait_for_messages(1, Duration::from_secs(5)).len(), 1);
    let log = server.log();
    assert!(log.contains(&"! starttls".to_owned()) && log.contains(&"! auth ok".to_owned()));
    assert!(!log.iter().any(|l| l.contains("s3cret") || l.contains(&b64("\0bot\0s3cret"))));
}

#[test]
fn implicit_tls_with_auth_login_and_a_wrong_password() {
    let server = SmtpServer::start_with_login(Mode::ImplicitTls, "bot", "s3cret").unwrap();
    assert_eq!(server.fingerprint_text().unwrap().len(), 32 * 3 - 1);
    let mut c = Client::new(tls(&server, connect(&server)));
    assert!(c.reply().starts_with("220"));
    assert!(c.send("EHLO test").contains("AUTH PLAIN LOGIN"));
    assert!(c.send("AUTH LOGIN").starts_with(&format!("334 {}", b64("Username:"))));
    assert!(c.send(&b64("bot")).starts_with(&format!("334 {}", b64("Password:"))));
    assert!(c.send(&b64("wrong")).starts_with("535"));
    assert!(c.send("AUTH LOGIN").starts_with("334"));
    c.send(&b64("bot"));
    assert!(c.send(&b64("s3cret")).starts_with("235"));
    assert!(c.deliver("a@x.test", &["b@y.test"], b"Subject: s\r\n\r\nbody").starts_with("250"));
    assert_eq!(server.messages().len(), 1);
}

#[test]
fn a_client_that_pins_another_certificate_is_refused() {
    let server = SmtpServer::start(Mode::ImplicitTls).unwrap();
    let mut wrong = server.fingerprint().unwrap();
    wrong[0] ^= 1;
    let conn = ClientConnection::new(pinned_client_config(wrong), ServerName::try_from("localhost").unwrap()).unwrap();
    let mut s = StreamOwned::new(conn, connect(&server));
    let mut buf = [0u8; 16];
    assert!(s.read(&mut buf).is_err());
}

#[test]
fn a_refused_recipient_is_550_and_the_others_go_through() {
    let server = SmtpServer::start(Mode::Plain).unwrap();
    server.reject_recipient("Bad@y.test");
    let mut c = Client::new(connect(&server));
    c.reply();
    c.send("EHLO t");
    assert!(c.send("MAIL FROM:<a@x.test>").starts_with("250"));
    assert!(c.send("RCPT TO:<bad@y.test>").starts_with("550"));
    assert!(c.send("RCPT TO:<ok@y.test>").starts_with("250"));
    assert!(c.send("DATA").starts_with("354"));
    c.raw(b"Subject: s\r\n\r\nx\r\n.\r\n");
    assert!(c.reply().starts_with("250"));
    assert_eq!(server.messages()[0].to, ["ok@y.test"]);
}

#[test]
fn the_connection_can_be_cut_in_the_middle_of_data() {
    let server = SmtpServer::start(Mode::Plain).unwrap();
    server.cut_during_data(true);
    let mut c = Client::new(connect(&server));
    c.reply();
    c.send("EHLO t");
    c.send("MAIL FROM:<a@x.test>");
    c.send("RCPT TO:<b@y.test>");
    assert!(c.send("DATA").starts_with("354"));
    c.raw(b"Subject: s\r\n\r\npartial\r\n");
    assert_eq!(c.reply(), "", "the connection closes with no answer");
    assert!(server.messages().is_empty());
    assert!(server.log().contains(&"! cut".to_owned()));
}

#[test]
fn a_composed_message_with_an_attachment_arrives_byte_for_byte() {
    let server = SmtpServer::start_with_login(Mode::StartTls, "bot", "pw").unwrap();
    let files = [Attachment { name: "r.csv", mime: Some("text/csv"), bytes: b"a,b\r\n1,2\r\n", cid: None }];
    let mail = compose(
        &Outgoing {
            from: "Ops <ops@x.test>",
            to: "dev@y.test",
            cc: "",
            bcc: "",
            subject: "Report \u{2713}",
            text: "see attached",
            html: None,
            in_reply_to: None,
            references: "",
            files: &files,
        },
        SystemTime::now(),
    );
    let mut c = Client::new(connect(&server));
    c.reply();
    c.send("EHLO t");
    c.send("STARTTLS");
    let mut c = Client::new(tls(&server, c.io.into_inner()));
    c.send("EHLO t");
    assert!(c.send(&format!("AUTH PLAIN {}", b64("\0bot\0pw"))).starts_with("235"));
    // The message has no line that starts with a dot, so it needs no stuffing; `compose` ends its last line.
    assert!(c.deliver("ops@x.test", &["dev@y.test"], mail.strip_suffix(b"\r\n").unwrap_or(&mail)).starts_with("250"));
    let got = server.wait_for_messages(1, Duration::from_secs(5));
    assert_eq!(got[0].raw, mail);
    assert!(got[0].text().contains("filename=\"r.csv\""));
}

#[test]
fn dropping_the_server_stops_it_and_removes_the_folder() {
    let server = SmtpServer::start(Mode::Plain).unwrap();
    let (addr, dir) = (server.addr(), server.eml_dir().to_owned());
    assert!(dir.is_dir());
    drop(server);
    assert!(!dir.exists());
    assert!(TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_err());
}
