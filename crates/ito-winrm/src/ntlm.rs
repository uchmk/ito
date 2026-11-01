//! NTLMv2 (MS-NLMP) for the client side of an HTTP `Negotiate` / `NTLM` sign-in: the three messages, no signing or
//! sealing (the caller runs over HTTPS), no MIC.

use hmac::{Hmac, KeyInit, Mac};
use md4::{Digest, Md4};
use md5::Md5;

const SIGNATURE: &[u8; 8] = b"NTLMSSP\0";

const UNICODE: u32 = 0x0000_0001;
const REQUEST_TARGET: u32 = 0x0000_0004;
const NTLM: u32 = 0x0000_0200;
const ALWAYS_SIGN: u32 = 0x0000_8000;
const EXTENDED_SECURITY: u32 = 0x0008_0000;
const TARGET_INFO: u32 = 0x0080_0000;
const BITS_128: u32 = 0x2000_0000;
const BITS_56: u32 = 0x8000_0000;

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut m = <Hmac<Md5> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// `user` as `DOMAIN\name` or plain (`name@realm` stays whole, with no domain).
pub(crate) fn split_user(user: &str) -> (&str, &str) {
    match user.split_once('\\') {
        Some((d, n)) => (d, n),
        None => ("", user),
    }
}

/// NTOWFv2: HMAC-MD5 of the upper-cased user and the domain, keyed by the MD4 of the password.
pub(crate) fn ntowf_v2(password: &str, user: &str, domain: &str) -> [u8; 16] {
    let nt: [u8; 16] = Md4::digest(utf16(password)).into();
    hmac_md5(&nt, &[&utf16(&user.to_uppercase()), &utf16(domain)])
}

/// Message 1.
pub(crate) fn negotiate() -> Vec<u8> {
    let flags = UNICODE | REQUEST_TARGET | NTLM | ALWAYS_SIGN | EXTENDED_SECURITY | BITS_128 | BITS_56;
    let mut m = SIGNATURE.to_vec();
    m.extend(1u32.to_le_bytes());
    m.extend(flags.to_le_bytes());
    // Domain and workstation: empty, pointing at the end of the 32-byte header.
    for _ in 0..2 {
        m.extend([0, 0, 0, 0]);
        m.extend(32u32.to_le_bytes());
    }
    m
}

/// What message 2 carries that message 3 needs.
pub(crate) struct Challenge {
    pub flags: u32,
    pub server: [u8; 8],
    pub target_info: Vec<u8>,
    /// MsvAvTimestamp, when the server sent one.
    pub timestamp: Option<[u8; 8]>,
}

