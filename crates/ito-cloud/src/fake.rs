//! A small S3 on `127.0.0.1` for tests (feature `test-util`). It checks the SigV4 signature and the body's SHA-256 of
//! every request, keeps objects and multipart uploads in memory, pages listings, and can be told to fail or be slow.
//! Path style only (`http://127.0.0.1:port/bucket/key`); [`FakeS3::target`] gives the matching [`Target`].

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::sign::{authorization, base64, iso_date, sha256_hex, sha256_raw, Keys};
use crate::{xml, Target};

pub const BUCKET: &str = "bucket";
pub const KEY_ID: &str = "AKIAFAKEFAKEFAKE";
pub const SECRET: &str = "fake-secret-key";
pub const REGION: &str = "us-east-1";

#[derive(Default)]
struct State {
    objects: BTreeMap<String, Vec<u8>>,
    /// upload id → (key, parts by number)
    uploads: HashMap<String, (String, BTreeMap<u64, Vec<u8>>)>,
    next_upload: u64,
    /// key → `x-amz-checksum-sha256` the object was stored with (`…-N` for a multipart one)
    sums: BTreeMap<String, String>,
    /// flip one byte of every body after its hash was checked, as a bad link would
    damage: bool,
    log: Vec<String>,
    /// (text the request line must contain, status, times left)
    failures: Vec<(String, u16, usize)>,
    page_size: usize,
    delay: Duration,
}

pub struct FakeS3 {
    addr: std::net::SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl FakeS3 {
    pub fn start() -> FakeS3 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1");
        let addr = listener.local_addr().expect("local addr");
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                if sp.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(conn) = conn else { continue };
                let st = st.clone();
                std::thread::spawn(move || serve(conn, &st));
            }
        });
        FakeS3 { addr, state, stop }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A [`Target`] that reaches this server with the right credentials.
    pub fn target(&self) -> Target {
        Target::S3 {
            bucket: BUCKET.into(),
            region: REGION.into(),
            endpoint: Some(format!("http://{}", self.addr)),
            path_style: true,
            key_id: KEY_ID.into(),
            secret: SECRET.into(),
            session_token: None,
        }
    }

    pub fn put_object(&self, key: &str, data: &[u8]) {
        self.lock().objects.insert(key.to_owned(), data.to_vec());
    }

    pub fn object(&self, key: &str) -> Option<Vec<u8>> {
        self.lock().objects.get(key).cloned()
    }

    pub fn keys(&self) -> Vec<String> {
        self.lock().objects.keys().cloned().collect()
    }

    /// Multipart uploads begun and neither completed nor aborted.
    pub fn open_uploads(&self) -> usize {
        self.lock().uploads.len()
    }

    /// Flip a byte of the stored object `key` but keep its checksum, as a bad disk would.
    pub fn corrupt_object(&self, key: &str) {
        if let Some(d) = self.lock().objects.get_mut(key) {
            if let Some(b) = d.first_mut() {
                *b ^= 0xff;
            }
        }
    }

    /// Flip one byte of every body that arrives, after its `x-amz-content-sha256` was checked (what a bad link does that
    /// the hash of the request cannot see). A request with `x-amz-checksum-sha256` then fails with `BadDigest`.
    pub fn damage_in_transit(&self, on: bool) {
        self.lock().damage = on;
    }

    /// The `x-amz-checksum-sha256` an object was stored with.
    pub fn checksum(&self, key: &str) -> Option<String> {
        self.lock().sums.get(key).cloned()
    }

    /// Every request so far as `METHOD /bucket/key?query`.
    pub fn requests(&self) -> Vec<String> {
        self.lock().log.clone()
    }

    /// Answer `status` to the next `times` requests whose line (`METHOD target`) contains `pattern`.
    pub fn fail(&self, pattern: &str, status: u16, times: usize) {
        self.lock().failures.push((pattern.to_owned(), status, times));
    }

    /// Keys per listing page (default 1000).
    pub fn set_page_size(&self, n: usize) {
        self.lock().page_size = n;
    }

    /// Wait this long before answering every request.
    pub fn set_delay(&self, d: Duration) {
        self.lock().delay = d;
    }
}

impl Drop for FakeS3 {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
    }
}

