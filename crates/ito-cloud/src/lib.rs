//! Send, fetch, list and delete files in S3-compatible object storage (Amazon S3, MinIO, Cloudflare R2, …) with a plain
//! blocking API: call it from a worker thread.
//!
//! - [`put`]: a file up to one key. A file up to [`Options::part_size`] goes in one request; a bigger one goes as a
//!   multipart upload (parts of `part_size`, at least 5 MiB, at most 10000 parts). A part that fails in a way that may
//!   pass ([`Error::retryable`]) is sent again up to 3 times. A failed or stopped upload is aborted so no half-made
//!   upload is left to be billed. There is no resume: a new call starts over.
//! - [`get`]: a key to a file, written to `<name>.part` and renamed when the whole body has come.
//! - [`list`]: the keys that match a pattern with `*` and `?` (the rule of `ito-match`; `*` also matches `/`). Only the
//!   keys from the text before the first wildcard are asked for, and every page of the answer is followed.
//! - [`delete`]: one key. Deleting a key that is not there is not an error (S3 says so).
//!
//! Requests are signed with SigV4 (the body's SHA-256 is signed, so a corrupted part is refused by the server). The
//! secret is given as text in [`Target`]; the caller reads it from `ito-secret` first. `Debug` hides it.
//! Every call takes a stop flag (looked at between requests and every 64 KiB of a download) and [`Options::timeout`]
//! for the whole call; [`Options::request_timeout`] bounds each request.
//!
//! The HTTP client is `ureq` with rustls (ring), so Linux and macOS builds need a C compiler (as `ito-sftp` does).
//! A server on a plain `http://` endpoint is allowed (MinIO on a LAN); Amazon is always `https://`.
//!
//! Feature `test-util` adds [`fake::FakeS3`], a small S3 on `127.0.0.1` for the tests of a program that uses this crate.

mod sign;
mod xml;

#[cfg(any(test, feature = "test-util"))]
pub mod fake;

use std::fmt;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sign::{authorization, base64, canonical_query, encode, hex, sha256_hex, sha256_raw, Keys};

/// The smallest part S3 accepts (except the last).
pub const MIN_PART: u64 = 5 * 1024 * 1024;
/// The most parts one multipart upload may have.
pub const MAX_PARTS: u64 = 10_000;

const CHUNK: usize = 64 * 1024;
const RETRIES: u32 = 3;

/// Where the files go.
#[derive(Clone, PartialEq, Eq)]
pub enum Target {
    /// An S3 bucket. `endpoint` is `https://host[:port]` (or `http://…`) of an S3-compatible store; `None` is Amazon S3
    /// in `region`. `path_style` puts the bucket in the path (`host/bucket/key`; MinIO needs it) instead of the host.
    S3 {
        bucket: String,
        region: String,
        endpoint: Option<String>,
        path_style: bool,
        key_id: String,
        secret: String,
        /// For temporary credentials (STS).
        session_token: Option<String>,
    },
}

impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::S3 { bucket, region, endpoint, path_style, key_id, session_token, .. } => f
                .debug_struct("S3")
                .field("bucket", bucket)
                .field("region", region)
                .field("endpoint", endpoint)
                .field("path_style", path_style)
                .field("key_id", key_id)
                .field("secret", &"***")
                .field("session_token", &session_token.as_ref().map(|_| "***"))
                .finish(),
        }
    }
}

/// Times and sizes. `Options::default()` is an hour for the call, ten minutes for a request, 16 MiB parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The whole call, every request and wait together.
    pub timeout: Duration,
    /// One request (a part's upload, a listing page, a download from start to end of its body).
    pub request_timeout: Duration,
    /// The size of a part, and the size above which a file goes as parts. Raised to [`MIN_PART`] at least, and as far
    /// as the file needs to fit in [`MAX_PARTS`]. One part is held in memory.
    pub part_size: u64,
    /// Send `x-amz-checksum-sha256` with every upload and ask for it on a download, so the store checks the bytes.
    /// Turn it off for an old S3-compatible store that refuses the header.
    pub checksum: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            timeout: Duration::from_secs(3600),
            request_timeout: Duration::from_secs(600),
            part_size: 16 * 1024 * 1024,
            checksum: true,
        }
    }
}

