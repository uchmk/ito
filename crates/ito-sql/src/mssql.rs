//! SQL Server through tiberius (TDS over rustls).

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use crate::{Clock, Error, Last, Outcome, SqlAuth, Target};

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
    let addr = config.get_addr();
    clock.block_on(async {
        let tcp = TcpStream::connect(addr).await.map_err(|e| Error::Connect(e.to_string()))?;
        let _ = tcp.set_nodelay(true);
        let mut client = Client::connect(config, tcp.compat_write()).await.map_err(connect_error)?;
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

/// Whether the batch may give rows back (it has `SELECT` or `EXEC` as a word).
fn returns_rows(batch: &str) -> bool {
    batch
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w.eq_ignore_ascii_case("select") || w.eq_ignore_ascii_case("exec") || w.eq_ignore_ascii_case("execute"))
}

fn connect_error(e: tiberius::error::Error) -> Error {
    Error::Connect(message(&e))
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
    use super::returns_rows;

    #[test]
    fn rows_are_expected_from_select_and_exec() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("exec sp_who"));
        assert!(!returns_rows("update t set selected = 1"));
        assert!(!returns_rows("insert into t values (1)"));
    }
}
