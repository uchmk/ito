//! A look before a commit: is the branch one to keep clear of direct commits
//! (`main`, `trunk`), and do the staged files look wrong to commit (a huge
//! file, a key or `.env`, a token inside)? The check is small, table-driven
//! and conservative: a name or a pattern that is nearly always a secret, so a
//! warning is rare enough to be read. It asks, never forbids.
//!
//! Git is not known here: the app lists what is staged ([`Entry`]: path,
//! size, binary or not) and hands over a function that reads some of them
//! ([`scan`]). Asking git for sizes and blobs stays with the app.

/// Branches no commit is made on without asking, when `protect` does not say.
pub const PROTECTED: [&str; 3] = ["main", "master", "trunk"];

/// A staged file this big (bytes) or bigger is warned of (50 MiB, where
/// GitHub starts to warn too).
pub const LARGE: u64 = 50 * 1024 * 1024;

/// A staged file smaller than this is noted for its size only if it is
/// suspected of something else: keeps the notes few, so the limit can be
/// changed in the settings without a new read.
pub const NOTE_FLOOR: u64 = 1024 * 1024;

/// The most a file may be to be read for tokens in it.
pub const SCAN_MAX: u64 = 256 * 1024;

/// The most files read for tokens in one look. Past it the rest is counted,
/// not read: see [`scan`].
pub const SCAN_FILES: usize = 1000;

/// The two settings: `protect` and `large_mb` (of a `[commit]` table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Branch names (with `*`) a commit asks about; none: never.
    pub protect: Vec<String>,
    /// Bytes from which a staged file is warned of; 0: never.
    pub large: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config { protect: PROTECTED.iter().map(|s| (*s).to_owned()).collect(), large: LARGE }
    }
}

impl Config {
    /// `large_mb` (MiB; 0 for never) as the bytes `large` holds.
    pub fn mib(mb: u64) -> u64 {
        mb.saturating_mul(1024 * 1024)
    }

    pub fn is_protected(&self, branch: &str) -> bool {
        is_protected(branch, &self.protect)
    }
}

/// Whether `branch` is one of `protect` (a name; `*` stands for any run of
/// characters in it, as in `release/*`).
pub fn is_protected(branch: &str, protect: &[String]) -> bool {
    protect.iter().any(|p| glob(p, branch))
}

/// `pat` against `text`, with `*` for any run of characters.
pub fn glob(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return pat == text;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if text.len() < first.len() + last.len() || !text.starts_with(first) || !text.ends_with(last) {
        return false;
    }
    let mut rest = &text[first.len()..text.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(k) => rest = &rest[k + mid.len()..],
            None => return false,
        }
    }
    true
}

/// Why a staged file looks like a secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Secret {
    /// Its name: `.env`, a private key's file.
    Name(&'static str),
    /// Something in it: the header of a private key, a token of a service.
    Content(&'static str),
}

impl Secret {
    /// What it is, in English (`".env file"`, `"GitHub token"`): the app
    /// words the sentence around it.
    pub fn what(&self) -> &'static str {
        match self {
            Secret::Name(w) | Secret::Content(w) => w,
        }
    }
}

/// A staged file worth a word before it is committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub path: String,
    /// Bytes.
    pub size: u64,
    pub secret: Option<Secret>,
}

impl Note {
    /// Whether it is warned of when files of `limit` bytes are too big
    /// (0: size does not matter).
    pub fn counts(&self, limit: u64) -> bool {
        self.secret.is_some() || self.is_large(limit)
    }

    pub fn is_large(&self, limit: u64) -> bool {
        limit > 0 && self.size >= limit
    }
}

/// Names that are nearly always a secret, as (what, matcher on the file's
/// lower-case name).
type NameRule = (&'static str, fn(&str) -> bool);
const NAMES: [NameRule; 5] = [
    (".env file", |n| {
        n == ".env"
            || n.strip_prefix(".env.").is_some_and(|s| !["example", "sample", "template", "dist", "defaults", "schema"].contains(&s))
    }),
    ("private key file", |n| {
        [".pem", ".key", ".p12", ".pfx", ".ppk", ".jks", ".keystore"].iter().any(|e| n.ends_with(e))
            || (n.ends_with(".asc") && n.contains("private"))
    }),
    ("SSH private key", |n| ["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"].contains(&n)),
    ("credentials file", |n| n == ".netrc" || n == "_netrc" || n == "credentials.json" || n == "service-account.json"),
    ("AWS credentials", |n| n == "credentials" || n == ".git-credentials"),
];

/// What the file named `path` (`/` or `\` between folders) is suspected of by
/// its name.
pub fn secret_name(path: &str) -> Option<Secret> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_ascii_lowercase();
    NAMES.iter().find(|(_, is)| is(&name)).map(|(what, _)| Secret::Name(what))
}

