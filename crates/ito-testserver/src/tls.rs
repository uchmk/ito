//! A throwaway self-signed certificate for the TLS modes, and the client side that trusts exactly it.

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

/// A certificate's SHA-256.
pub type Fingerprint = [u8; 32];

/// `AB:CD:...` (upper case, colons).
pub fn format(fp: &Fingerprint) -> String {
    fp.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A self-signed certificate for `localhost` and `127.0.0.1`, made in memory.
#[derive(Clone)]
pub struct Cert {
    der: Vec<u8>,
    config: Arc<rustls::ServerConfig>,
}

impl Cert {
    pub fn generate() -> Cert {
        let made = rcgen::generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()]).expect("a certificate");
        let (der, key) = (made.cert.der().to_vec(), made.signing_key.serialize_der());
        let config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(der.clone())], PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key)))
            .expect("a server config");
        Cert { der, config: Arc::new(config) }
    }

    /// The certificate, DER.
    pub fn der(&self) -> &[u8] {
        &self.der
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Sha256::digest(&self.der).into()
    }

    pub(crate) fn server_config(&self) -> Arc<rustls::ServerConfig> {
        self.config.clone()
    }
}

/// A client config that accepts the one certificate with this fingerprint and nothing else (no CA, no name check):
/// for a test that speaks TLS to a server of this crate by hand.
pub fn pinned_client_config(fingerprint: Fingerprint) -> Arc<rustls::ClientConfig> {
    let p = provider();
    let verifier = Pin { want: fingerprint, algs: p.signature_verification_algorithms };
    let config = rustls::ClientConfig::builder_with_provider(p)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Arc::new(config)
}

#[derive(Debug)]
struct Pin {
    want: Fingerprint,
    algs: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let got: Fingerprint = Sha256::digest(end_entity).into();
        if got == self.want {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("certificate fingerprint differs".to_owned()))
        }
    }

    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &CertificateDer<'_>,
        d: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}