/// One object of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// What a [`put`] or [`get`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub bytes: u64,
    /// SHA-256 of the whole file as lower-case hex, taken while it streamed (not read back).
    pub sha256: String,
    /// The store checked (or was checked against) the bytes: an upload with the checksum accepted, or a download whose
    /// `x-amz-checksum-sha256` was a whole-object one and matched. False for [`Options::checksum`] off, a multipart
    /// object's checksum (a checksum of checksums) or an object stored without one.
    pub verified: bool,
}

/// `fn(bytes done, bytes total)`, called after each chunk of a download and each part of an upload.
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The key (or the bucket) is not there.
    NotFound,
    /// The bytes that arrived are not the bytes that were sent (the store's `BadDigest`, or a download whose checksum
    /// does not match). Nothing is kept: no object was made, and a download's `.part` is deleted.
    Mismatch(String),
    /// The store refused the credentials or the action (401, 403). The text is the store's own.
    Auth(String),
    /// Any other answer that is not a success.
    Status { code: u16, body: String },
    /// The store could not be reached or the connection broke.
    Network(String),
    /// The TLS handshake failed (a bad certificate, for example).
    Tls(String),
    /// The stop flag was raised.
    Cancelled,
    /// The time ran out.
    Timeout,
    /// A local file could not be read or written, or the arguments are wrong.
    Io(String),
}

impl Error {
    /// Whether the same call may work later: the network, a timeout, a 5xx, 429 and 408.
    pub fn retryable(&self) -> bool {
        match self {
            Error::Network(_) | Error::Timeout => true,
            Error::Status { code, .. } => *code >= 500 || *code == 429 || *code == 408,
            _ => false,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => f.write_str("not found"),
            Error::Mismatch(s) => write!(f, "the bytes do not match: {s}"),
            Error::Auth(s) => write!(f, "refused: {s}"),
            Error::Status { code, body } if body.is_empty() => write!(f, "the store answered {code}"),
            Error::Status { code, body } => write!(f, "the store answered {code}: {body}"),
            Error::Network(s) => write!(f, "network: {s}"),
            Error::Tls(s) => write!(f, "TLS: {s}"),
            Error::Cancelled => f.write_str("cancelled"),
            Error::Timeout => f.write_str("timed out"),
            Error::Io(s) => write!(f, "file: {s}"),
        }
    }
}

impl std::error::Error for Error {}

