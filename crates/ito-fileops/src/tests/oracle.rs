//! robocopy as the oracle: the same fixture twice, robocopy on one and `copy_tree` on the other. Compares the
//! exit code with `robocopy_code()` and the files left in `into`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use super::*;
use crate::{copy_tree, TreeOptions};

type Setup = fn(&Path, &Path);

struct Case {
    name: &'static str,
    pattern: Option<&'static str>,
    /// Run the tool once, then `then`, before the run that is compared.
    warm: bool,
    setup: Setup,
    then: Option<Setup>,
}

fn t0() -> SystemTime {
    SystemTime::now() - days(3)
}

fn put(p: &Path, text: &str) {
    write(p, text);
    set_mtime(p, t0());
}

fn base(from: &Path, into: &Path) {
    put(&from.join("a.txt"), "alpha");
    put(&from.join("sub/b.txt"), "bravo!");
    put(&from.join("sub/deep/c.log"), "charlie");
    fs::create_dir_all(from.join("emptydir")).unwrap();
    fs::create_dir_all(into).unwrap();
}

fn robocopy(from: &Path, into: &Path, pattern: Option<&str>) -> i32 {
    let mut c = Command::new("robocopy");
    c.arg(from).arg(into);
    if let Some(p) = pattern {
        c.arg(p);
    }
    c.args(["/E", "/R:1", "/W:1", "/NJH", "/NJS", "/NP", "/NFL", "/NDL", "/NC", "/NS"]).stdout(Stdio::null()).stderr(Stdio::null());
    c.status().unwrap().code().unwrap()
}

fn native(from: &Path, into: &Path, pattern: Option<&str>) -> i32 {
    let opts = TreeOptions { retries: 1, wait: Duration::from_millis(50), time_tolerance: Duration::ZERO };
    copy_tree(from, into, pattern, &opts, &no_stop(), &mut |_| {}).robocopy_code()
}

/// Relative path (lower case) to size, or -1 for a folder.
fn listing(root: &Path) -> BTreeMap<String, i64> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, i64>) {
        for e in fs::read_dir(dir).unwrap() {
            let e = e.unwrap();
            let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().to_lowercase();
            if e.file_type().unwrap().is_dir() {
                out.insert(rel, -1);
                walk(&e.path(), root, out);
            } else {
                out.insert(rel, e.metadata().unwrap().len() as i64);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn compare(case: &Case) -> (i32, i32, bool) {
    let t = tempfile::tempdir().unwrap();
    let mut codes = Vec::new();
    let mut lists = Vec::new();
    for side in 0..2 {
        let (from, into) = (t.path().join(format!("{side}/from")), t.path().join(format!("{side}/into")));
        fs::create_dir_all(&from).unwrap();
        (case.setup)(&from, &into);
        let run = |from: &Path, into: &Path| if side == 0 { robocopy(from, into, case.pattern) } else { native(from, into, case.pattern) };
        if case.warm {
            run(&from, &into);
            if let Some(then) = case.then {
                then(&from, &into);
            }
        }
        codes.push(run(&from, &into));
        lists.push(listing(&into));
    }
    (codes[0], codes[1], lists[0] == lists[1])
}

fn cases() -> Vec<Case> {
    vec![
        Case { name: "first copy", pattern: None, warm: false, setup: base, then: None },
        Case { name: "second run", pattern: None, warm: true, setup: base, then: None },
        Case {
            name: "changed file",
            pattern: None,
            warm: true,
            setup: base,
            then: Some(|from, _| write(&from.join("a.txt"), "alpha, changed")),
        },
        Case { name: "extra file", pattern: None, warm: true, setup: base, then: Some(|_, into| write(&into.join("extra.txt"), "x")) },
        Case {
            name: "extra folder",
            pattern: None,
            warm: true,
            setup: base,
            then: Some(|_, into| write(&into.join("extradir/inside.txt"), "x")),
        },
        Case {
            name: "source file, destination folder",
            pattern: None,
            warm: false,
            setup: |from, into| {
                base(from, into);
                write(&into.join("a.txt/inner.txt"), "keep");
            },
            then: None,
        },
        Case {
            name: "source folder, destination file",
            pattern: None,
            warm: false,
            setup: |from, into| {
                base(from, into);
                write(&into.join("sub"), "i am a file");
            },
            then: None,
        },
        Case {
            name: "empty folders only",
            pattern: None,
            warm: false,
            setup: |from, into| {
                fs::create_dir_all(from.join("e1/e2")).unwrap();
                fs::create_dir_all(into).unwrap();
            },
            then: None,
        },
        Case {
            name: "pattern with filtered extras",
            pattern: Some("*.txt"),
            warm: false,
            setup: |from, into| {
                base(from, into);
                write(&into.join("stray.log"), "x");
            },
            then: None,
        },
        Case {
            name: "pattern with a matching extra",
            pattern: Some("*.txt"),
            warm: false,
            setup: |from, into| {
                base(from, into);
                write(&into.join("stray.txt"), "x");
            },
            then: None,
        },
        Case { name: "pattern, second run", pattern: Some("*.txt"), warm: true, setup: base, then: None },
    ]
}

#[test]
fn robocopy_and_copy_tree_agree() {
    let mut report = Vec::new();
    let mut disagree = Vec::new();
    for case in cases() {
        let (rc, nat, same_files) = compare(&case);
        report.push(format!("{}: robocopy {rc}, copy_tree {nat}, same files {same_files}", case.name));
        if rc != nat || !same_files {
            disagree.push(case.name);
        }
    }
    eprintln!("{}", report.join("\n"));
    assert!(disagree.is_empty(), "disagreement in: {disagree:?}\n{}", report.join("\n"));
}
