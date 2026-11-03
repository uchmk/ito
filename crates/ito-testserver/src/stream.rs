//! A connection that is plain or TLS, with a buffered reader, shared by the IMAP and WinRM servers.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};

use rustls::{ServerConnection, StreamOwned};

use crate::is_timeout;
use crate::tls::Cert;

pub(crate) enum Conn {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ServerConnection, TcpStream>>),
}

impl Conn {
    /// Plain, or TLS from the first byte when `cert` is given.
    pub fn new(stream: TcpStream, cert: Option<&Cert>) -> io::Result<Conn> {
        match cert {
            None => Ok(Conn::Plain(stream)),
            Some(c) => {
                let conn = ServerConnection::new(c.server_config()).map_err(io::Error::other)?;
                Ok(Conn::Tls(Box::new(StreamOwned::new(conn, stream))))
            }
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Conn::Plain(s) => s.read(buf),
            Conn::Tls(s) => s.read(buf),
        }
    }

    pub fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Conn::Plain(s) => s.write_all(bytes).and_then(|_| s.flush()),
            Conn::Tls(s) => s.write_all(bytes).and_then(|_| s.flush()),
        }
    }
}

/// A connection with what was read and not yet used.
pub(crate) struct Reader<'a> {
    pub conn: Conn,
    buf: Vec<u8>,
    stop: &'a AtomicBool,
}

impl<'a> Reader<'a> {
    pub fn new(conn: Conn, stop: &'a AtomicBool) -> Self {
        Reader { conn, buf: Vec::new(), stop }
    }

    /// Whether `cond` (checked between reads) says to give up, or the server is stopping.
    fn fill(&mut self, cond: &dyn Fn() -> bool) -> Option<()> {
        loop {
            if self.stop.load(Ordering::Relaxed) || cond() {
                return None;
            }
            let mut tmp = [0u8; 8192];
            match self.conn.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => {
                    self.buf.extend_from_slice(&tmp[..n]);
                    return Some(());
                }
                Err(e) if is_timeout(&e) => {}
                Err(_) => return None,
            }
        }
    }

    /// One line without its line end; `None` when the peer is gone, the server stops or `cond` is true.
    pub fn line(&mut self, cond: &dyn Fn() -> bool) -> Option<String> {
        loop {
            if let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=i).collect();
                return Some(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_owned());
            }
            self.fill(cond)?;
        }
    }

    pub fn take(&mut self, n: usize, cond: &dyn Fn() -> bool) -> Option<Vec<u8>> {
        while self.buf.len() < n {
            self.fill(cond)?;
        }
        Some(self.buf.drain(..n).collect())
    }
}

pub(crate) fn b64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub(crate) fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in s.trim().bytes().take_while(|&b| b != b'=') {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
