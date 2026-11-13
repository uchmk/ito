//! SQL Server through tiberius (TDS over rustls).

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::{Clock, ConnectKind, Error, Last, Outcome, SqlAuth, Target};

pub(crate) fn run(
    target: &Target,
    batches: &[String],
    want_rows: bool,
    clock: &Clock,
    last: &mut Last,
    outcome: &mut Outcome,
) -> Result<(), Error> {
    let Target::SqlServer { host, port, database, auth, trust_cert } = target else { unreachable!() };
    let mut config = Config::new();
    config.host(host);
    config.port(*port);
    config.database(database);
    match auth {
        SqlAuth::Login { user, password } => config.authentication(AuthMethod::sql_server(user, password)),
        #[cfg(windows)]
        SqlAuth::Windows => config.authentication(AuthMethod::Integrated),
        #[cfg(not(windows))]
        SqlAuth::Windows => return Err(Error::Unsupported("Windows authentication is for Windows only".into())),
    }
    if *trust_cert {
        config.trust_cert();
    }
    // tiberius' rustls would otherwise look for a process-wide provider; ours is ring.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let integrated = *auth == SqlAuth::Windows;
    clock.block_on(async {
        let mut client = connect(config, integrated).await?;
        for batch in batches {
            if want_rows && returns_rows(batch) {
                let mut stream = client.simple_query(batch.as_str()).await.map_err(sql_error)?;
                while let Some(item) = stream.try_next().await.map_err(sql_error)? {
                    match item {
                        QueryItem::Metadata(meta) => {
                            let names: Vec<String> = meta.columns().iter().map(|c| c.name().to_owned()).collect();
                            last.begin(&names);
                        }
                        QueryItem::Row(row) => last.row(row.into_iter().map(|d| show(&d))),
                    }
                }
            } else {
                let r = client.execute(batch.as_str(), &[]).await.map_err(sql_error)?;
                outcome.rows_affected += r.total();
            }
        }
        Ok(())
    })
}

/// Why one connect attempt failed: the TCP connection, or what tiberius did on it.
enum Attempt {
    Tcp(std::io::Error),
    Tds(tiberius::error::Error),
}

async fn attempt(config: &Config) -> Result<Client<tokio_util::compat::Compat<TcpStream>>, Attempt> {
    let tcp = TcpStream::connect(config.get_addr()).await.map_err(Attempt::Tcp)?;
    let _ = tcp.set_nodelay(true);
    Client::connect(config.clone(), tcp.compat_write()).await.map_err(Attempt::Tds)
}

/// Connect, and follow one redirect: Azure SQL's redirect connection policy answers the sign-in with
/// `Error::Routing { host, port }`, which means "connect there instead" (tiberius' README does the same). The routed
/// name is also the one the TLS check uses. A second redirect is given up on. (Needs an Azure SQL to run; no test.)
async fn connect(mut config: Config, integrated: bool) -> Result<Client<tokio_util::compat::Compat<TcpStream>>, Error> {
    let mut routed = false;
    loop {
        match attempt(&config).await {
            Ok(client) => return Ok(client),
            Err(Attempt::Tcp(e)) => return Err(Error::connect(ConnectKind::Unreachable, e.to_string())),
            Err(Attempt::Tds(tiberius::error::Error::Routing { host, port })) => {
                if routed {
                    return Err(Error::connect(ConnectKind::Other, format!("redirected twice (to {host}:{port})")));
                }
                routed = true;
                config.host(&host);
                config.port(port);
            }
            Err(Attempt::Tds(e)) => return Err(Error::connect(connect_kind(&e, integrated), message(&e))),
        }
    }
}

/// SQL Server error numbers the sign-in can fail with.
fn kind_for_code(code: u32) -> ConnectKind {
    match code {
        // login failed, untrusted domain, account disabled, account locked, password expired, password must change
        18456 | 18452 | 18470 | 18486 | 18487 | 18488 => ConnectKind::SignIn,
        // cannot open the database (also given when the login may not open it)
        4060 => ConnectKind::Missing,
        _ => ConnectKind::Other,
    }
}

/// Why a connect failed. tiberius turns only a certificate refusal into `Error::Tls`; every other handshake failure stays
/// an `Io` error, and tokio-rustls wraps rustls errors as `InvalidData`.
fn connect_kind(e: &tiberius::error::Error, integrated: bool) -> ConnectKind {
    use tiberius::error::{Error as E, IoErrorKind};
    match e {
        E::Tls(_) => ConnectKind::Tls,
        // With a SQL login an `InvalidData` is rustls' handshake error. With Windows sign-in, the sign-in steps also return
        // io errors through `?` (tiberius client/connection.rs) and winauth makes `InvalidData` itself, and tiberius drops
        // the source, so the phase cannot be told: Other.
        E::Io { kind: IoErrorKind::InvalidData, .. } => {
            if integrated {
                ConnectKind::Other
            } else {
                ConnectKind::Tls
            }
        }
        E::Io { .. } => ConnectKind::Unreachable,
        E::Server(t) => kind_for_code(t.code()),
        // With our features (no Kerberos / sspi-rs) tiberius has no integrated-auth error variant to map to SignIn.
        _ => ConnectKind::Other,
    }
}

