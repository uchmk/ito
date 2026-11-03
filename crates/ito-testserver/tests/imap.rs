use std::net::TcpStream;
use std::time::Duration;

use ito_mail::{Account, Error, From, Inbox, Position, Stream};
use ito_testserver::imap::{ImapServer, Mode};
use ito_testserver::tls::pinned_client_config;
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, StreamOwned};

fn account(server: &ImapServer) -> Account {
    Account { host: "127.0.0.1".into(), port: server.port(), user: "me@example.com".into(), folder: "INBOX".into() }
}

fn open_as(server: &ImapServer, password: &str, from: From) -> Result<Inbox, Error> {
    let (addr, pin) = (server.addr(), server.fingerprint());
    Inbox::open_with(
        move || {
            let tcp = TcpStream::connect(addr).map_err(|e| Error::Connect(e.to_string()))?;
            tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            Ok(match pin {
                None => Box::new(tcp) as Box<dyn Stream>,
                Some(fp) => {
                    let conn = ClientConnection::new(pinned_client_config(fp), ServerName::try_from("localhost").unwrap()).unwrap();
                    Box::new(StreamOwned::new(conn, tcp))
                }
            })
        },
        &account(server),
        password,
        from,
    )
}

fn open(server: &ImapServer, from: From) -> Inbox {
    open_as(server, "secret", from).expect("open")
}

#[test]
fn new_mail_is_returned_once_over_plain_and_over_tls() {
    for mode in [Mode::Plain, Mode::ImplicitTls] {
        let server = ImapServer::start(mode).unwrap();
        server.add_mail("old@example.com", "before");
        let mut inbox = open(&server, From::End);
        assert!(inbox.poll().unwrap().is_empty(), "{mode:?}: mail from before open is not new");
        let uid = server.add_mail("Ann <ann@example.com>", "Hello there");
        let got = inbox.poll().unwrap();
        assert_eq!(got.len(), 1, "{mode:?}");
        assert_eq!(got[0].uid, uid);
        assert_eq!(got[0].from, "ann@example.com");
        assert_eq!(got[0].name.as_deref(), Some("Ann"));
        assert_eq!(got[0].subject, "Hello there");
        assert!(got[0].date.is_some());
        assert!(inbox.poll().unwrap().is_empty(), "the same mail is not returned twice");
        let cmds = server.commands();
        assert!(cmds.iter().any(|c| c.starts_with("LOGIN \"me@example.com\"") || c.starts_with("LOGIN me@example.com")), "{cmds:?}");
        assert!(cmds.iter().any(|c| c.starts_with("EXAMINE")), "{cmds:?}");
        assert!(cmds.iter().any(|c| c.contains("BODY.PEEK[HEADER.FIELDS")), "{cmds:?}");
        assert!(!server.log().iter().any(|l| l.contains("secret")), "the password is never logged");
    }
}

#[test]
fn resume_with_after_returns_what_came_while_away() {
    let server = ImapServer::start(Mode::ImplicitTls).unwrap();
    server.add_mail("a@example.com", "one");
    let mut inbox = open(&server, From::End);
    server.add_mail("b@example.com", "two");
    assert_eq!(inbox.poll().unwrap().len(), 1);
    let saved = inbox.position();
    assert_eq!(saved.uid, 2);
    drop(inbox);

    let u3 = server.add_mail("c@example.com", "three");
    let u4 = server.add_mail("d@example.com", "four");
    let mut again = open(&server, From::After(saved));
    let got = again.poll().unwrap();
    assert_eq!(got.iter().map(|m| m.uid).collect::<Vec<_>>(), vec![u3, u4]);
    assert_eq!(got[0].subject, "three");
    assert_eq!(again.position(), Position { uid_validity: server.uid_validity(), uid: u4 });
}

#[test]
fn a_uidvalidity_change_starts_over_instead_of_replaying() {
    let server = ImapServer::start(Mode::Plain).unwrap();
    server.add_mail("a@example.com", "one");
    server.add_mail("b@example.com", "two");
    let mut inbox = open(&server, From::End);
    let saved = inbox.position();
    assert_eq!(saved.uid, 2);

    // A restored mailbox: new validity, the same two messages numbered from 1.
    server.renumber(77);
    server.drop_connections();
    std::thread::sleep(Duration::from_millis(400));
    // The first poll sees the broken connection, the next one reconnects.
    let _ = inbox.poll();
    assert!(inbox.poll().unwrap().is_empty(), "the old mail must not come back as new");
    assert_eq!(inbox.position(), Position { uid_validity: 77, uid: 2 });
    let uid = server.add_mail("c@example.com", "three");
    let got = inbox.poll().unwrap();
    assert_eq!(got.iter().map(|m| m.uid).collect::<Vec<_>>(), vec![uid]);

    // A saved position from another validity means nothing to a fresh open.
    let fresh = open(&server, From::After(saved));
    assert_eq!(fresh.position(), Position { uid_validity: 77, uid: 3 });
    assert!(server.connections() >= 3);
}

#[test]
fn a_dropped_connection_is_picked_up_again() {
    let server = ImapServer::start(Mode::ImplicitTls).unwrap();
    let mut inbox = open(&server, From::End);
    assert!(inbox.poll().unwrap().is_empty());
    let before = server.connections();
    server.drop_connections();
    std::thread::sleep(Duration::from_millis(400));
    server.add_mail("a@example.com", "after the drop");
    let mut got = inbox.poll();
    if got.is_err() {
        got = inbox.poll();
    }
    let got = got.unwrap();
    assert_eq!(got.len(), 1);
    assert!(server.connections() > before);
    assert!(server.log().iter().any(|l| l == "! dropped"));
}

#[test]
fn login_and_folder_errors() {
    let server = ImapServer::start_with_login(Mode::Plain, "me@example.com", "secret").unwrap();
    assert_eq!(open_as(&server, "wrong", From::End).err(), Some(Error::Login));
    assert!(open_as(&server, "secret", From::End).is_ok());
    assert!(server.log().iter().any(|l| l == "! login failed"));
    assert!(server.log().iter().any(|l| l == "! login ok"));

    let mut acct = account(&server);
    acct.folder = "Archive".into();
    let addr = server.addr();
    let err = Inbox::open_with(
        move || Ok(Box::new(TcpStream::connect(addr).map_err(|e| Error::Connect(e.to_string()))?) as Box<dyn Stream>),
        &acct,
        "secret",
        From::End,
    )
    .err();
    assert_eq!(err, Some(Error::NoFolder));
}

#[test]
fn a_gap_in_the_uids_and_a_folded_header_are_handled() {
    let server = ImapServer::start(Mode::Plain).unwrap();
    let mut inbox = open(&server, From::End);
    let a = server.add_mail("a@example.com", "one");
    let b = server.add_mail("b@example.com", "two");
    assert!(server.remove_message(a));
    let c = server.add_message(
        "From: c@example.com\r\nSubject: a long\r\n subject line\r\nDate: Tue, 03 Nov 2026 10:00:00 +0900\r\nX-Other: no\r\n\r\nbody\r\n",
    );
    let got = inbox.poll().unwrap();
    assert_eq!(got.iter().map(|m| m.uid).collect::<Vec<_>>(), vec![b, c]);
    assert_eq!(got[1].subject, "a long subject line");
}
