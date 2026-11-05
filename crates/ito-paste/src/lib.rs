//! Smart paste for text editors (weft, and kura/michi/tsumugi when they paste into notes).
//!
//! - [`clipboard_html`] reads the clipboard's HTML flavour.
//! - [`html_to_markdown`] turns an HTML fragment into Markdown: headings, paragraphs, bold, italic,
//!   strike, code, links, images, nested lists, quotes, code blocks and tables (GFM).
//! - [`tsv_to_table`] turns tab-separated text (a spreadsheet copied without HTML) into a GFM table.
//!
//! No window and no HTML engine: the reader is small and forgiving. Scripts and styles are ignored,
//! unknown tags drop to their text, and `javascript:` links lose the link. Reading the clipboard
//! can block, so call it from a worker thread, not the UI thread.

use std::fmt;

mod convert;
mod parse;
mod tsv;

pub use tsv::tsv_to_table;

/// The clipboard could not be read.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "clipboard: {}", self.0)
    }
}

impl std::error::Error for Error {}

/// The HTML fragment on the clipboard (`CF_HTML` on Windows, `text/html` elsewhere), or `None`
/// when the clipboard holds no HTML.
pub fn clipboard_html() -> Result<Option<String>, Error> {
    let mut cb = arboard::Clipboard::new().map_err(|e| Error(e.to_string()))?;
    match cb.get().html() {
        Ok(s) if s.trim().is_empty() => Ok(None),
        Ok(s) => Ok(Some(s)),
        Err(arboard::Error::ContentNotAvailable) => Ok(None),
        Err(e) => Err(Error(e.to_string())),
    }
}

/// Markdown for an HTML fragment (or a whole page: only the part between the browser's
/// `StartFragment` and `EndFragment` marks is used when they are there). No trailing newline.
pub fn html_to_markdown(html: &str) -> String {
    let html = fragment(html);
    convert::document(&parse::parse(html))
}

fn fragment(html: &str) -> &str {
    const START: &str = "<!--StartFragment-->";
    const END: &str = "<!--EndFragment-->";
    match html.find(START) {
        Some(s) => {
            let from = s + START.len();
            let to = html[from..].find(END).map_or(html.len(), |e| from + e);
            &html[from..to]
        }
        None => html,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(html: &str) -> String {
        html_to_markdown(html)
    }

    #[test]
    fn headings_paragraphs_and_marks() {
        assert_eq!(md("<h1>Title</h1><p>one <b>two</b> <i>three</i> <s>four</s></p>"), "# Title\n\none **two** *three* ~~four~~");
    }

    #[test]
    fn spaces_stay_outside_the_marks() {
        assert_eq!(md("<p>a<b> b </b>c</p>"), "a **b** c");
    }

    #[test]
    fn links_and_images() {
        assert_eq!(
            md(r#"<p><a href="https://x.test/a b">go</a> <img src="p.png" alt="pic"></p>"#),
            "[go](https://x.test/a%20b) ![pic](p.png)"
        );
        assert_eq!(md(r#"<a href="https://x.test">https://x.test</a>"#), "<https://x.test>");
        assert_eq!(md(r#"<a href="javascript:alert(1)">bad</a>"#), "bad");
        assert_eq!(md(r#"<img src="data:image/png;base64,AAAA" alt="inline">"#), "inline");
    }

    #[test]
    fn nested_lists() {
        let html = "<ul><li>a<ul><li>a1</li><li>a2</li></ul></li><li>b</li></ul>";
        assert_eq!(md(html), "- a\n  - a1\n  - a2\n- b");
        assert_eq!(md(r#"<ol start="3"><li>x<li>y</ol>"#), "3. x\n4. y");
    }

    #[test]
    fn loose_list_items_keep_their_paragraphs() {
        assert_eq!(md("<ul><li><p>a</p><p>b</p></li></ul>"), "- a\n\n  b");
    }

    #[test]
    fn quote_and_code() {
        assert_eq!(md("<blockquote><p>q1</p><p>q2</p></blockquote>"), "> q1\n>\n> q2");
        assert_eq!(
            md(r#"<pre><code class="language-rust">fn a() {}
  b</code></pre>"#),
            "```rust\nfn a() {}\n  b\n```"
        );
        assert_eq!(md("<p>use <code>a`b</code></p>"), "use ``a`b``");
    }

    #[test]
    fn table_with_alignment() {
        let html =
            r#"<table><thead><tr><th>A</th><th align="right">B</th></tr></thead><tbody><tr><td>1</td><td>x|y</td></tr></tbody></table>"#;
        assert_eq!(md(html), "| A | B |\n| --- | ---: |\n| 1 | x\\|y |");
    }

    #[test]
    fn table_without_th_uses_the_first_row_and_pads() {
        assert_eq!(md("<table><tr><td>a<td>b<tr><td>c</table>"), "| a | b |\n| --- | --- |\n| c |  |");
    }

    #[test]
    fn layout_table_of_one_cell_is_just_its_content() {
        assert_eq!(md("<table><tr><td><p>hello</p></td></tr></table>"), "hello");
    }

    #[test]
    fn scripts_comments_and_unknown_tags() {
        assert_eq!(md("<p>a<script>if (a < b) { x() }</script><!-- c --><blink>b</blink></p><style>p{}</style>"), "ab");
    }

    #[test]
    fn entities() {
        assert_eq!(md("<p>a &amp; b &lt;c&gt; &#x41;&#66; &nbsp;&bogus;</p>"), "a & b \\<c> AB &bogus;");
    }

    #[test]
    fn specials_are_escaped_but_snake_case_is_not() {
        assert_eq!(md("<p>2 * 3 [x] snake_case _lead</p>"), "2 \\* 3 \\[x\\] snake_case \\_lead");
        assert_eq!(
            md("<p># not a heading</p><p>1. not a list</p><p>- nor this</p>"),
            "\\# not a heading\n\n1\\. not a list\n\n\\- nor this"
        );
    }

    #[test]
    fn line_breaks_and_fragment_marks() {
        assert_eq!(md("<p>a<br>b</p>"), "a\\\nb");
        assert_eq!(md("<html><body><!--StartFragment--><b>in</b><!--EndFragment--><p>out</p></body></html>"), "**in**");
    }

    #[test]
    fn task_boxes() {
        assert_eq!(
            md(r#"<ul><li><input type="checkbox" checked> done</li><li><input type="checkbox"> todo</li></ul>"#),
            "- [x] done\n- [ ] todo"
        );
    }

    #[test]
    fn broken_html_does_not_panic() {
        for s in ["<", "<<>>", "<a href=", "<p", "</", "<div><div><div>", "&", "&#;", "<!--", "<table><td></tr></table>x", "日本語<b>太字"]
        {
            let _ = md(s);
        }
        assert_eq!(md("日本語<b>太字"), "日本語**太字**");
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let html = "<div>".repeat(5000) + "x";
        assert_eq!(md(&html), "x");
    }

    #[test]
    fn real_clipboard_does_not_panic() {
        // May be Ok or Err (no display in CI); it must not panic.
        let _ = clipboard_html();
    }
}
