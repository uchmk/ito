//! Cutting a script into batches.

use crate::Dialect;

/// Cut `script` into the batches to send one by one. Blank batches (only spaces and comments) are dropped.
///
/// - SQL Server: a line that is only `GO` (any case, spaces around) ends a batch.
/// - PostgreSQL / SQLite: a `;` ends a statement unless it is inside `'…'`, `"…"`, a comment, a PostgreSQL
///   `$tag$ … $tag$` body, or (SQLite) between `CREATE TRIGGER` and its `END`.
///
/// The `;` that ends a statement is kept off the text. The text is otherwise as written.
pub fn split_batches(script: &str, dialect: Dialect) -> Vec<String> {
    let out = match dialect {
        Dialect::SqlServer => go_batches(script),
        Dialect::Postgres | Dialect::Sqlite => semicolon_batches(script, dialect),
    };
    out.into_iter().filter(|b| !is_blank(b)).collect()
}

fn go_batches(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in script.lines() {
        if line.trim().eq_ignore_ascii_case("go") {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push_str(line);
            cur.push('\n');
        }
    }
    out.push(cur);
    out
}

fn semicolon_batches(script: &str, dialect: Dialect) -> Vec<String> {
    let chars: Vec<char> = script.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    // The first three words of the statement (to see CREATE [TEMP] TRIGGER) and the last word before the `;`.
    let mut head: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut last_word = String::new();
    let mut i = 0;
    macro_rules! end_word {
        () => {
            if !word.is_empty() {
                if head.len() < 3 {
                    head.push(word.to_ascii_lowercase());
                }
                last_word = word.to_ascii_lowercase();
                word.clear();
            }
        };
    }
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\'' || c == '"' {
            end_word!();
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    if chars.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            i = (i + 1).min(chars.len());
            cur.extend(&chars[start..i]);
            last_word.clear();
            continue;
        }
        if c == '-' && next == Some('-') {
            end_word!();
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            cur.extend(&chars[start..i]);
            continue;
        }
        if c == '/' && next == Some('*') {
            end_word!();
            let start = i;
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    // SQLite does not nest; PostgreSQL does.
                    if depth == 0 || dialect == Dialect::Sqlite {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            cur.extend(&chars[start..i.min(chars.len())]);
            i = i.min(chars.len());
            continue;
        }
        if c == '$' && dialect == Dialect::Postgres && word.is_empty() {
            // $tag$ … $tag$ (the tag may be empty)
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if chars.get(j) == Some(&'$') {
                let tag: String = chars[i..=j].iter().collect();
                let body_start = j + 1;
                let rest: String = chars[body_start..].iter().collect();
                let end = rest.find(&tag).map(|p| rest[..p].chars().count() + tag.chars().count());
                let stop = body_start + end.unwrap_or(chars.len() - body_start);
                cur.extend(&chars[i..stop]);
                i = stop;
                last_word.clear();
                continue;
            }
        }
        if c == ';' {
            end_word!();
            let trigger = head.first().map(String::as_str) == Some("create") && head.iter().skip(1).take(2).any(|w| w == "trigger");
            if dialect == Dialect::Sqlite && trigger && last_word != "end" {
                cur.push(c);
            } else {
                out.push(std::mem::take(&mut cur));
                head.clear();
                last_word.clear();
            }
            i += 1;
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            end_word!();
            if !c.is_whitespace() {
                last_word.clear();
            }
        }
        cur.push(c);
        i += 1;
    }
    out.push(cur);
    out
}

/// Only spaces and comments.
fn is_blank(batch: &str) -> bool {
    let chars: Vec<char> = batch.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
        } else if chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use Dialect::*;

    fn cut(s: &str, d: Dialect) -> Vec<String> {
        split_batches(s, d).into_iter().map(|b| b.trim().to_owned()).collect()
    }

    #[test]
    fn sql_server_cuts_at_go_lines() {
        let s = "select 1\nGO\n  go  \nselect 'go'\nselect 2\nGO";
        assert_eq!(cut(s, SqlServer), ["select 1", "select 'go'\nselect 2"]);
    }

    #[test]
    fn blank_and_comment_only_batches_are_dropped() {
        assert!(cut("-- nothing\n/* here */\nGO\n", SqlServer).is_empty());
        assert!(cut("  ;; -- x\n;", Sqlite).is_empty());
    }

    #[test]
    fn semicolons_in_quotes_and_comments_do_not_cut() {
        let s = "insert into t values ('a;b'); -- x; y\nselect \"c;d\"; /* ; */ select 3";
        assert_eq!(cut(s, Sqlite), ["insert into t values ('a;b')", "-- x; y\nselect \"c;d\"", "/* ; */ select 3"]);
        assert_eq!(cut("select 'it''s;ok'; select 2", Postgres), ["select 'it''s;ok'", "select 2"]);
    }

    #[test]
    fn postgres_dollar_quotes_hold_semicolons() {
        let s = "create function f() returns int as $f$ begin return 1; end $f$ language plpgsql; select 1; do $$ begin null; end $$";
        let got = cut(s, Postgres);
        assert_eq!(got.len(), 3);
        assert!(got[0].ends_with("plpgsql"));
        assert_eq!(got[1], "select 1");
        assert!(got[2].starts_with("do $$"));
    }

    #[test]
    fn postgres_block_comments_nest() {
        assert_eq!(cut("/* a /* b; */ c; */ select 1; select 2", Postgres), ["/* a /* b; */ c; */ select 1", "select 2"]);
    }

    #[test]
    fn sqlite_trigger_is_one_statement() {
        let s = "create table t(a); create trigger g after insert on t begin update t set a=1; select 2; end; select 3";
        let got = cut(s, Sqlite);
        assert_eq!(got.len(), 3);
        assert!(got[1].starts_with("create trigger") && got[1].ends_with("end"));
        assert_eq!(got[2], "select 3");
    }
}