fn io_err(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

/// Send `local` to `key`; returns what was sent. With [`Options::checksum`] the store checks the SHA-256 of every request
/// (a mismatch is [`Error::Mismatch`] and no object is made). A `key` that is empty or ends with `/` gets the file's name added
/// (`backup/` + `a.zip` = `backup/a.zip`). Keys have no leading `/`.
pub fn put(target: &Target, local: &Path, key: &str, opts: &Options, cancel: &AtomicBool, progress: Progress) -> Result<Moved, Error> {
    let key = full_key(key, local)?;
    let size = fs::metadata(local).map_err(io_err)?.len();
    let ctx = Ctx::new(target, opts, cancel)?;
    ctx.check()?;
    let part = opts.part_size.max(MIN_PART).max(size.div_ceil(MAX_PARTS));
    if size <= part {
        let mut data = Vec::with_capacity(size as usize);
        File::open(local).map_err(io_err)?.read_to_end(&mut data).map_err(io_err)?;
        let hash = sha256_hex(&data);
        let extra: Vec<(&str, String)> =
            if opts.checksum { vec![("x-amz-checksum-sha256", base64(&sha256_raw(&data)))] } else { Vec::new() };
        ctx.retry(|| ctx.call("PUT", &key, &[], &data, &hash, &extra).and_then(|r| ctx.ok(r)).map(drop))?;
        progress(size, size);
        return Ok(Moved { bytes: size, sha256: hash, verified: opts.checksum });
    }
    let upload = ctx.create_upload(&key, opts.checksum)?;
    let sent = send_parts(&ctx, &key, &upload, local, size, part, opts.checksum, progress);
    match sent {
        Ok(sha256) => Ok(Moved { bytes: size, sha256, verified: opts.checksum }),
        Err(e) => {
            // Even when stopped or timed out: leave no half-made upload behind.
            let never = AtomicBool::new(false);
            if let Ok(clean) = Ctx::new(target, &Options { timeout: Duration::from_secs(30), ..*opts }, &never) {
                let _ = clean.call("DELETE", &key, &[("uploadId", upload)], &[], EMPTY_SHA, &[]).and_then(|r| clean.ok(r));
            }
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn send_parts(
    ctx: &Ctx,
    key: &str,
    upload: &str,
    local: &Path,
    size: u64,
    part: u64,
    checksum: bool,
    progress: Progress,
) -> Result<String, Error> {
    let mut file = File::open(local).map_err(io_err)?;
    let count = size.div_ceil(part);
    let mut etags = Vec::with_capacity(count as usize);
    let mut sums: Vec<String> = Vec::new();
    let mut whole = Sha256::new();
    let mut buf = Vec::new();
    for n in 1..=count {
        ctx.check()?;
        buf.clear();
        (&mut file).take(part).read_to_end(&mut buf).map_err(io_err)?;
        whole.update(&buf);
        let hash = sha256_hex(&buf);
        let sum = base64(&sha256_raw(&buf));
        let extra: Vec<(&str, String)> = if checksum { vec![("x-amz-checksum-sha256", sum.clone())] } else { Vec::new() };
        if checksum {
            sums.push(sum);
        }
        let query = [("partNumber", n.to_string()), ("uploadId", upload.to_owned())];
        let etag = ctx.retry(|| {
            let r = ctx.call("PUT", key, &query, &buf, &hash, &extra).and_then(|r| ctx.ok(r))?;
            match r.headers().get("etag").and_then(|v| v.to_str().ok()) {
                Some(e) => Ok(e.to_owned()),
                None => Err(Error::Network("the store sent no ETag for a part".into())),
            }
        })?;
        etags.push(etag);
        progress(((n - 1) * part + buf.len() as u64).min(size), size);
    }
    let mut body = String::from("<CompleteMultipartUpload>");
    for (i, e) in etags.iter().enumerate() {
        let sum = sums.get(i).map(|s| format!("<ChecksumSHA256>{s}</ChecksumSHA256>")).unwrap_or_default();
        body.push_str(&format!("<Part><PartNumber>{}</PartNumber><ETag>{}</ETag>{sum}</Part>", i + 1, xml::escape(e)));
    }
    body.push_str("</CompleteMultipartUpload>");
    let bytes = body.into_bytes();
    let hash = sha256_hex(&bytes);
    let mut r = ctx.call("POST", key, &[("uploadId", upload.to_owned())], &bytes, &hash, &[]).and_then(|r| ctx.ok(r))?;
    // S3 may answer 200 and put the failure in the body.
    let text = r.body_mut().read_to_string().map_err(net)?;
    if text.contains("<Error>") {
        return Err(Error::Status { code: 500, body: xml_error(&text) });
    }
    Ok(hex(&whole.finalize()))
}

/// Fetch `key` into `local`; returns what was written. The file is made whole or not at all (`.part`, then rename).
/// With [`Options::checksum`] a whole-object `x-amz-checksum-sha256` from the store is compared with the bytes
/// that arrived; a difference is [`Error::Mismatch`] and no file is left.
pub fn get(target: &Target, key: &str, local: &Path, opts: &Options, cancel: &AtomicBool, progress: Progress) -> Result<Moved, Error> {
    let ctx = Ctx::new(target, opts, cancel)?;
    ctx.check()?;
    if let Some(dir) = local.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    let mut part_name = local.file_name().ok_or_else(|| Error::Io("no file name".into()))?.to_os_string();
    part_name.push(".part");
    let part: PathBuf = local.with_file_name(part_name);
    let done = fetch(&ctx, key, &part, opts.checksum, progress);
    match done {
        Ok(n) => {
            if let Err(e) = fs::rename(&part, local) {
                let _ = fs::remove_file(&part);
                return Err(io_err(e));
            }
            Ok(n)
        }
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

fn fetch(ctx: &Ctx, key: &str, part: &Path, checksum: bool, progress: Progress) -> Result<Moved, Error> {
    let extra: Vec<(&str, String)> = if checksum { vec![("x-amz-checksum-mode", "ENABLED".to_owned())] } else { Vec::new() };
    let mut resp = ctx.call("GET", key, &[], &[], EMPTY_SHA, &extra).and_then(|r| ctx.ok(r))?;
    // A multipart object's checksum ends in `-<parts>` and is a checksum of checksums: it cannot be compared.
    let promised = resp
        .headers()
        .get("x-amz-checksum-sha256")
        .and_then(|v| v.to_str().ok())
        .filter(|v| checksum && !v.contains('-'))
        .map(str::to_owned);
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let mut out = File::create(part).map_err(io_err)?;
    let mut reader = resp.body_mut().as_reader();
    let mut buf = vec![0u8; CHUNK];
    let mut got = 0u64;
    let mut sha = Sha256::new();
    loop {
        ctx.check()?;
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(read_error(e)),
        };
        out.write_all(&buf[..n]).map_err(io_err)?;
        sha.update(&buf[..n]);
        got += n as u64;
        progress(got, total.unwrap_or(got));
    }
    out.flush().map_err(io_err)?;
    if let Some(t) = total {
        if t != got {
            return Err(Error::Network(format!("the body ended after {got} of {t} bytes")));
        }
    }
    let digest = sha.finalize();
    if let Some(p) = &promised {
        if *p != base64(&digest) {
            return Err(Error::Mismatch(format!("the store's SHA-256 is {p} but {got} bytes arrived with {}", base64(&digest))));
        }
    }
    Ok(Moved { bytes: got, sha256: hex(&digest), verified: promised.is_some() })
}

/// The objects whose keys match `pattern` (`*` and `?`; no wildcard means that one key). Nothing matching is an empty list.
pub fn list(target: &Target, pattern: &str, opts: &Options, cancel: &AtomicBool) -> Result<Vec<Entry>, Error> {
    let ctx = Ctx::new(target, opts, cancel)?;
    let pattern = pattern.trim_start_matches('/');
    let prefix = &pattern[..pattern.find(['*', '?']).unwrap_or(pattern.len())];
    let mut found = Vec::new();
    let mut token: Option<String> = None;
    loop {
        ctx.check()?;
        let mut query = vec![("list-type", "2".to_owned()), ("prefix", prefix.to_owned())];
        if let Some(t) = &token {
            query.push(("continuation-token", t.clone()));
        }
        let mut r = ctx.call("GET", "", &query, &[], EMPTY_SHA, &[]).and_then(|r| ctx.ok(r))?;
        let text = r.body_mut().read_to_string().map_err(net)?;
        for c in xml::blocks(&text, "Contents") {
            let Some(key) = xml::first(c, "Key") else { continue };
            if !ito_match::glob_match(pattern, &key, false) {
                continue;
            }
            let size = xml::first(c, "Size").and_then(|s| s.parse().ok()).unwrap_or(0);
            let modified = xml::first(c, "LastModified").and_then(|s| sign::parse_iso(&s)).map(|t| UNIX_EPOCH + Duration::from_secs(t));
            found.push(Entry { key, size, modified });
        }
        token = match (xml::first(&text, "IsTruncated").as_deref(), xml::first(&text, "NextContinuationToken")) {
            (Some("true"), Some(t)) if !t.is_empty() => Some(t),
            _ => break,
        };
    }
    Ok(found)
}

/// Delete `key`. A key that is not there is not an error.
pub fn delete(target: &Target, key: &str, opts: &Options, cancel: &AtomicBool) -> Result<(), Error> {
    let ctx = Ctx::new(target, opts, cancel)?;
    ctx.check()?;
    let key = key.trim_start_matches('/');
    if key.is_empty() || key.ends_with('/') {
        return Err(Error::Io("a key is needed".into()));
    }
    ctx.call("DELETE", key, &[], &[], EMPTY_SHA, &[]).and_then(|r| ctx.ok(r)).map(drop)
}

fn full_key(key: &str, local: &Path) -> Result<String, Error> {
    let key = key.trim_start_matches('/');
    if key.is_empty() || key.ends_with('/') {
        let name = local.file_name().and_then(|n| n.to_str()).ok_or_else(|| Error::Io("the file has no name".into()))?;
        Ok(format!("{key}{name}"))
    } else {
        Ok(key.to_owned())
    }
}

const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

type Response = ureq::http::Response<ureq::Body>;

/// One call's connection, stop flag and clock.
struct Ctx<'a> {
    agent: ureq::Agent,
    cancel: &'a AtomicBool,
    deadline: Instant,
    request_timeout: Duration,
    bucket: &'a str,
    region: &'a str,
    key_id: &'a str,
    secret: &'a str,
    token: Option<&'a str>,
    /// `https://host[:port]` or `http://…`, without the bucket for a virtual-host store.
    scheme: &'static str,
    authority: String,
    path_style: bool,
}

impl<'a> Ctx<'a> {
    fn new(target: &'a Target, opts: &Options, cancel: &'a AtomicBool) -> Result<Self, Error> {
        let Target::S3 { bucket, region, endpoint, path_style, key_id, secret, session_token } = target;
        if bucket.is_empty() || region.is_empty() {
            return Err(Error::Io("the bucket and the region are needed".into()));
        }
        let (scheme, authority, path_style) = match endpoint.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
            Some(e) => {
                let (scheme, rest) = match e.split_once("://") {
                    Some(("http", rest)) => ("http", rest),
                    Some(("https", rest)) => ("https", rest),
                    Some((other, _)) => return Err(Error::Io(format!("the endpoint scheme `{other}` is not http or https"))),
                    None => ("https", e),
                };
                let authority = rest.split('/').next().unwrap_or("").to_owned();
                if authority.is_empty() {
                    return Err(Error::Io("the endpoint has no host".into()));
                }
                (scheme, authority, *path_style)
            }
            None => ("https", format!("s3.{region}.amazonaws.com"), *path_style),
        };
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(concat!("ito-cloud/", env!("CARGO_PKG_VERSION")))
            .build();
        Ok(Ctx {
            agent: ureq::Agent::new_with_config(config),
            cancel,
            deadline: Instant::now() + opts.timeout,
            request_timeout: opts.request_timeout,
            bucket,
            region,
            key_id,
            secret,
            token: session_token.as_deref(),
            scheme,
            authority,
            path_style,
        })
    }

    fn check(&self) -> Result<(), Error> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(Error::Timeout)
        } else {
            Ok(())
        }
    }

    /// Run `f`, again up to [`RETRIES`] times (after 1 s, 2 s, 3 s that the stop flag cuts short) while its error may pass.
    fn retry<T>(&self, mut f: impl FnMut() -> Result<T, Error>) -> Result<T, Error> {
        let mut attempt = 0;
        loop {
            match f() {
                Err(e) if e.retryable() && attempt < RETRIES => {
                    attempt += 1;
                    let until = Instant::now() + Duration::from_millis(1000 * u64::from(attempt) / retry_speed());
                    while Instant::now() < until {
                        self.check()?;
                        std::thread::sleep(Duration::from_millis(20));
                    }
                }
                other => return other,
            }
        }
    }

    /// One signed request. `key` is empty for the bucket itself.
    fn call(
        &self,
        method: &str,
        key: &str,
        query: &[(&str, String)],
        body: &[u8],
        payload_hash: &str,
        extra: &[(&str, String)],
    ) -> Result<Response, Error> {
        self.check()?;
        let (host, path) = if self.path_style {
            (
                self.authority.clone(),
                if key.is_empty() {
                    format!("/{}", encode(self.bucket, false))
                } else {
                    format!("/{}/{}", encode(self.bucket, false), encode(key, true))
                },
            )
        } else {
            (format!("{}.{}", self.bucket, self.authority), format!("/{}", encode(key, true)))
        };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let amz_date = sign::amz_date(now);
        let mut headers: Vec<(&str, String)> =
            vec![("host", host.clone()), ("x-amz-content-sha256", payload_hash.to_owned()), ("x-amz-date", amz_date.clone())];
        if let Some(t) = self.token {
            headers.push(("x-amz-security-token", t.to_owned()));
        }
        headers.extend(extra.iter().map(|(k, v)| (*k, v.clone())));
        let auth = authorization(
            &Keys { key_id: self.key_id, secret: self.secret, region: self.region },
            method,
            &path,
            query,
            &headers,
            payload_hash,
            &amz_date,
        );
        let mut url = format!("{}://{host}{path}", self.scheme);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&canonical_query(query));
        }
        let mut req = ureq::http::Request::builder().method(method).uri(url);
        for (k, v) in headers.iter().filter(|(k, _)| *k != "host") {
            req = req.header(*k, v.as_str());
        }
        let req = req.header("authorization", auth).body(body).map_err(|e| Error::Io(e.to_string()))?;
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        let timeout = self.request_timeout.min(remaining).max(Duration::from_millis(1));
        let req = self.agent.configure_request(req).timeout_global(Some(timeout)).build();
        self.agent.run(req).map_err(net)
    }

    /// The response when it is a success; otherwise the error it stands for.
    fn ok(&self, mut r: Response) -> Result<Response, Error> {
        let code = r.status().as_u16();
        if (200..300).contains(&code) {
            return Ok(r);
        }
        let text = r.body_mut().read_to_string().unwrap_or_default();
        let msg = xml_error(&text);
        Err(match code {
            404 => Error::NotFound,
            400 if msg.starts_with("BadDigest") || msg.starts_with("XAmzContentSHA256Mismatch") || msg.starts_with("InvalidDigest") => {
                Error::Mismatch(msg)
            }
            401 | 403 => Error::Auth(msg),
            _ => Error::Status { code, body: msg },
        })
    }

    fn create_upload(&self, key: &str, checksum: bool) -> Result<String, Error> {
        let extra: Vec<(&str, String)> = if checksum { vec![("x-amz-checksum-algorithm", "SHA256".to_owned())] } else { Vec::new() };
        let mut r =
            self.retry(|| self.call("POST", key, &[("uploads", String::new())], &[], EMPTY_SHA, &extra).and_then(|r| self.ok(r)))?;
        let text = r.body_mut().read_to_string().map_err(net)?;
        xml::first(&text, "UploadId").filter(|s| !s.is_empty()).ok_or_else(|| Error::Network("the store sent no UploadId".into()))
    }
}

