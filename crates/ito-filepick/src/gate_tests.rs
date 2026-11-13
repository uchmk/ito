//! The local gate (pure, no disk) and the JSON of places and styles.

use super::*;

#[test]
fn gate_local_classifies_without_the_disk() {
    use crate::path::Gate;
    let w = Style::Windows;
    for ok in ["C:\\", "C:\\x\\y", "c:/x", "C:/x", "\\\\srv\\share", "\\\\srv\\share\\dir", "//srv/share/dir", "C:\\x\\..\\y", "C:\\x\\CON"]
    {
        assert_eq!(w.gate_local(ok), Ok(ok.to_owned()), "{ok}");
    }
    assert_eq!(w.gate_local("C:"), Ok("C:\\".to_owned()));
    assert_eq!(w.gate_local("d:"), Ok("d:\\".to_owned()));
    assert_eq!(w.gate_local("C:foo"), Err(Gate::Path(PathError::DriveRelative)));
    assert_eq!(w.gate_local("\\\\?\\C:\\x"), Err(Gate::Path(PathError::Verbatim)));
    assert_eq!(w.gate_local("\\\\.\\pipe\\"), Err(Gate::Path(PathError::Verbatim)));
    assert_eq!(w.gate_local("\\??\\C:\\x"), Err(Gate::Path(PathError::Verbatim)));
    assert_eq!(w.gate_local("\\\\srv"), Err(Gate::Path(PathError::NoShare)));
    assert_eq!(w.gate_local("\\\\srv\\"), Err(Gate::Path(PathError::NoShare)));
    assert_eq!(w.gate_local("C:\\a\0b"), Err(Gate::Path(PathError::Nul)));
    for rel in ["foo", ".\\foo", "..\\foo", "\\foo", ""] {
        assert_eq!(w.gate_local(rel), Err(Gate::NotAbsolute), "{rel:?}");
    }
    let p = Style::Posix;
    for ok in ["/", "/a/b", "/a/../b", "/a\\b"] {
        assert_eq!(p.gate_local(ok), Ok(ok.to_owned()), "{ok}");
    }
    for rel in ["foo", "./foo", "", "\\foo", "C:\\x"] {
        assert_eq!(p.gate_local(rel), Err(Gate::NotAbsolute), "{rel:?}");
    }
    assert_eq!(p.gate_local("/a\0b"), Err(Gate::Path(PathError::Nul)));
}

#[cfg(feature = "serde")]
#[test]
fn places_and_styles_in_json() {
    for (kind, text) in [(PlaceKind::Root, "root"), (PlaceKind::Drive, "drive"), (PlaceKind::Home, "home"), (PlaceKind::Folder, "folder")] {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<PlaceKind>(&json).unwrap(), kind);
    }
    let place = Place::new("C:", "C:\\", PlaceKind::Drive);
    let json = serde_json::to_string(&place).unwrap();
    assert_eq!(json, r#"{"label":"C:","path":"C:\\","kind":"drive"}"#);
    assert_eq!(serde_json::from_str::<Place>(&json).unwrap(), place);
    assert_eq!(serde_json::to_string(&Style::Windows).unwrap(), "\"windows\"");
    assert_eq!(serde_json::to_string(&Style::Posix).unwrap(), "\"posix\"");
    for s in [Style::Windows, Style::Posix] {
        assert_eq!(serde_json::from_str::<Style>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
    }
}
