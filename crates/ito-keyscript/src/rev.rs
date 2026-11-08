//! Which ito commit a build is of, read from where cargo put the source. Pure string work: `build.rs` includes this file
//! (`#[path]`) and the tests run it as a module of the crate.

/// The rev in a cargo git checkout path (`.../git/checkouts/ito-<hash>/<rev>/crates/ito-keyscript`, either separator):
/// the component after `ito-*` when it is 7 to 40 hex digits. `None` for a path dependency, a vendored copy or anything else.
pub fn rev_from_manifest_dir(dir: &str) -> Option<String> {
    let parts: Vec<&str> = dir.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let at = parts.windows(3).position(|w| w[0] == "checkouts" && w[1].starts_with("ito-"))?;
    let rev = *parts.get(at + 2)?;
    ((7..=40).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit())).then(|| rev.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cargo_git_checkout_names_its_rev() {
        let want = Some("5df6419".to_owned());
        assert_eq!(rev_from_manifest_dir(r"C:\Users\me\.cargo\git\checkouts\ito-1a2b3c4d5e6f7a8b\5df6419\crates\ito-keyscript"), want);
        assert_eq!(rev_from_manifest_dir("/home/me/.cargo/git/checkouts/ito-1a2b3c4d5e6f7a8b/5df6419/crates/ito-keyscript"), want);
        assert_eq!(rev_from_manifest_dir("/Users/me/.cargo/git/checkouts/ito-1a2b3c4d5e6f7a8b/5df6419/crates/ito-keyscript"), want);
        let full = "5df6419aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(full.len(), 40);
        assert_eq!(rev_from_manifest_dir(&format!("/h/.cargo/git/checkouts/ito-abc/{full}/crates/ito-keyscript")), Some(full.to_owned()));
    }

    #[test]
    fn anything_else_has_no_rev() {
        assert_eq!(rev_from_manifest_dir("/h/.cargo/git/checkouts/ito-abc/main/crates/ito-keyscript"), None, "not hex");
        assert_eq!(rev_from_manifest_dir("/h/.cargo/git/checkouts/ito-abc/abc12/crates/ito-keyscript"), None, "too short");
        assert_eq!(rev_from_manifest_dir("/h/.cargo/git/checkouts/other-abc/5df6419/crates/ito-keyscript"), None, "not ito");
        assert_eq!(rev_from_manifest_dir(r"C:\dev\ito\crates\ito-keyscript"), None, "a path dependency");
        assert_eq!(rev_from_manifest_dir("/h/app/vendor/ito-keyscript"), None, "vendored");
        assert_eq!(rev_from_manifest_dir("/h/.cargo/git/checkouts/ito-abc"), None, "no component after");
    }
}
