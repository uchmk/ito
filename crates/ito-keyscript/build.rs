//! Records which ito commit this crate was built from (for `run.json`): `ITO_BUILD_REV`, else the rev in cargo's git checkout
//! path. No git command runs at build time.

#[path = "src/rev.rs"]
mod rev;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/rev.rs");
    println!("cargo:rerun-if-env-changed=ITO_BUILD_REV");
    let from_env = std::env::var("ITO_BUILD_REV").ok().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    let (rev, source) = match from_env {
        Some(r) => (Some(r), "env"),
        None => match std::env::var("CARGO_MANIFEST_DIR").ok().and_then(|d| rev::rev_from_manifest_dir(&d)) {
            Some(r) => (Some(r), "checkout-path"),
            None => (None, "unknown"),
        },
    };
    println!("cargo:rustc-env=ITO_REV={}", rev.unwrap_or_default());
    println!("cargo:rustc-env=ITO_REV_SOURCE={source}");
}
