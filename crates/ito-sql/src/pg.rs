//! PostgreSQL through tokio-postgres (rustls, ring).

use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring, CryptoProvider};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_postgres::SimpleQueryMessage;

use crate::{Clock, Error, Last, Outcome, Target, Tls};

pub(crate) fn run(target: &Target, batches: &[String], clock: &Clock, last: &mut Last, outcome: &mut Outcome) -> Result<(), Error> {
    let Target::Postgres { host, port, database, user, password, tls } = target else { unreachable!() };
    let mut config = tokio_postgres::Config::new();
    config.host(host).port(*port).dbname(database).user(user).password(password);
    clock.block_on(async {
        let client = match tls {
            Tls::Disable => {
                let (client, conn) = config.connect(tokio_postgres::NoTls).await.map_err(connect_error)?;
                tokio::spawn(conn);
                client
            }
            Tls::Require | Tls::Verify => {
                let connector = tokio_postgres_rustls::MakeRustlsConnect::new(client_config(*tls == Tls::Verify)?);
                let (client, conn) = config.connect(connector).await.map_err(connect_error)?;
                tokio::spawn(conn);
                client
            }
        };
        for batch in batches {
            let messages = client.simple_query(batch).await.map_err(sql_error)?;
            let mut described = false;
            for m in messages {
                match m {
                    SimpleQueryMessage::RowDescription(cols) => {
                        described = true;
                        let names: Vec<String> = cols.iter().map(|c| c.name().to_owned()).collect();
                        last.begin(&names);
                    }
                    SimpleQueryMessage::Row(row) => {
                        last.row((0..row.len()).map(|i| row.get(i).map(str::to_owned)));
                    }
                    // A SELECT also reports how many rows it returned; those are not changed rows.
                    SimpleQueryMessage::CommandComplete(n) => {
                        if !described {
                            outcome.rows_affected += n;
                        }
                        described = false;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })
}

fn connect_error(e: tokio_postgres::Error) -> Error {
    match e.as_db_error() {
        Some(db) => Error::Connect(db.message().to_owned()),
        None => Error::Connect(error_chain(&e)),
    }
}

fn sql_error(e: tokio_postgres::Error) -> Error {
    match e.as_db_error() {
        Some(db) => Error::Sql(db.message().to_owned()),
        None => Error::Sql(error_chain(&e)),
    }
}

fn error_chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut src = e.source();
    while let Some(inner) = src {
        s.push_str(": ");
        s.push_str(&inner.to_string());
        src = inner.source();
    }
    s
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring::default_provider())
}

fn client_config(verify: bool) -> Result<ClientConfig, Error> {
    let builder = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::Connect(format!("TLS: {e}")))?;
    Ok(if verify {
        let roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        builder.with_root_certificates(roots).with_no_client_auth()
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(AnyCertificate(provider()))).with_no_client_auth()
    })
}

/// `sslmode=require`: encrypt, but do not check who answers.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