fn u16_at(m: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(m.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(m: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(m.get(at..at + 4)?.try_into().ok()?))
}

/// Read message 2.
pub(crate) fn parse_challenge(m: &[u8]) -> Option<Challenge> {
    if m.get(..8)? != SIGNATURE || u32_at(m, 8)? != 2 {
        return None;
    }
    let flags = u32_at(m, 20)?;
    let server: [u8; 8] = m.get(24..32)?.try_into().ok()?;
    let target_info = if flags & TARGET_INFO != 0 {
        let (len, off) = (u16_at(m, 40)?, u32_at(m, 44)? as usize);
        m.get(off..off.checked_add(len)?)?.to_vec()
    } else {
        vec![]
    };
    // AV pairs: id u16, len u16, value. 7 is the timestamp.
    let mut timestamp = None;
    let mut i = 0;
    while i + 4 <= target_info.len() {
        let (id, len) = (u16_at(&target_info, i)?, u16_at(&target_info, i + 2)?);
        if id == 0 {
            break;
        }
        if id == 7 && len == 8 {
            timestamp = target_info.get(i + 4..i + 12).and_then(|t| t.try_into().ok());
        }
        i += 4 + len;
    }
    Some(Challenge { flags, server, target_info, timestamp })
}

/// Message 3. `now`: Windows FILETIME (100 ns since 1601), used when the server sent no timestamp.
pub(crate) fn authenticate(c: &Challenge, user: &str, password: &str, client: [u8; 8], now: u64) -> Vec<u8> {
    let (domain, name) = split_user(user);
    let key = ntowf_v2(password, name, domain);
    let ts = c.timestamp.unwrap_or_else(|| now.to_le_bytes());
    let mut blob = vec![1, 1, 0, 0, 0, 0, 0, 0];
    blob.extend(ts);
    blob.extend(client);
    blob.extend([0; 4]);
    blob.extend(&c.target_info);
    blob.extend([0; 4]);
    let proof = hmac_md5(&key, &[&c.server, &blob]);
    let mut nt = proof.to_vec();
    nt.extend(&blob);
    // With a server timestamp the LM response is zeros (the NT response says it all).
    let lm = if c.timestamp.is_some() {
        vec![0; 24]
    } else {
        let mut v = hmac_md5(&key, &[&c.server, &client]).to_vec();
        v.extend(client);
        v
    };
    let (domain, name) = (utf16(domain), utf16(name));
    let fields: [&[u8]; 6] = [&lm, &nt, &domain, &name, b"", b""];
    let mut m = SIGNATURE.to_vec();
    m.extend(3u32.to_le_bytes());
    let mut at = 80u32;
    for f in fields {
        m.extend((f.len() as u16).to_le_bytes());
        m.extend((f.len() as u16).to_le_bytes());
        m.extend(at.to_le_bytes());
        at += f.len() as u32;
    }
    // Encrypted random session key: none.
    m.extend([0, 0, 0, 0]);
    m.extend(at.to_le_bytes());
    let flags = c.flags & (UNICODE | NTLM | ALWAYS_SIGN | EXTENDED_SECURITY | BITS_128 | BITS_56 | TARGET_INFO);
    m.extend(flags.to_le_bytes());
    m.extend([0; 8]);
    for f in fields {
        m.extend(f);
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn ntowf_v2_matches_the_spec_vector() {
        // MS-NLMP 4.2.4.1.1: User "User", Domain "Domain", Password "Password".
        assert_eq!(hex(&ntowf_v2("Password", "User", "Domain")), "0c868a403bfd7a93a3001ef22ef02e3f");
    }

    #[test]
    fn the_ntlmv2_proof_matches_the_spec_vector() {
        // MS-NLMP 4.2.4.2.2: time 0, client challenge "aa…", server challenge 0123456789abcdef,
        // target info = NbDomainName "Domain", NbComputerName "Server".
        let mut info = vec![];
        for (id, s) in [(2u16, "Domain"), (1, "Server")] {
            info.extend(id.to_le_bytes());
            info.extend(((s.len() * 2) as u16).to_le_bytes());
            info.extend(utf16(s));
        }
        info.extend([0; 4]);
        let c = Challenge {
            flags: UNICODE | NTLM | EXTENDED_SECURITY | TARGET_INFO,
            server: [1, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef],
            target_info: info,
            timestamp: None,
        };
        let m = authenticate(&c, "Domain\\User", "Password", [0xaa; 8], 0);
        // The NT response is the 16-byte proof first.
        let (len, off) = (u16_at(&m, 20).unwrap(), u32_at(&m, 24).unwrap() as usize);
        assert!(len > 16);
        assert_eq!(hex(&m[off..off + 16]), "68cd0ab851e51c96aabc927bebef6a1c");
    }

    #[test]
    fn a_challenge_is_read_with_its_timestamp() {
        let mut info = vec![];
        info.extend(7u16.to_le_bytes());
        info.extend(8u16.to_le_bytes());
        info.extend([9, 8, 7, 6, 5, 4, 3, 2]);
        info.extend([0; 4]);
        let mut m = SIGNATURE.to_vec();
        m.extend(2u32.to_le_bytes());
        m.extend([0; 8]); // target name
        m.extend((UNICODE | TARGET_INFO).to_le_bytes());
        m.extend([1, 2, 3, 4, 5, 6, 7, 8]);
        m.extend([0; 8]); // reserved
        m.extend((info.len() as u16).to_le_bytes());
        m.extend((info.len() as u16).to_le_bytes());
        m.extend(48u32.to_le_bytes());
        m.extend(&info);
        let c = parse_challenge(&m).unwrap();
        assert_eq!(c.server, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(c.timestamp, Some([9, 8, 7, 6, 5, 4, 3, 2]));
        assert_eq!(c.target_info, info);
        assert!(parse_challenge(b"nope").is_none());
    }

    #[test]
    fn the_user_is_split_at_the_backslash_only() {
        assert_eq!(split_user("CORP\\taro"), ("CORP", "taro"));
        assert_eq!(split_user("taro@corp.example"), ("", "taro@corp.example"));
        assert_eq!(split_user("taro"), ("", "taro"));
    }
}
