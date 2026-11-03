//! The Japanese-text checks, on the prose of the text (not front matter, code, link targets, URLs or tags).

use std::ops::Range;

use ito_mdedit::Change;

use crate::scan::{self, Class, Scan};
use crate::{Kind, Punctuation, Sink, Spacing, Tone};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cls {
    Alnum,
    Cjk,
    Other,
}

fn cls(c: char) -> Cls {
    if c.is_ascii_alphanumeric() {
        Cls::Alnum
    } else if matches!(c as u32, 0x3005 | 0x3041..=0x309F | 0x30A1..=0x30FA | 0x30FC..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
    {
        Cls::Cjk
    } else {
        Cls::Other
    }
}

fn fullwidth_to_ascii(c: char) -> Option<char> {
    match c as u32 {
        v @ 0xFF10..=0xFF19 => char::from_u32('0' as u32 + (v - 0xFF10)),
        v @ 0xFF21..=0xFF3A => char::from_u32('A' as u32 + (v - 0xFF21)),
        v @ 0xFF41..=0xFF5A => char::from_u32('a' as u32 + (v - 0xFF41)),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Polite,
    Plain,
}

const POLITE: [&str; 8] = ["でしょう", "ましょう", "でした", "ました", "ません", "ください", "です", "ます"];
const PLAIN: [&str; 7] = ["であろう", "であった", "ではない", "である", "でない", "だった", "だろう"];

/// The sentence ending at the end of `tail` (closing brackets and spaces skipped): its style and the byte range of
/// the ending word within `tail`.
fn ending(tail: &str) -> Option<(Style, Range<usize>)> {
    let t = tail.trim_end_matches(|c: char| c.is_whitespace() || "」』）)】］]\"'”’*_".contains(c));
    for (list, style) in [(&POLITE[..], Style::Polite), (&PLAIN[..], Style::Plain)] {
        for e in list {
            if t.ends_with(e) {
                return Some((style, t.len() - e.len()..t.len()));
            }
        }
    }
    None
}

pub(crate) fn run(text: &str, sc: &Scan, sink: &mut Sink) {
    let any =
        [Kind::CjkSpaceMissing, Kind::CjkSpaceExtra, Kind::MixedPunctuation, Kind::FullwidthSpace, Kind::FullwidthAlnum, Kind::MixedTone]
            .into_iter()
            .any(|k| sink.on(k));
    if !any {
        return;
    }
    let (spacing, _) = (sink.rules.spacing, ());
    let mut puncts: Vec<(usize, char)> = Vec::new();
    let mut sentences: Vec<(Style, Range<usize>)> = Vec::new();
    for (i, line) in sc.lines.iter().enumerate() {
        if matches!(line.class, Class::Front | Class::Code | Class::Blank | Class::Hr | Class::SetextUnderline) {
            continue;
        }
        let s = &text[line.range.clone()];
        let base = line.range.start;
        let masks = scan::masks(s);
        let mut segs: Vec<Range<usize>> = Vec::new();
        let mut cur = 0;
        for m in &masks {
            if m.start > cur {
                segs.push(cur..m.start);
            }
            cur = cur.max(m.end);
        }
        if cur < s.len() {
            segs.push(cur..s.len());
        }
        let line_end = s.trim_end().len();
        let tone_line = !matches!(line.class, Class::Atx { .. } | Class::Setext { .. });
        let para_ends = !sc.lines.get(i + 1).is_some_and(|l| l.class == Class::Para);
        for seg in segs {
            let part = &s[seg.clone()];
            let at = base + seg.start;
            spaces(part, at, spacing, sink);
            wide(part, at, sink);
            for (o, c) in part.char_indices() {
                if matches!(c, '、' | '。' | '，' | '．') {
                    puncts.push((at + o, c));
                }
            }
            if tone_line && sink.on(Kind::MixedTone) {
                let mut from = 0;
                for (o, c) in part.char_indices() {
                    if matches!(c, '。' | '！' | '？') {
                        if let Some((st, r)) = ending(&part[from..o]) {
                            sentences.push((st, at + from + r.start..at + from + r.end));
                        }
                        from = o + c.len_utf8();
                    }
                }
                if para_ends && seg.end >= line_end && from < part.len() {
                    if let Some((st, r)) = ending(&part[from..]) {
                        sentences.push((st, at + from + r.start..at + from + r.end));
                    }
                }
            }
        }
    }
    punctuation(&puncts, sink);
    tone(&sentences, sink);
}

/// Spaces between half-width letters or digits and Japanese text.
fn spaces(part: &str, at: usize, rule: Spacing, sink: &mut Sink) {
    if rule == Spacing::Any {
        return;
    }
    let chars: Vec<(usize, char)> = part.char_indices().collect();
    let mut w = 0;
    while w < chars.len() {
        let (o, c) = chars[w];
        let pair = |a: Cls, b: Cls| (a == Cls::Alnum && b == Cls::Cjk) || (a == Cls::Cjk && b == Cls::Alnum);
        if c == ' ' {
            let mut e = w;
            while e < chars.len() && chars[e].1 == ' ' {
                e += 1;
            }
            if rule == Spacing::NoSpace && w > 0 && e < chars.len() && pair(cls(chars[w - 1].1), cls(chars[e].1)) {
                let end = chars.get(e).map_or(part.len(), |x| x.0);
                sink.replace(Kind::CjkSpaceExtra, at + o..at + end, "");
            }
            w = e;
            continue;
        }
        if rule == Spacing::Space && w > 0 && pair(cls(chars[w - 1].1), cls(c)) {
            let p = at + o;
            let range = at + chars[w - 1].0..p + c.len_utf8();
            sink.add(Kind::CjkSpaceMissing, range, Some(Change { range: p..p, text: " ".into(), cursor: p + 1 }));
        }
        w += 1;
    }
}

/// Full-width spaces and full-width letters and digits.
fn wide(part: &str, at: usize, sink: &mut Sink) {
    let mut run: Option<(usize, String)> = None;
    let mut end = 0;
    for (o, c) in part.char_indices() {
        if c == '\u{3000}' {
            sink.replace(Kind::FullwidthSpace, at + o..at + o + c.len_utf8(), " ");
        }
        match (fullwidth_to_ascii(c), &mut run) {
            (Some(a), Some((_, s))) => {
                s.push(a);
                end = o + c.len_utf8();
            }
            (Some(a), None) => {
                run = Some((o, a.to_string()));
                end = o + c.len_utf8();
            }
            (None, _) => {
                if let Some((s, ascii)) = run.take() {
                    sink.replace(Kind::FullwidthAlnum, at + s..at + end, ascii);
                }
            }
        }
    }
    if let Some((s, ascii)) = run.take() {
        sink.replace(Kind::FullwidthAlnum, at + s..at + end, ascii);
    }
}

fn punctuation(puncts: &[(usize, char)], sink: &mut Sink) {
    if !sink.on(Kind::MixedPunctuation) {
        return;
    }
    let ideo = puncts.iter().filter(|(_, c)| matches!(c, '、' | '。')).count();
    let west = puncts.len() - ideo;
    let keep_ideo = match sink.rules.punctuation {
        Punctuation::Ideographic => true,
        Punctuation::Western => false,
        Punctuation::Auto => {
            if ideo == 0 || west == 0 {
                return;
            }
            ideo >= west
        }
    };
    for &(p, c) in puncts {
        let is_ideo = matches!(c, '、' | '。');
        if is_ideo != keep_ideo {
            let to = match c {
                '、' => '，',
                '。' => '．',
                '，' => '、',
                _ => '。',
            };
            sink.replace(Kind::MixedPunctuation, p..p + c.len_utf8(), to.to_string());
        }
    }
}

fn tone(sentences: &[(Style, Range<usize>)], sink: &mut Sink) {
    if !sink.on(Kind::MixedTone) || sentences.is_empty() {
        return;
    }
    let polite = sentences.iter().filter(|(s, _)| *s == Style::Polite).count();
    let plain = sentences.len() - polite;
    let keep = match sink.rules.tone {
        Tone::Polite => Style::Polite,
        Tone::Plain => Style::Plain,
        Tone::Auto => {
            if polite == 0 || plain == 0 {
                return;
            }
            if polite > plain || (polite == plain && sentences[0].0 == Style::Polite) {
                Style::Polite
            } else {
                Style::Plain
            }
        }
    };
    for (st, r) in sentences {
        if *st != keep {
            sink.add(Kind::MixedTone, r.clone(), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{fixed, of};
    use crate::*;

    fn with(text: &str, rules: &Rules) -> Vec<Finding> {
        lint(text, rules, &|_| true)
    }

    fn spacing(s: Spacing) -> Rules {
        Rules { spacing: s, ..Rules::default() }
    }

    fn fix(text: &str, rules: &Rules, kind: Kind) -> String {
        let f = with(text, rules);
        fix_all(text, &f, Some(kind)).map_or(text.to_string(), |c| c.apply(text))
    }

    #[test]
    fn spacing_is_off_by_default() {
        assert!(of("日本語ABC日本語 123 です\n", Kind::CjkSpaceMissing).is_empty());
        assert!(of("日本語ABC日本語 123 です\n", Kind::CjkSpaceExtra).is_empty());
    }

    #[test]
    fn space_required_between_cjk_and_latin() {
        let r = spacing(Spacing::Space);
        let t = "日本語ABCと123個、x。\n";
        assert_eq!(fix(t, &r, Kind::CjkSpaceMissing), "日本語 ABC と 123 個、x。\n");
        // Punctuation is not a letter: no space next to it. Already spaced text is fine.
        assert!(with("日本語 ABC 日本語、ok。\n", &r).iter().all(|f| f.kind != Kind::CjkSpaceMissing));
        let f = with("あA\n", &r);
        assert_eq!(f[0].range, 0..4);
    }

    #[test]
    fn no_space_between_cjk_and_latin() {
        let r = spacing(Spacing::NoSpace);
        let t = "日本語 ABC と  123 個 and 英語 word\n";
        assert_eq!(fix(t, &r, Kind::CjkSpaceExtra), "日本語ABCと123個and英語word\n");
        // Two Latin words, or a space beside punctuation, are not touched.
        assert!(with("a b 、日本語、 c\n", &r).iter().all(|f| f.kind != Kind::CjkSpaceExtra));
        // A space at the start or the end of a segment.
        assert!(with("日本語 \n", &r).iter().all(|f| f.kind != Kind::CjkSpaceExtra));
    }

    #[test]
    fn spacing_skips_code_links_and_urls() {
        let r = spacing(Spacing::Space);
        let t = "`日本語abc` [リンク](https://例.jp/日本語abc) https://x.jp/abc <b>日本語</b>\n```\n日本語abc\n```\n";
        assert!(with("---\nk: 日本語abc\n---\n本文\n", &r).iter().all(|f| f.kind != Kind::CjkSpaceMissing));
        assert!(with(t, &r).iter().all(|f| f.kind != Kind::CjkSpaceMissing), "{:?}", with(t, &r));
        // Across a code span there is no adjacency to check.
        assert!(with("日本語`x`語\n", &r).iter().all(|f| f.kind != Kind::CjkSpaceMissing));
        // The link text is prose.
        assert_eq!(with("[日本語abc](x.md)\n", &r).iter().filter(|f| f.kind == Kind::CjkSpaceMissing).count(), 1);
    }

    #[test]
    fn mixed_punctuation_auto_flags_the_minority() {
        let t = "これは、テスト。次は，ここ。さらに、あれ。\n";
        let f = of(t, Kind::MixedPunctuation);
        assert_eq!(f.len(), 1);
        assert_eq!(&t[f[0].range.clone()], "，");
        assert_eq!(fixed(t, Kind::MixedPunctuation), "これは、テスト。次は、ここ。さらに、あれ。\n");
        // One style only: nothing in Auto.
        assert!(of("これは，テスト．\n", Kind::MixedPunctuation).is_empty());
        // Western wins when there are more of it, and the fix goes the other way.
        assert_eq!(fixed("a，b．c，d、e\n", Kind::MixedPunctuation), "a，b．c，d，e\n");
    }

    #[test]
    fn punctuation_rule_can_be_chosen() {
        let r = Rules { punctuation: Punctuation::Western, ..Rules::default() };
        assert_eq!(fix("これは、テスト。\n", &r, Kind::MixedPunctuation), "これは，テスト．\n");
        let r = Rules { punctuation: Punctuation::Ideographic, ..Rules::default() };
        assert_eq!(fix("これは，テスト．\n", &r, Kind::MixedPunctuation), "これは、テスト。\n");
        assert!(with("これは、テスト。\n", &r).iter().all(|f| f.kind != Kind::MixedPunctuation));
    }

    #[test]
    fn fullwidth_space_and_alnum() {
        let t = "　字下げ\u{3000}あ\n";
        assert_eq!(fixed(t, Kind::FullwidthSpace), " 字下げ あ\n");
        assert_eq!(of(t, Kind::FullwidthSpace).len(), 2);
        assert_eq!(fixed("ＡＢＣ１２３ and ａｂｃ ok\n", Kind::FullwidthAlnum), "ABC123 and abc ok\n");
        assert_eq!(of("ＡＢＣ１２３ ａ\n", Kind::FullwidthAlnum).len(), 2);
        // Full-width punctuation is not touched.
        assert!(of("（ａ）！\n", Kind::FullwidthAlnum).len() == 1);
        // Code is left alone.
        assert!(of("`ＡＢＣ`\n\n```\n　\n```\n", Kind::FullwidthAlnum).is_empty());
        assert!(of("```\n　\n```\n", Kind::FullwidthSpace).is_empty());
    }

    #[test]
    fn mixed_tone_flags_the_minority_sentence() {
        let t = "これは本です。それはペンです。あれは机である。\n";
        let f = of(t, Kind::MixedTone);
        assert_eq!(f.len(), 1);
        assert_eq!(&t[f[0].range.clone()], "である");
        assert!(f[0].fix.is_none());
        let t2 = "これは本である。それはペンである。あれは机です。\n";
        let f = of(t2, Kind::MixedTone);
        assert_eq!(f.len(), 1);
        assert_eq!(&t2[f[0].range.clone()], "です");
    }

    #[test]
    fn tone_cases() {
        // One style only: nothing in Auto.
        assert!(of("本です。ペンでした。行きます。\n", Kind::MixedTone).is_empty());
        // Tie: the first style wins.
        let t = "本です。机である。\n";
        assert_eq!(&t[of(t, Kind::MixedTone)[0].range.clone()], "である");
        // Brackets after the full stop and before it; endings that are neither style are ignored.
        let t = "「行きます」。話です。それは違うと思う。机であった。\n";
        assert_eq!(of(t, Kind::MixedTone).len(), 1);
        // A heading and a table are not sentences; the end of a paragraph without 。 counts.
        let t = "# 概要である\n\n本です\n\nペンです。\n\n- 机である\n";
        let f = of(t, Kind::MixedTone);
        assert_eq!(f.len(), 1);
        assert_eq!(&t[f[0].range.clone()], "である");
        // Soft-wrapped lines: only the end of the paragraph counts.
        assert!(of("これは長い文章で、行きます\nですが、本です。\n", Kind::MixedTone).is_empty());
        // Code is not a sentence.
        assert!(of("本です。\n\n```\n机である。\n```\n", Kind::MixedTone).is_empty());
        let r = Rules { tone: Tone::Plain, ..Rules::default() };
        assert_eq!(with("本です。ペンである。\n", &r).iter().filter(|f| f.kind == Kind::MixedTone).count(), 1);
        let r = Rules { tone: Tone::Polite, ..Rules::default() };
        assert_eq!(with("本である。ペンである。\n", &r).iter().filter(|f| f.kind == Kind::MixedTone).count(), 2);
    }

    #[test]
    fn crlf_japanese() {
        let t = "本です。\r\nペンである。\r\n　字下げ\r\n";
        assert_eq!(of(t, Kind::MixedTone).len(), 1);
        assert_eq!(fixed(t, Kind::FullwidthSpace), "本です。\r\nペンである。\r\n 字下げ\r\n");
    }

    #[test]
    fn japanese_checks_can_be_turned_off() {
        let t = "　ＡＢＣ、。，\n";
        assert!(!with(t, &Rules::default()).is_empty());
        assert!(with(t, &Rules::default().without_japanese()).is_empty());
    }

    #[test]
    fn fix_all_is_one_change() {
        let t = "　ＡＢ、。，\n";
        let f = with(t, &Rules::default());
        let one = fix_all(t, &f, None).unwrap();
        assert_eq!(one.apply(t), " AB、。、\n");
        assert_eq!(&one.text, " AB、。、");
        assert_eq!(fixes(&f, Kind::FullwidthSpace).len(), 1);
    }
}
