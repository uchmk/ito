//! The listener's certificate: self-signed, made on first use, kept in a folder the app names, and known by its
//! SHA-256 fingerprint. There is no CA; a client trusts the fingerprint the person was shown (see `client`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

const CERT_FILE: &str = "ito-http-cert.der";
const KEY_FILE: &str = "ito-http-key.der";

/// Where the certificate and its key are kept.
#[derive(Clone, Debug)]
pub struct TlsCfg {
    /// Made if it does not exist. The key in it is a secret: keep it in the app's own state folder.
    pub cert_dir: PathBuf,
}

impl TlsCfg {
    pub fn new(cert_dir: impl Into<PathBuf>) -> Self {
        TlsCfg { cert_dir: cert_dir.into() }
    }
}

/// A certificate's SHA-256.
pub type Fingerprint = [u8; 32];

pub(crate) fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The SHA-256 of a DER certificate.
pub fn sha256(der: &[u8]) -> Fingerprint {
    Sha256::digest(der).into()
}

/// `AB:CD:…` (upper case, colons), the way a person compares fingerprints.
pub fn format(fp: &Fingerprint) -> String {
    fp.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

/// A fingerprint written with or without colons, spaces or dashes, in either case. `None` when it is not 32 bytes of hex.
pub fn parse(s: &str) -> Option<Fingerprint> {
    let hex: Vec<u8> = s.bytes().filter(|b| !matches!(b, b':' | b' ' | b'-')).collect();
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in hex.chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

/// The certificate in `cert_dir` (made first if there is none) and its key.
pub(crate) struct Identity {
    pub cert: Vec<u8>,
    key: Vec<u8>,
}

impl Identity {
    pub fn fingerprint(&self) -> Fingerprint {
        sha256(&self.cert)
    }

    pub fn server_config(&self) -> io::Result<Arc<rustls::ServerConfig>> {
        let bad = |e: rustls::Error| io::Error::new(io::ErrorKind::InvalidData, e.to_string());
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(bad)?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert.clone())],
                PrivateKeyDer::from(PrivatePkcs8KeyDer::from(self.key.clone())),
            )
            .map_err(bad)?;
        Ok(Arc::new(config))
    }
}

/// Read the certificate in `cert_dir`, or make one when it (or its key) is missing.
pub(crate) fn load_or_create(cfg: &TlsCfg) -> io::Result<Identity> {
    let (cert_path, key_path) = (cfg.cert_dir.join(CERT_FILE), cfg.cert_dir.join(KEY_FILE));
    if let (Ok(cert), Ok(key)) = (fs::read(&cert_path), fs::read(&key_path)) {
        if !cert.is_empty() && !key.is_empty() {
            return Ok(Identity { cert, key });
        }
    }
    fs::create_dir_all(&cfg.cert_dir)?;
    let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "ito-http".to_owned()])
        .map_err(|e| io::Error::other(format!("cannot make a certificate: {e}")))?;
    let (cert, key) = (made.cert.der().to_vec(), made.signing_key.serialize_der());
    // The key first: a certificate without its key is useless, a key without a certificate is made again.
    write_private(&key_path, &key)?;
    write_private(&cert_path, &cert)?;
    Ok(Identity { cert, key })
}

/// The fingerprint of the certificate in `cert_dir`, made if there is none yet. What the person is shown, and
/// what another PC pins.
pub fn fingerprint(cert_dir: &Path) -> io::Result<Fingerprint> {
    Ok(load_or_create(&TlsCfg::new(cert_dir))?.fingerprint())
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
    }
    #[cfg(not(unix))]
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, path)
}
