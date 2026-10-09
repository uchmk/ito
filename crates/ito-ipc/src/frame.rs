//! One message on the wire: its length as a little-endian `u32`, then the
//! message in postcard (serde's compact binary form: varints, no field names).

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Larger than any screen (a 500 x 200 grid of cells is under 10 MB): a
/// length past this is a stream out of step, not a message.
const MAX: u32 = 64 << 20;

pub fn write<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let body = postcard::to_allocvec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(body.len()).ok().filter(|&n| n <= MAX).ok_or_else(|| io::Error::other("message too large"))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    w.write_all(&out)?;
    w.flush()
}

/// The next message, or `UnexpectedEof` when the other end has gone.
pub fn read<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a message of {len} bytes")));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    enum Msg {
        Hello { version: u32 },
        Input { id: u64, bytes: Vec<u8> },
    }

    #[test]
    fn a_message_comes_back_as_it_went() {
        let mut buf = Vec::new();
        write(&mut buf, &Msg::Hello { version: 3 }).unwrap();
        write(&mut buf, &Msg::Input { id: 7, bytes: b"ls\r".to_vec() }).unwrap();
        let mut r = &buf[..];
        assert_eq!(read::<_, Msg>(&mut r).unwrap(), Msg::Hello { version: 3 });
        assert_eq!(read::<_, Msg>(&mut r).unwrap(), Msg::Input { id: 7, bytes: b"ls\r".to_vec() });
        assert_eq!(read::<_, Msg>(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof, "then the end");
    }

    #[test]
    fn a_length_out_of_step_is_refused() {
        let mut r = &[0xff, 0xff, 0xff, 0xff, 1, 2][..];
        assert_eq!(read::<_, Msg>(&mut r).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
