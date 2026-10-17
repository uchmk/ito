//! Give `{{app}}.exe` its icon and version information (ito's `ito-winres`
//! writes the `.res` in Rust, so no resource compiler is needed).

#[path = "src/icon.rs"]
mod icon;

/// The sizes Explorer and the taskbar pick from, at 100 % to 200 % scaling.
const SIDES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    ito_winres::emit("{{app}}", &SIDES, icon::pixels);
}