struct Request {
    method: String,
    target: String,
    path: String,
    query: Vec<(String, String)>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next()?.to_owned(), parts.next()?.to_owned());
    let mut headers = HashMap::new();
    loop {
        line.clear();
        r.read_line(&mut line).ok()?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':')?;
        headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
    }
    let len: usize = headers.get("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    let (path, q) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let query = q
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect();
    Some(Request { method, path: path.to_owned(), target, query, headers, body })
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn authentic(req: &Request) -> bool {
    let Some(auth) = req.headers.get("authorization") else { return false };
    let Some(signed) = auth.split("SignedHeaders=").nth(1).and_then(|s| s.split(',').next()) else { return false };
    let mut headers = Vec::new();
    for name in signed.split(';') {
        let Some(v) = req.headers.get(name) else { return false };
        headers.push((name, v.clone()));
    }
    let query: Vec<(&str, String)> = req.query.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let hash = req.headers.get("x-amz-content-sha256").cloned().unwrap_or_default();
    let date = req.headers.get("x-amz-date").cloned().unwrap_or_default();
    if date.len() < 8 {
        return false;
    }
    let keys = Keys { key_id: KEY_ID, secret: SECRET, region: REGION };
    authorization(&keys, &req.method, &req.path, &query, &headers, &hash, &date) == *auth
}

fn reply(mut stream: &TcpStream, code: u16, headers: &[(&str, String)], body: &[u8]) {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn error(stream: &TcpStream, code: u16, name: &str, message: &str) {
    let body = format!("<?xml version=\"1.0\"?><Error><Code>{name}</Code><Message>{}</Message></Error>", xml::escape(message));
    reply(stream, code, &[("Content-Type", "application/xml".into())], body.as_bytes());
}

fn serve(stream: TcpStream, state: &Mutex<State>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Some(req) = read_request(&stream) else { return };
    let (delay, injected) = {
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        let line = format!("{} {}", req.method, req.target);
        s.log.push(line.clone());
        let hit = s.failures.iter().position(|(p, _, n)| *n > 0 && line.contains(p.as_str()));
        let code = hit.map(|i| {
            s.failures[i].2 -= 1;
            s.failures[i].1
        });
        (s.delay, code)
    };
    if !delay.is_zero() {
        std::thread::sleep(delay);
    }
    if let Some(code) = injected {
        let name = if code == 403 { "AccessDenied" } else { "Injected" };
        return error(&stream, code, name, "injected failure");
    }
    if !authentic(&req) {
        return error(&stream, 403, "SignatureDoesNotMatch", "the signature does not match");
    }
    if req.headers.get("x-amz-content-sha256").map(String::as_str) != Some(sha256_hex(&req.body).as_str()) {
        return error(&stream, 400, "XAmzContentSHA256Mismatch", "the body does not match its hash");
    }
    let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
    let mut req = req;
    if s.damage {
        if let Some(b) = req.body.first_mut() {
            *b ^= 0xff;
        }
    }
    if let Some(sum) = req.headers.get("x-amz-checksum-sha256") {
        if *sum != base64(&sha256_raw(&req.body)) {
            drop(s);
            return error(&stream, 400, "BadDigest", "the SHA-256 you specified did not match the calculated checksum");
        }
    }
    handle(&stream, &req, &mut s);
}

fn handle(stream: &TcpStream, req: &Request, s: &mut State) {
    let Some(rest) = req.path.strip_prefix('/').map(decode) else { return error(stream, 404, "NoSuchBucket", "no bucket") };
    let (bucket, key) = rest.split_once('/').unwrap_or((rest.as_str(), ""));
    if bucket != BUCKET {
        return error(stream, 404, "NoSuchBucket", "no such bucket");
    }
    let q = |name: &str| req.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let has = |name: &str| req.query.iter().any(|(k, _)| k == name);
    match (req.method.as_str(), key.is_empty()) {
        ("GET", true) => list(stream, req, s),
        ("PUT", false) if has("partNumber") => {
            let (Some(id), Some(n)) = (q("uploadId"), q("partNumber").and_then(|n| n.parse::<u64>().ok())) else {
                return error(stream, 400, "InvalidArgument", "partNumber and uploadId");
            };
            let Some((_, parts)) = s.uploads.get_mut(&id) else { return error(stream, 404, "NoSuchUpload", "no such upload") };
            let etag = format!("\"{}\"", &sha256_hex(&req.body)[..32]);
            parts.insert(n, req.body.clone());
            reply(stream, 200, &[("ETag", etag)], b"");
        }
        ("PUT", false) => {
            let etag = format!("\"{}\"", &sha256_hex(&req.body)[..32]);
            s.objects.insert(key.to_owned(), req.body.clone());
            match req.headers.get("x-amz-checksum-sha256") {
                Some(sum) => s.sums.insert(key.to_owned(), sum.clone()),
                None => s.sums.remove(key),
            };
            reply(stream, 200, &[("ETag", etag)], b"");
        }
        ("POST", false) if has("uploads") => {
            s.next_upload += 1;
            let id = format!("upload-{}", s.next_upload);
            s.uploads.insert(id.clone(), (key.to_owned(), BTreeMap::new()));
            let body = format!("<InitiateMultipartUploadResult><Bucket>{BUCKET}</Bucket><Key>{}</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>", xml::escape(key));
            reply(stream, 200, &[], body.as_bytes());
        }
        ("POST", false) if has("uploadId") => {
            let id = q("uploadId").unwrap_or_default();
            let Some((_, parts)) = s.uploads.get(&id) else { return error(stream, 404, "NoSuchUpload", "no such upload") };
            let text = String::from_utf8_lossy(&req.body).into_owned();
            let mut whole = Vec::new();
            let mut raw = Vec::new();
            let mut with_sums = true;
            for p in xml::blocks(&text, "Part") {
                let n = xml::first(p, "PartNumber").and_then(|n| n.parse::<u64>().ok()).unwrap_or(0);
                let Some(data) = parts.get(&n) else { return error(stream, 400, "InvalidPart", "a part is missing") };
                let etag = format!("\"{}\"", &sha256_hex(data)[..32]);
                if xml::first(p, "ETag").as_deref() != Some(etag.as_str()) {
                    return error(stream, 400, "InvalidPart", "a part's ETag does not match");
                }
                let digest = sha256_raw(data);
                match xml::first(p, "ChecksumSHA256") {
                    Some(c) if c != base64(&digest) => return error(stream, 400, "InvalidPart", "a part's checksum does not match"),
                    Some(_) => {}
                    None => with_sums = false,
                }
                raw.extend_from_slice(&digest);
                whole.extend_from_slice(data);
            }
            let count = xml::blocks(&text, "Part").len();
            s.uploads.remove(&id);
            s.objects.insert(key.to_owned(), whole);
            if with_sums {
                s.sums.insert(key.to_owned(), format!("{}-{count}", base64(&sha256_raw(&raw))));
            } else {
                s.sums.remove(key);
            }
            let body = format!("<CompleteMultipartUploadResult><Key>{}</Key></CompleteMultipartUploadResult>", xml::escape(key));
            reply(stream, 200, &[], body.as_bytes());
        }
        ("DELETE", false) if has("uploadId") => {
            let id = q("uploadId").unwrap_or_default();
            if s.uploads.remove(&id).is_none() {
                return error(stream, 404, "NoSuchUpload", "no such upload");
            }
            reply(stream, 204, &[], b"");
        }
        ("DELETE", false) => {
            s.objects.remove(key);
            reply(stream, 204, &[], b"");
        }
        ("GET", false) => match s.objects.get(key) {
            Some(data) => {
                let mut h = vec![("ETag", format!("\"{}\"", &sha256_hex(data)[..32]))];
                if let (Some(sum), true) = (s.sums.get(key), req.headers.get("x-amz-checksum-mode").is_some_and(|m| m == "ENABLED")) {
                    h.push(("x-amz-checksum-sha256", sum.clone()));
                }
                reply(stream, 200, &h, data)
            }
            None => error(stream, 404, "NoSuchKey", "the key does not exist"),
        },
        _ => error(stream, 400, "InvalidRequest", "not supported by the fake"),
    }
}

fn list(stream: &TcpStream, req: &Request, s: &State) {
    let q = |name: &str| req.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let prefix = q("prefix").unwrap_or_default();
    let after = q("continuation-token");
    let page = if s.page_size == 0 { 1000 } else { s.page_size };
    let mut keys = s.objects.keys().filter(|k| k.starts_with(&prefix) && after.as_ref().is_none_or(|a| *k > a));
    let mut body = String::from("<?xml version=\"1.0\"?><ListBucketResult>");
    let mut last = None;
    let mut n = 0;
    for k in keys.by_ref().take(page) {
        let modified = iso_date(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        body.push_str(&format!(
            "<Contents><Key>{}</Key><LastModified>{modified}</LastModified><Size>{}</Size></Contents>",
            xml::escape(k),
            s.objects[k].len()
        ));
        last = Some(k.clone());
        n += 1;
    }
    let more = keys.next().is_some();
    body.push_str(&format!("<KeyCount>{n}</KeyCount><IsTruncated>{more}</IsTruncated>"));
    if let (true, Some(l)) = (more, last) {
        body.push_str(&format!("<NextContinuationToken>{}</NextContinuationToken>", xml::escape(&l)));
    }
    body.push_str("</ListBucketResult>");
    reply(stream, 200, &[("Content-Type", "application/xml".into())], body.as_bytes());
}