/// The wait before a retry is divided by this in tests so they do not sleep for seconds.
fn retry_speed() -> u64 {
    if cfg!(test) {
        50
    } else {
        1
    }
}

/// `Code: Message` of an S3 error body, or the first 300 characters of whatever it is.
fn xml_error(text: &str) -> String {
    match (xml::first(text, "Code"), xml::first(text, "Message")) {
        (Some(c), Some(m)) => format!("{c}: {m}"),
        (Some(c), None) => c,
        _ => text.trim().chars().take(300).collect(),
    }
}

fn net(e: ureq::Error) -> Error {
    match e {
        ureq::Error::Timeout(_) => Error::Timeout,
        ureq::Error::Tls(m) => Error::Tls(m.to_owned()),
        ureq::Error::Rustls(m) => Error::Tls(m.to_string()),
        ureq::Error::Io(e) if e.kind() == std::io::ErrorKind::TimedOut => Error::Timeout,
        other => Error::Network(other.to_string()),
    }
}

/// An error out of a body reader (`ureq` wraps its own error in an `io::Error`).
fn read_error(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::TimedOut {
        return Error::Timeout;
    }
    match e.into_inner().map(|inner| inner.downcast::<ureq::Error>()) {
        Some(Ok(u)) => net(*u),
        Some(Err(other)) => Error::Network(other.to_string()),
        None => Error::Network("the connection broke".into()),
    }
}

#[cfg(test)]
mod tests;
