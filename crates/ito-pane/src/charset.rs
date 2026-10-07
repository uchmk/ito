//! A pane's character set, when it is not UTF-8: what the program writes is
//! turned into UTF-8 before the parser reads it, and what is typed is turned
//! back. For a file in Shift_JIS shown with `cat`, or an old machine reached
//! with `ssh`. Windows has no use for it: ConPTY hands over UTF-8 whatever the
//! console's code page.

use std::sync::{Arc, Mutex};

use encoding_rs::{Decoder, Encoding};

/// The character sets offered, by the names `set` takes.
pub const CHARSETS: [&str; 8] = ["UTF-8", "Shift_JIS", "EUC-JP", "ISO-2022-JP", "GBK", "Big5", "EUC-KR", "windows-1252"];

/// The pane's character set, shared by the reader thread and the pane.
#[derive(Clone, Default)]
pub struct Charset(Arc<Mutex<Option<(&'static Encoding, Decoder)>>>);

impl Charset {
    /// Use `name` (one of [`CHARSETS`], any case); UTF-8 reads and writes the
    /// bytes as they are. `false` for a name not known.
    pub fn set(&self, name: &str) -> bool {
        let Some(enc) = Encoding::for_label(name.trim().as_bytes()) else { return false };
        let mut st = self.0.lock().unwrap_or_else(|e| e.into_inner());
        *st = (enc != encoding_rs::UTF_8).then(|| (enc, enc.new_decoder_without_bom_handling()));
        true
    }

    /// Its name: `UTF-8`, `Shift_JIS`.
    pub fn name(&self) -> &'static str {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map_or("UTF-8", |(e, _)| e.name())
    }

    /// Read from `read` into `buf` as UTF-8. A character cut by the read
    /// waits in the decoder for the next; a read that gave only such a part
    /// reads again, so 0 still means the end.
    pub fn read(&self, buf: &mut [u8], mut read: impl FnMut(&mut [u8]) -> std::io::Result<usize>) -> std::io::Result<usize> {
        let mut st = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some((_, decoder)) = st.as_mut() else { return read(buf) };
        // Few enough bytes that their UTF-8 surely fits in `buf`.
        let mut want = (buf.len() / 4).max(1);
        while want > 1 && decoder.max_utf8_buffer_length(want).is_none_or(|n| n > buf.len()) {
            want /= 2;
        }
        let mut raw = vec![0u8; want];
        loop {
            let n = read(&mut raw)?;
            if n == 0 {
                return Ok(0);
            }
            let (_, _, written, _) = decoder.decode_to_utf8(&raw[..n], buf, false);
            if written > 0 {
                return Ok(written);
            }
        }
    }

    /// What is typed, in the pane's character set: UTF-8 text turned, any
    /// other bytes as they are.
    pub fn encode(&self, bytes: Vec<u8>) -> Vec<u8> {
        let st = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let Some((enc, _)) = st.as_ref() else { return bytes };
        match std::str::from_utf8(&bytes) {
            Ok(text) => encode_or_question(enc, text),
            Err(_) => bytes,
        }
    }
}

/// `text` in `enc`, a character it has no code for as `?`: encoding_rs's
/// own `encode` writes `&#128512;` there, which a program reads as typed
/// (the source review, 2026-10-07).
fn encode_or_question(enc: &'static encoding_rs::Encoding, text: &str) -> Vec<u8> {
    use encoding_rs::EncoderResult;
    let mut encoder = enc.new_encoder();
    let mut out = Vec::with_capacity(text.len() * 2 + 8);
    let mut rest = text;
    let mut buf = [0u8; 1024];
    loop {
        let (result, read, written) = encoder.encode_from_utf8_without_replacement(rest, &mut buf, true);
        out.extend_from_slice(&buf[..written]);
        rest = &rest[read..];
        match result {
            EncoderResult::InputEmpty => return out,
            EncoderResult::OutputFull => {}
            EncoderResult::Unmappable(_) => out.push(b'?'),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shift_JIS read in pieces, a character cut in two between them.
    #[test]
    fn shift_jis_comes_out_as_utf8_and_goes_back() {
        let c = Charset::default();
        assert_eq!(c.name(), "UTF-8");
        assert!(c.set("shift_jis") && c.name() == "Shift_JIS");
        assert!(!c.set("no-such"));
        let sjis = encoding_rs::SHIFT_JIS.encode("日本語 ok\r\n").0.into_owned();
        let mut chunks = vec![sjis[..1].to_vec(), sjis[1..3].to_vec(), sjis[3..].to_vec()].into_iter();
        let mut out = Vec::new();
        let mut buf = [0u8; 64];
        loop {
            let n = c.read(&mut buf, |raw| {
                let Some(next) = chunks.next() else { return Ok(0) };
                raw[..next.len()].copy_from_slice(&next);
                Ok(next.len())
            }).unwrap();
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        assert_eq!(String::from_utf8(out).unwrap(), "日本語 ok\r\n");
        assert_eq!(c.encode("日本".as_bytes().to_vec()), encoding_rs::SHIFT_JIS.encode("日本").0.into_owned());
        // A character Shift_JIS has no code for goes as ?, not &#128512;.
        let mut want = encoding_rs::SHIFT_JIS.encode("日").0.into_owned();
        want.extend_from_slice(b"?x");
        assert_eq!(c.encode("日😀x".as_bytes().to_vec()), want);
        assert_eq!(c.encode(vec![0x1b, b'[', b'A']), vec![0x1b, b'[', b'A'], "keys stay as they are");
        assert!(c.set("UTF-8"));
        assert_eq!(c.encode("日本".as_bytes().to_vec()), "日本".as_bytes());
    }
}
