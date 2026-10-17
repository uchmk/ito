//! One message on the wire: its length as a little-endian `u32`, then the
//! message in postcard (serde's compact binary form: varints, no field names).

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Larger than any screen (a 500 x 200 grid of cells is under 10 MB): a
/// length past this is a stream out of step, not a message.
const MAX: u32 = 64 << 20;

/// The largest body `write` and `read` accept.
pub const MAX_BODY: u32 = MAX;

/// The whole frame (length, then body) of `msg`, to encode once and send the
/// same bytes to many peers.
pub fn frame<T: Serialize>(msg: &T) -> io::Result<Vec<u8>> {
    frame_max(msg, MAX)
}

/// `frame` with a body no longer than `max` bytes.
pub fn frame_max<T: Serialize>(msg: &T, max: u32) -> io::Result<Vec<u8>> {
    let body = postcard::to_allocvec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(body.len()).ok().filter(|&n| n <= max).ok_or_else(|| io::Error::other("message too large"))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

pub fn write<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    w.write_all(&frame(msg)?)?;
    w.flush()
}

/// The body of the next frame, not yet decoded; `InvalidData` when its length
/// is past `max`, `UnexpectedEof` when the other end has gone.
pub fn read_body_max<R: Read>(r: &mut R, max: u32) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a message of {len} bytes")));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body)?;
    Ok(body)
}

/// `read_body_max` with the usual limit.
pub fn read_body<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    read_body_max(r, MAX)
}

/// A body read by `read_body`, as a message.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> io::Result<T> {
    postcard::from_bytes(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// The first two numbers of a body: the variant index of an enum message and
/// the varint after it (a protocol number, say), whatever version sent it, so
/// an old or foreign peer can be refused before the rest is decoded. `None`
/// when the body is too short or a number is not a varint that fits.
pub fn head(body: &[u8]) -> Option<(u32, u16)> {
    let (variant, rest) = varint(body)?;
    let (next, _) = varint(rest)?;
    Some((u32::try_from(variant).ok()?, u16::try_from(next).ok()?))
}

fn varint(b: &[u8]) -> Option<(u64, &[u8])> {
    let mut v = 0u64;
    for (i, &x) in b.iter().enumerate().take(10) {
        v |= u64::from(x & 0x7f) << (7 * i);
        if x & 0x80 == 0 {
            return Some((v, &b[i + 1..]));
        }
    }
    None
}

/// The next message, or `UnexpectedEof` when the other end has gone.
pub fn read<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<T> {
    decode(&read_body(r)?)
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

    #[test]
    fn a_frame_is_what_write_puts_on_the_wire() {
        let msg = Msg::Input { id: 300, bytes: vec![1, 2] };
        let mut buf = Vec::new();
        write(&mut buf, &msg).unwrap();
        assert_eq!(frame(&msg).unwrap(), buf);
        let body = read_body(&mut &buf[..]).unwrap();
        assert_eq!(decode::<Msg>(&body).unwrap(), msg);
    }

    #[test]
    fn a_caller_chosen_limit_refuses_early() {
        let buf = frame(&Msg::Input { id: 1, bytes: vec![0; 100] }).unwrap();
        assert_eq!(read_body_max(&mut &buf[..], 50).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(frame_max(&Msg::Input { id: 1, bytes: vec![0; 100] }, 50).is_err());
    }

    #[test]
    fn head_reads_the_variant_and_the_number_after_it() {
        let body = postcard::to_allocvec(&Msg::Hello { version: 3 }).unwrap();
        assert_eq!(head(&body), Some((0, 3)));
        let body = postcard::to_allocvec(&Msg::Input { id: 7, bytes: vec![] }).unwrap();
        assert_eq!(head(&body), Some((1, 7)));
        assert_eq!(head(&[]), None);
        assert_eq!(head(&[0x80]), None);
        assert_eq!(head(&[1]), None);
    }
}