/// Whether the batch may give rows back (it has `SELECT` or `EXEC` as a word).
fn returns_rows(batch: &str) -> bool {
    batch
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w.eq_ignore_ascii_case("select") || w.eq_ignore_ascii_case("exec") || w.eq_ignore_ascii_case("execute"))
}

fn sql_error(e: tiberius::error::Error) -> Error {
    Error::Sql(message(&e))
}

fn message(e: &tiberius::error::Error) -> String {
    match e {
        tiberius::error::Error::Server(t) => t.message().to_owned(),
        other => other.to_string(),
    }
}

/// A cell as text; `None` is NULL.
fn show(d: &ColumnData<'static>) -> Option<String> {
    Some(match d {
        ColumnData::U8(v) => (*v)?.to_string(),
        ColumnData::I16(v) => (*v)?.to_string(),
        ColumnData::I32(v) => (*v)?.to_string(),
        ColumnData::I64(v) => (*v)?.to_string(),
        ColumnData::F32(v) => (*v)?.to_string(),
        ColumnData::F64(v) => (*v)?.to_string(),
        ColumnData::Bit(v) => if (*v)? { "1" } else { "0" }.to_owned(),
        ColumnData::String(v) => v.as_ref()?.to_string(),
        ColumnData::Guid(v) => v.as_ref()?.to_string(),
        ColumnData::Binary(v) => format!("0x{}", v.as_ref()?.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        ColumnData::Numeric(v) => v.as_ref()?.to_string(),
        ColumnData::Xml(v) => v.as_ref()?.to_string(),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            NaiveDateTime::from_sql(d).ok().flatten()?.format("%Y-%m-%d %H:%M:%S%.f").to_string()
        }
        ColumnData::Date(_) => NaiveDate::from_sql(d).ok().flatten()?.format("%Y-%m-%d").to_string(),
        ColumnData::Time(_) => NaiveTime::from_sql(d).ok().flatten()?.format("%H:%M:%S%.f").to_string(),
        ColumnData::DateTimeOffset(_) => {
            chrono::DateTime::<chrono::Utc>::from_sql(d).ok().flatten()?.format("%Y-%m-%d %H:%M:%S%.f +00:00").to_string()
        }
    })
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use tiberius::error::{Error as E, IoErrorKind};

    use super::{connect_kind, kind_for_code, returns_rows};
    use crate::{ConnectKind, Error, SqlAuth, Target};

    #[test]
    fn server_error_numbers_are_told_apart() {
        for code in [18456, 18452, 18470, 18486, 18487, 18488] {
            assert_eq!(kind_for_code(code), ConnectKind::SignIn, "{code}");
        }
        assert_eq!(kind_for_code(4060), ConnectKind::Missing);
        assert_eq!(kind_for_code(53), ConnectKind::Other);
    }

    #[test]
    fn io_and_tls_errors_are_told_apart() {
        let io = |kind| E::Io { kind, message: "x".into() };
        assert_eq!(connect_kind(&io(IoErrorKind::ConnectionRefused), false), ConnectKind::Unreachable);
        assert_eq!(connect_kind(&io(IoErrorKind::InvalidData), false), ConnectKind::Tls);
        assert_eq!(connect_kind(&io(IoErrorKind::InvalidData), true), ConnectKind::Other);
        assert_eq!(connect_kind(&io(IoErrorKind::ConnectionReset), true), ConnectKind::Unreachable);
        assert_eq!(connect_kind(&E::Tls("bad certificate".into()), false), ConnectKind::Tls);
        assert_eq!(connect_kind(&E::Protocol("x".into()), false), ConnectKind::Other);
    }

    #[test]
    fn nothing_listening_is_unreachable() {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let t = Target::SqlServer {
            host: "127.0.0.1".into(),
            port,
            database: "d".into(),
            auth: SqlAuth::Login { user: "u".into(), password: "p".into() },
            trust_cert: true,
        };
        let e = crate::run(&t, "select 1", None, &AtomicBool::new(false), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(e, Error::Connect { kind: ConnectKind::Unreachable, .. }), "{e:?}");
        assert!(e.retryable());
    }

    #[test]
    fn rows_are_expected_from_select_and_exec() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("exec sp_who"));
        assert!(!returns_rows("update t set selected = 1"));
        assert!(!returns_rows("insert into t values (1)"));
    }
}
