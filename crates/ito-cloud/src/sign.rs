//! AWS Signature Version 4 for S3, and the clock text it needs.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub(crate) fn sha256_raw(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

/// Standard base64 with padding (the form of `x-amz-checksum-sha256`).
pub(crate) fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut m = <HmacSha256 as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data.as_bytes());
    m.finalize().into_bytes().to_vec()
}

/// Percent-encode as SigV4 wants: letters, digits and `-_.~` stay; `/` stays only when `keep_slash`.
pub(crate) fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The query as it is written in the URL and in the canonical request: names sorted, both sides encoded.
pub(crate) fn canonical_query(query: &[(&str, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query.iter().map(|(k, v)| (encode(k, false), encode(v, false))).collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

pub(crate) struct Keys<'a> {
    pub key_id: &'a str,
    pub secret: &'a str,
    pub region: &'a str,
}

/// The `Authorization` header value. `headers` are every header to sign (names in lower case);
/// `path` is already encoded; `amz_date` is `YYYYMMDDTHHMMSSZ`.
pub(crate) fn authorization(
    keys: &Keys,
    method: &str,
    path: &str,
    query: &[(&str, String)],
    headers: &[(&str, String)],
    payload_hash: &str,
    amz_date: &str,
) -> String {
    let mut headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.trim())).collect();
    headers.sort();
    let canonical_headers: String = headers.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = headers.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    let canonical = format!("{method}\n{path}\n{}\n{canonical_headers}\n{signed}\n{payload_hash}", canonical_query(query));
    let date = &amz_date[..8];
    let scope = format!("{date}/{}/s3/aws4_request", keys.region);
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let k = hmac(format!("AWS4{}", keys.secret).as_bytes(), date);
    let k = hmac(&k, keys.region);
    let k = hmac(&k, "s3");
    let k = hmac(&k, "aws4_request");
    let signature = hex(&hmac(&k, &to_sign));
    format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={signature}", keys.key_id)
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYYMMDDTHHMMSSZ` for a Unix time.
pub(crate) fn amz_date(unix: u64) -> String {
    let (y, m, d) = civil_from_days((unix / 86400) as i64);
    let s = unix % 86400;
    format!("{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z", s / 3600, s % 3600 / 60, s % 60)
}

/// `YYYY-MM-DDTHH:MM:SS.000Z` for a Unix time (the form S3 lists).
#[cfg(any(test, feature = "test-util"))]
pub(crate) fn iso_date(unix: u64) -> String {
    let (y, m, d) = civil_from_days((unix / 86400) as i64);
    let s = unix % 86400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", s / 3600, s % 3600 / 60, s % 60)
}

/// Unix seconds of an ISO 8601 time like `2024-01-02T03:04:05.000Z` (always UTC in S3 listings).
pub(crate) fn parse_iso(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let days = days_from_civil(n(0..4)?, n(5..7)?, n(8..10)?);
    let secs = days * 86400 + n(11..13)? * 3600 + n(14..16)? * 60 + n(17..19)?;
    u64::try_from(secs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: Keys = Keys { key_id: "AKIAIOSFODNN7EXAMPLE", secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY", region: "us-east-1" };
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn matches_the_aws_get_object_example() {
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com".to_owned()),
            ("range", "bytes=0-9".to_owned()),
            ("x-amz-content-sha256", EMPTY.to_owned()),
            ("x-amz-date", "20130524T000000Z".to_owned()),
        ];
        let a = authorization(&KEYS, "GET", "/test.txt", &[], &headers, EMPTY, "20130524T000000Z");
        assert!(a.ends_with("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"), "{a}");
        assert!(a.contains("SignedHeaders=host;range;x-amz-content-sha256;x-amz-date"));
    }

    #[test]
    fn matches_the_aws_list_example() {
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com".to_owned()),
            ("x-amz-content-sha256", EMPTY.to_owned()),
            ("x-amz-date", "20130524T000000Z".to_owned()),
        ];
        let query = [("prefix", "J".to_owned()), ("max-keys", "2".to_owned())];
        let a = authorization(&KEYS, "GET", "/", &query, &headers, EMPTY, "20130524T000000Z");
        assert!(a.ends_with("Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"), "{a}");
    }

    #[test]
    fn matches_the_aws_put_object_example() {
        // The key `test$file.text` is encoded in the path; the body is "Welcome to Amazon S3."
        let hash = sha256_hex(b"Welcome to Amazon S3.");
        assert_eq!(hash, "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072");
        let path = format!("/{}", encode("test$file.text", true));
        assert_eq!(path, "/test%24file.text");
        let headers = [
            ("date", "Fri, 24 May 2013 00:00:00 GMT".to_owned()),
            ("host", "examplebucket.s3.amazonaws.com".to_owned()),
            ("x-amz-content-sha256", hash.clone()),
            ("x-amz-date", "20130524T000000Z".to_owned()),
            ("x-amz-storage-class", "REDUCED_REDUNDANCY".to_owned()),
        ];
        let a = authorization(&KEYS, "PUT", &path, &[], &headers, &hash, "20130524T000000Z");
        assert!(a.ends_with("Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"), "{a}");
    }

    #[test]
    fn base64_matches_the_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encodes_keys() {
        assert_eq!(encode("a b/ü~.txt", true), "a%20b/%C3%BC~.txt");
        assert_eq!(encode("a/b", false), "a%2Fb");
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_369_353_600), "20130524T000000Z");
        assert_eq!(parse_iso("2013-05-24T00:00:00.000Z"), Some(1_369_353_600));
        assert_eq!(parse_iso("2024-02-29T23:59:59Z"), Some(1_709_251_199));
        assert_eq!(parse_iso(&iso_date(1_709_251_199)), Some(1_709_251_199));
        assert_eq!(parse_iso("garbage"), None);
    }
}