/// Tokens that start with a fixed word, as (what, prefix, characters after,
/// how many after).
type TokenRule = (&'static str, &'static str, fn(u8) -> bool, usize);
const TOKENS: [TokenRule; 6] = [
    ("AWS access key id", "AKIA", |b| b.is_ascii_uppercase() || b.is_ascii_digit(), 16),
    ("GitHub token", "ghp_", |b| b.is_ascii_alphanumeric(), 36),
    ("GitHub token", "github_pat_", |b| b.is_ascii_alphanumeric() || b == b'_', 40),
    ("Slack token", "xoxb-", |b| b.is_ascii_alphanumeric() || b == b'-', 24),
    ("Anthropic API key", "sk-ant-api", |b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_', 30),
    ("Google API key", "AIza", |b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_', 35),
];

/// What `bytes` is suspected of by what is in it.
pub fn secret_content(bytes: &[u8]) -> Option<Secret> {
    if contains(bytes, b"-----BEGIN ") {
        for kind in ["RSA ", "EC ", "DSA ", "OPENSSH ", "ENCRYPTED ", "PGP ", ""] {
            if contains(bytes, format!("-----BEGIN {kind}PRIVATE KEY").as_bytes()) {
                return Some(Secret::Content("private key"));
            }
        }
    }
    for (what, prefix, class, n) in TOKENS {
        let p = prefix.as_bytes();
        let mut from = 0;
        while let Some(k) = find(&bytes[from..], p) {
            let at = from + k + p.len();
            // Not the tail of a longer word, and `n` token characters after.
            let before_ok = from + k == 0 || !bytes[from + k - 1].is_ascii_alphanumeric();
            if before_ok && bytes.len() >= at + n && bytes[at..at + n].iter().all(|&b| class(b)) {
                return Some(Secret::Content(what));
            }
            from = from + k + 1;
        }
    }
    None
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    find(hay, needle).is_some()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A staged file as the app knows it: no content yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    /// Bytes.
    pub size: u64,
    /// Not to be read for tokens (an image, an archive).
    pub binary: bool,
}

/// Looks at `entries` (leave out deleted ones and anything that is not a
/// file): every name and size, and the content of the small text files. The
/// content comes from `read`, which is given the indices (into `entries`)
/// to read in one go and answers one `Option<Vec<u8>>` for each, in order
/// (`None`: could not be read). At most `max_read` files are read; the
/// number of small text files left unread past that is the second part of
/// the answer, so the caller can say the look was partial.
pub fn scan(entries: &[Entry], max_read: usize, read: impl FnOnce(&[usize]) -> Vec<Option<Vec<u8>>>) -> (Vec<Note>, usize) {
    let mut unread = 0;
    let mut notes: Vec<Note> = Vec::new();
    // (place in `notes`, index in `entries`)
    let mut to_read: Vec<(usize, usize)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let secret = secret_name(&e.path);
        if secret.is_some() || e.size >= NOTE_FLOOR {
            notes.push(Note { path: e.path.clone(), size: e.size, secret });
        } else if e.size <= SCAN_MAX && !e.binary {
            if to_read.len() >= max_read {
                unread += 1;
                continue;
            }
            to_read.push((notes.len(), i));
            // A place kept for it, dropped again if nothing is found.
            notes.push(Note { path: e.path.clone(), size: e.size, secret: None });
        }
    }
    if !to_read.is_empty() {
        let ids: Vec<usize> = to_read.iter().map(|(_, i)| *i).collect();
        let contents = read(&ids);
        for ((at, _), content) in to_read.iter().zip(contents) {
            if let Some(s) = content.as_deref().and_then(secret_content) {
                notes[*at].secret = Some(s);
            }
        }
    }
    notes.retain(|n| n.secret.is_some() || n.size >= NOTE_FLOOR);
    (notes, unread)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn protected_branches_match_by_name_or_star() {
        let p = names(&["main", "release/*", "*-prod"]);
        assert!(is_protected("main", &p));
        assert!(is_protected("release/1.2", &p));
        assert!(is_protected("eu-prod", &p));
        assert!(!is_protected("feature/main", &p));
        assert!(!is_protected("mainline", &p));
        assert!(!is_protected("main", &[]));
        assert!(Config::default().is_protected("trunk"));
    }

    #[test]
    fn names_that_hold_secrets_are_found_and_samples_are_not() {
        for p in [".env", "app/.env.local", "keys/server.pem", "a/id_rsa", "x.PFX", ".netrc", ".aws/credentials", r"a\b\.env"] {
            assert!(secret_name(p).is_some(), "{p}");
        }
        for p in [".env.example", "id_rsa.pub", "src/environment.rs", "credentials.rs", "monkey.txt", "keyboard.rs"] {
            assert_eq!(secret_name(p), None, "{p}");
        }
    }

    #[test]
    fn tokens_and_keys_are_found_in_content_and_near_misses_are_not() {
        assert_eq!(secret_content(b"x\n-----BEGIN OPENSSH PRIVATE KEY-----\n"), Some(Secret::Content("private key")));
        assert_eq!(secret_content(b"-----BEGIN PRIVATE KEY-----"), Some(Secret::Content("private key")));
        assert_eq!(secret_content(b"id = AKIAIOSFODNN7EXAMPLE\n"), Some(Secret::Content("AWS access key id")));
        let gh = format!("t={}", "ghp_".to_owned() + &"a1".repeat(18));
        assert_eq!(secret_content(gh.as_bytes()), Some(Secret::Content("GitHub token")));
        // A public key or certificate, a short word, a longer word it ends.
        assert_eq!(secret_content(b"-----BEGIN PUBLIC KEY-----"), None);
        assert_eq!(secret_content(b"-----BEGIN CERTIFICATE-----"), None);
        assert_eq!(secret_content(b"AKIA is a prefix"), None);
        assert_eq!(secret_content(b"XAKIAIOSFODNN7EXAMPLE"), None);
        assert_eq!(secret_content(b"fn main() {}"), None);
    }

    fn entry(path: &str, size: u64) -> Entry {
        Entry { path: path.into(), size, binary: false }
    }

    /// A reader that answers from `files` by the index asked.
    fn reader<'a>(files: &'a [(&'a str, &'a [u8])]) -> impl FnOnce(&[usize]) -> Vec<Option<Vec<u8>>> + 'a {
        move |ids| ids.iter().map(|&i| Some(files[i].1.to_vec())).collect()
    }

    #[test]
    fn a_scan_finds_names_content_and_size() {
        let files: [(&str, &[u8]); 6] = [
            ("ok.txt", b"hello\n"),
            ("prod.env.txt", b"x"),
            (".env", b"A=1\n"),
            ("notes.txt", b"key: AKIAIOSFODNN7EXAMPLE\n"),
            ("big.bin", b""),
            ("deep/id_ed25519", b"x"),
        ];
        let entries: Vec<Entry> =
            files
                .iter()
                .map(|(p, b)| {
                    if *p == "big.bin" {
                        Entry { path: (*p).into(), size: 2 * 1024 * 1024, binary: true }
                    } else {
                        entry(p, b.len() as u64)
                    }
                })
                .collect();
        let (notes, unread) = scan(&entries, SCAN_FILES, reader(&files));
        assert_eq!(unread, 0);
        let by = |p: &str| notes.iter().find(|n| n.path == p).unwrap_or_else(|| panic!("{p}: {notes:?}"));
        assert_eq!(by(".env").secret, Some(Secret::Name(".env file")));
        assert_eq!(by("deep/id_ed25519").secret, Some(Secret::Name("SSH private key")));
        assert_eq!(by("notes.txt").secret, Some(Secret::Content("AWS access key id")));
        assert_eq!((by("big.bin").secret, by("big.bin").size), (None, 2 * 1024 * 1024));
        assert!(!by("big.bin").counts(LARGE) && by("big.bin").counts(1024 * 1024));
        assert_eq!(notes.len(), 4, "{notes:?}");
    }

    #[test]
    fn files_past_the_read_cap_are_counted_not_dropped_silently() {
        let files: [(&str, &[u8]); 3] = [("a.txt", b"a\n"), ("b.txt", b"key AKIAIOSFODNN7EXAMPLE\n"), ("c.txt", b"c\n")];
        let entries: Vec<Entry> = files.iter().map(|(p, b)| entry(p, b.len() as u64)).collect();
        let (notes, unread) = scan(&entries, 1, reader(&files));
        assert_eq!(unread, 2);
        assert!(notes.is_empty(), "{notes:?}");
        let (notes, unread) = scan(&entries, 10, reader(&files));
        assert_eq!((unread, notes.len()), (0, 1));
    }

    #[test]
    fn nothing_staged_reads_nothing() {
        let (notes, unread) = scan(&[], 10, |_| panic!("nothing to read"));
        assert_eq!((notes, unread), (vec![], 0));
    }
}
