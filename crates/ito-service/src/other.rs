//! Not Windows: there is no Service Control Manager.

use crate::{Error, Install, Status, Stop};
use std::time::Duration;

pub fn run(_name: &str, _main: Box<dyn FnOnce(Stop) -> i32 + Send>) -> Result<(), Error> {
    Err(Error::Unsupported)
}

pub fn install(_i: &Install) -> Result<(), Error> {
    Err(Error::Unsupported)
}

pub fn uninstall(_name: &str) -> Result<(), Error> {
    Err(Error::Unsupported)
}

pub fn start(_name: &str) -> Result<(), Error> {
    Err(Error::Unsupported)
}

pub fn stop(_name: &str, _wait: Duration) -> Result<(), Error> {
    Err(Error::Unsupported)
}

pub fn state(_name: &str) -> Result<Option<Status>, Error> {
    Err(Error::Unsupported)
}

pub fn read_password(_prompt: &str) -> Result<String, Error> {
    Err(Error::Unsupported)
}
