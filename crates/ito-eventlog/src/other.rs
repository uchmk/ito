//! macOS and Linux: syslog through `logger`, tagged with the source.

use std::process::{Command, Stdio};

use crate::{Filter, From, Level, Record, WhyNot};

#[derive(Debug)]
pub struct Source(String);

pub fn open(source: &str) -> Result<Source, WhyNot> {
    Ok(Source(source.to_owned()))
}

impl Source {
    pub fn write(&self, level: Level, id: u32, text: &str) -> Result<(), WhyNot> {
        let priority = match level {
            Level::Info => "user.info",
            Level::Warning => "user.warning",
            Level::Error => "user.err",
        };
        let status = Command::new("logger")
            .args(["-t", &self.0, "-p", priority])
            .arg(format!("[{id}] {text}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|_| WhyNot::Unsupported)?;
        if status.success() {
            Ok(())
        } else {
            Err(WhyNot::Failed(status.code().unwrap_or(-1) as u32))
        }
    }
}

#[derive(Debug)]
pub struct Reader;

pub fn reader(_channel: &str, _filter: &Filter, _from: From) -> Result<Reader, WhyNot> {
    Err(WhyNot::Unsupported)
}

impl Reader {
    pub fn poll(&mut self) -> Result<Vec<Record>, WhyNot> {
        Err(WhyNot::Unsupported)
    }

    pub fn position(&self) -> u64 {
        0
    }
}
