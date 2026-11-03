use std::ops::Range;

use crate::{Buffer, Edit, Error, Selection};

/// Something an action did besides the text, which undo and redo must also undo and redo (an image pasted into a
/// document wrote `assets/x.webp`: undo sends the file to the trash, redo brings it back; a rename that followed
/// links edited several files). The app has already done it when it applies the [`Transaction`]; `Doc` only calls
/// these afterwards, undo in the reverse order of the list and redo in order, before it touches the text. An `Err`
/// stops the undo / redo with the text and the earlier effects put back.
pub trait Effect: Send {
    fn undo(&mut self) -> Result<(), String>;
    fn redo(&mut self) -> Result<(), String>;
}

/// One user action: edits applied in order (each one in the coordinates left by the one before), the selection to
/// restore on undo and the selection to leave on redo, and the effects.
pub struct Transaction {
    pub edits: Vec<Edit>,
    pub before: Selection,
    pub after: Selection,
    pub effects: Vec<Box<dyn Effect>>,
}

impl Transaction {
    pub fn new(edits: Vec<Edit>, before: Selection, after: Selection) -> Transaction {
        Transaction { edits, before, after, effects: Vec::new() }
    }

    /// One `ito-mdedit` command as a transaction: the caret goes to the change's cursor.
    pub fn from_change(change: &ito_mdedit::Change, before: Selection) -> Transaction {
        Transaction::new(vec![Edit::from(change)], before, Selection::caret(change.cursor))
    }

    pub fn with_effect(mut self, effect: Box<dyn Effect>) -> Transaction {
        self.effects.push(effect);
        self
    }
}

struct Entry {
    forward: Vec<Edit>,
    /// The edits that undo `forward`, in the order to apply them.
    inverse: Vec<Edit>,
    before: Selection,
    after: Selection,
    effects: Vec<Box<dyn Effect>>,
    /// What the next key may join onto, when this step is typing or deleting.
    run: Option<Run>,
}

/// A step that the next key can join, and where that key has to land.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    /// Typing that ended at this byte.
    Typing(usize),
    /// Backspaces; the next one deletes up to this byte (where the last one started).
    Backspace(usize),
    /// Forward deletes; the next one starts at this byte.
    Delete(usize),
}

/// Apply `edits` in order; the edits that undo them (in the order to apply) come back. On an error nothing is left
/// applied.
fn run(buffer: &mut Buffer, edits: &[Edit]) -> Result<Vec<Edit>, Error> {
    let mut inverse: Vec<Edit> = Vec::with_capacity(edits.len());
    for e in edits {
        match buffer.replace(e.range.clone(), &e.text) {
            Ok(inv) => inverse.insert(0, inv),
            Err(err) => {
                for inv in &inverse {
                    let _ = buffer.replace(inv.range.clone(), &inv.text);
                }
                return Err(err);
            }
        }
    }
    Ok(inverse)
}

/// A buffer, its selection and its history.
pub struct Doc {
    buffer: Buffer,
    sel: Selection,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// The next typing starts a new step.
    sealed: bool,
    last_ms: u64,
    /// How many undo steps the text on disk is the result of; `None` when that point is no longer in the history.
    saved_at: Option<usize>,
    /// Typing joins the step before it when the keys come within this many milliseconds. Default 1000.
    pub pause_ms: u64,
}

impl Doc {
    pub fn new(read: &str) -> Doc {
        Doc {
            buffer: Buffer::new(read),
            sel: Selection::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            sealed: true,
            last_ms: 0,
            saved_at: Some(0),
            pause_ms: 1000,
        }
    }

    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }
    pub fn text(&self) -> &str {
        self.buffer.text()
    }
    pub fn version(&self) -> u64 {
        self.buffer.version()
    }
    pub fn selection(&self) -> Selection {
        self.sel
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The text was written to the file: it is clean from here until an edit, and clean again when undo / redo gets
    /// back to this point in the history. It ends the typing step in progress, so a key typed after the save can
    /// never join a step that was saved.
    pub fn mark_saved(&mut self) {
        self.saved_at = Some(self.undo.len());
        self.sealed = true;
    }

    /// Whether the text differs from what [`Doc::mark_saved`] was last called on (from what [`Doc::new`] was given,
    /// before any save). It follows the history position, not the version: save, edit, undo is clean; undo past the
    /// saved point is dirty; redo back is clean. A new edit after undoing past the saved point loses that point,
    /// so the text stays dirty until the next save.
    pub fn is_dirty(&self) -> bool {
        self.saved_at != Some(self.undo.len())
    }

    /// Move the caret or selection (a click, an arrow key). It ends the typing step in progress.
    pub fn set_selection(&mut self, sel: Selection) {
        let len = self.buffer.len();
        let sel = Selection {
            anchor: crate::pos::floor(self.text(), sel.anchor.min(len)),
            head: crate::pos::floor(self.text(), sel.head.min(len)),
        };
        if sel != self.sel {
            self.sealed = true;
        }
        self.sel = sel;
    }

    /// End the typing step in progress: the next typing is a new undo step (a pause, focus lost, a command).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// Forget the history (after the file is reloaded, say). The text stays, and so does whether it is dirty.
    pub fn clear_history(&mut self) {
        self.saved_at = (!self.is_dirty()).then_some(0);
        self.undo.clear();
        self.redo.clear();
        self.sealed = true;
    }

    /// A new step is coming: the undone steps are dropped, and a saved point among them goes with them.
    fn drop_redo(&mut self) {
        self.redo.clear();
        if self.saved_at.is_some_and(|s| s > self.undo.len()) {
            self.saved_at = None;
        }
    }

    /// Apply one user action as one undo step. `Err` when an edit does not fit the text; nothing is changed then.
    pub fn apply(&mut self, tx: Transaction) -> Result<(), Error> {
        let inverse = run(&mut self.buffer, &tx.edits)?;
        self.sel = tx.after;
        self.drop_redo();
        self.undo.push(Entry { forward: tx.edits, inverse, before: tx.before, after: tx.after, effects: tx.effects, run: None });
        self.sealed = true;
        Ok(())
    }

    /// Type `text` at the caret (over the selection, if there is one). `now_ms` is any clock in milliseconds. Typing
    /// joins the step before it while the keys follow each other within `pause_ms` at the place the last one ended
    /// and no line break was typed; the caret moving or [`Doc::seal`] ends it.
    pub fn typed(&mut self, text: &str, now_ms: u64) -> Result<(), Error> {
        let before = self.sel;
        let r = before.range();
        let join = before.is_empty()
            && !self.sealed
            && now_ms.saturating_sub(self.last_ms) <= self.pause_ms
            && self.undo.last().is_some_and(|e| e.run == Some(Run::Typing(r.start)));
        let inv = self.buffer.replace(r.clone(), text)?;
        let after = Selection::caret(r.start + text.len());
        let edit = Edit { range: r, text: text.to_owned() };
        let run = (!text.contains('\n')).then_some(Run::Typing(after.head));
        if join {
            let e = self.undo.last_mut().expect("joined step exists");
            e.forward.push(edit);
            e.inverse.insert(0, inv);
            e.after = after;
            e.run = run;
        } else {
            self.drop_redo();
            self.undo.push(Entry { forward: vec![edit], inverse: vec![inv], before, after, effects: Vec::new(), run });
        }
        self.sel = after;
        self.redo.clear();
        self.sealed = false;
        self.last_ms = now_ms;
        Ok(())
    }

    /// Delete `range` as the Backspace or Delete key does (the app picks the range: a grapheme, or a word). `now_ms`
    /// is the same clock as [`Doc::typed`]. With nothing selected, a Backspace (the range ends at the caret) joins the
    /// Backspace step before it and a Delete (the range starts at the caret) joins the Delete step before it, while
    /// the keys follow each other within `pause_ms` and each continues where the last one stopped. A caret that
    /// jumped, [`Doc::seal`], typing, or a different kind of edit between them ends it; so does deleting a selection
    /// (it is a step of its own and nothing joins it). The caret ends at the start of `range`. An empty range does
    /// nothing.
    pub fn deleted(&mut self, range: Range<usize>, now_ms: u64) -> Result<(), Error> {
        if range.start == range.end {
            return Ok(());
        }
        let before = self.sel;
        let kind = if !before.is_empty() {
            None
        } else if range.end == before.head {
            Some(Run::Backspace(range.start))
        } else if range.start == before.head {
            Some(Run::Delete(range.start))
        } else {
            None
        };
        let join = !self.sealed
            && now_ms.saturating_sub(self.last_ms) <= self.pause_ms
            && match (kind, self.undo.last().and_then(|e| e.run)) {
                (Some(Run::Backspace(_)), Some(Run::Backspace(at))) => range.end == at,
                (Some(Run::Delete(_)), Some(Run::Delete(at))) => range.start == at,
                _ => false,
            };
        let inv = self.buffer.replace(range.clone(), "")?;
        let after = Selection::caret(range.start);
        let edit = Edit { range, text: String::new() };
        if join {
            let e = self.undo.last_mut().expect("joined step exists");
            e.forward.push(edit);
            e.inverse.insert(0, inv);
            e.after = after;
            e.run = kind;
        } else {
            self.drop_redo();
            self.undo.push(Entry { forward: vec![edit], inverse: vec![inv], before, after, effects: Vec::new(), run: kind });
        }
        self.sel = after;
        self.sealed = false;
        self.last_ms = now_ms;
        Ok(())
    }

    /// An input method's committed text: always a step of its own, never joined to what came before or after.
    pub fn commit(&mut self, text: &str, now_ms: u64) -> Result<(), Error> {
        self.seal();
        self.typed(text, now_ms)?;
        self.seal();
        Ok(())
    }

    /// Undo the last step. `Ok(false)` when there is none.
    pub fn undo(&mut self) -> Result<bool, Error> {
        let Some(mut e) = self.undo.pop() else { return Ok(false) };
        for i in (0..e.effects.len()).rev() {
            if let Err(m) = e.effects[i].undo() {
                for j in i + 1..e.effects.len() {
                    let _ = e.effects[j].redo();
                }
                self.undo.push(e);
                return Err(Error::Effect(m));
            }
        }
        if let Err(err) = run(&mut self.buffer, &e.inverse) {
            for f in e.effects.iter_mut() {
                let _ = f.redo();
            }
            self.undo.push(e);
            return Err(err);
        }
        self.sel = e.before;
        self.redo.push(e);
        self.sealed = true;
        Ok(true)
    }

    /// Redo the step undone last. `Ok(false)` when there is none.
    pub fn redo(&mut self) -> Result<bool, Error> {
        let Some(mut e) = self.redo.pop() else { return Ok(false) };
        for i in 0..e.effects.len() {
            if let Err(m) = e.effects[i].redo() {
                for j in (0..i).rev() {
                    let _ = e.effects[j].undo();
                }
                self.redo.push(e);
                return Err(Error::Effect(m));
            }
        }
        if let Err(err) = run(&mut self.buffer, &e.forward) {
            for f in e.effects.iter_mut().rev() {
                let _ = f.undo();
            }
            self.redo.push(e);
            return Err(err);
        }
        self.sel = e.after;
        self.undo.push(e);
        self.sealed = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::Eol;

    fn typed_all(d: &mut Doc, s: &str, t0: u64) {
        for (i, c) in s.chars().enumerate() {
            d.typed(&c.to_string(), t0 + i as u64 * 50).unwrap();
        }
    }

    #[test]
    fn line_endings_and_bom_are_kept() {
        let d = Doc::new("\u{feff}a\r\nb\r\n");
        assert!(d.buffer().bom());
        assert_eq!(d.buffer().eol(), Eol::Crlf);
        assert_eq!(d.buffer().newline(), "\r\n");
        assert_eq!(d.buffer().saved(), "\u{feff}a\r\nb\r\n");
        assert_eq!(d.text(), "a\r\nb\r\n");
        assert_eq!(Buffer::new("a\nb\r\n").eol(), Eol::Mixed);
        assert_eq!(Buffer::new("one").eol(), Eol::None);
    }

    #[test]
    fn typing_is_one_step_until_a_pause_or_a_jump() {
        let mut d = Doc::new("");
        typed_all(&mut d, "hello", 0);
        assert_eq!(d.text(), "hello");
        d.typed(" w", 5000).unwrap();
        assert_eq!(d.text(), "hello w");
        d.undo().unwrap();
        assert_eq!(d.text(), "hello", "the pause started a new step");
        assert_eq!(d.selection(), Selection::caret(5));
        d.undo().unwrap();
        assert_eq!(d.text(), "");
        assert!(!d.can_undo());
        d.redo().unwrap();
        assert_eq!(d.text(), "hello");
        assert_eq!(d.selection(), Selection::caret(5));

        let mut d = Doc::new("");
        typed_all(&mut d, "ab", 0);
        d.set_selection(Selection::caret(0));
        d.typed("x", 200).unwrap();
        d.undo().unwrap();
        assert_eq!(d.text(), "ab", "moving the caret ended the step");
    }

    #[test]
    fn a_line_break_ends_the_step_and_a_commit_stands_alone() {
        let mut d = Doc::new("");
        typed_all(&mut d, "ab", 0);
        d.typed("\n", 100).unwrap();
        d.typed("c", 150).unwrap();
        d.undo().unwrap();
        assert_eq!(d.text(), "ab\n");
        let mut d = Doc::new("");
        d.typed("a", 0).unwrap();
        d.commit("日本", 10).unwrap();
        d.typed("b", 20).unwrap();
        d.undo().unwrap();
        d.undo().unwrap();
        assert_eq!(d.text(), "a");
    }

    #[test]
    fn typing_over_a_selection_replaces_it_and_undo_brings_it_back() {
        let mut d = Doc::new("hello world");
        d.set_selection(Selection { anchor: 6, head: 11 });
        d.typed("there", 0).unwrap();
        assert_eq!(d.text(), "hello there");
        d.undo().unwrap();
        assert_eq!(d.text(), "hello world");
        assert_eq!(d.selection(), Selection { anchor: 6, head: 11 });
    }

    #[test]
    fn a_transaction_of_several_edits_is_one_step_and_versions_only_go_up() {
        let mut d = Doc::new("# A\n\ntext\n");
        let v0 = d.version();
        let c = ito_mdedit::demote_section(d.text(), 0).unwrap();
        d.apply(Transaction::from_change(&c, Selection::caret(0))).unwrap();
        assert_eq!(d.text(), "## A\n\ntext\n");
        let edits = vec![Edit { range: 0..0, text: "x".into() }, Edit { range: 1..1, text: "y".into() }];
        d.apply(Transaction::new(edits, Selection::caret(0), Selection::caret(2))).unwrap();
        assert_eq!(d.text(), "xy## A\n\ntext\n");
        d.undo().unwrap();
        assert_eq!(d.text(), "## A\n\ntext\n");
        d.undo().unwrap();
        assert_eq!(d.text(), "# A\n\ntext\n");
        assert!(d.version() > v0 + 4);
        d.redo().unwrap();
        d.redo().unwrap();
        assert_eq!(d.text(), "xy## A\n\ntext\n");
        assert_eq!(d.selection(), Selection::caret(2));
    }

    #[test]
    fn dirty_follows_the_history_position() {
        let mut d = Doc::new("abc");
        assert!(!d.is_dirty());
        d.typed("x", 0).unwrap();
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(!d.is_dirty(), "back at the text that was read");
        d.redo().unwrap();
        d.mark_saved();
        assert!(!d.is_dirty());
        d.typed("y", 10).unwrap();
        assert!(d.is_dirty(), "typing after a save is a step of its own");
        d.undo().unwrap();
        assert!(!d.is_dirty());
        assert_eq!(d.text(), "xabc");
        d.undo().unwrap();
        assert!(d.is_dirty(), "undone past the saved point");
        d.redo().unwrap();
        assert!(!d.is_dirty());
    }

    #[test]
    fn saving_twice_moves_the_saved_point() {
        let mut d = Doc::new("");
        d.typed("a", 0).unwrap();
        d.mark_saved();
        d.set_selection(Selection::caret(0));
        d.typed("b", 5).unwrap();
        d.mark_saved();
        assert!(!d.is_dirty());
        d.undo().unwrap();
        assert!(d.is_dirty(), "the first save no longer counts");
        d.redo().unwrap();
        assert!(!d.is_dirty());
    }

    #[test]
    fn a_new_edit_after_undoing_past_the_saved_point_loses_it() {
        let mut d = Doc::new("");
        d.typed("a", 0).unwrap();
        d.mark_saved();
        d.undo().unwrap();
        assert!(d.is_dirty());
        d.typed("b", 10).unwrap();
        d.undo().unwrap();
        d.redo().unwrap();
        d.undo().unwrap();
        d.undo().unwrap_or(false);
        assert!(d.is_dirty(), "the saved text is gone from the history");
        // An edit that leaves the saved point on the undo side keeps it.
        let mut d = Doc::new("");
        d.typed("a", 0).unwrap();
        d.mark_saved();
        d.typed("b", 5).unwrap();
        d.undo().unwrap();
        d.typed("c", 9000).unwrap();
        d.undo().unwrap();
        assert!(!d.is_dirty());
        // Clearing the history keeps the verdict.
        d.typed("z", 20000).unwrap();
        d.clear_history();
        assert!(d.is_dirty());
        d.mark_saved();
        d.clear_history();
        assert!(!d.is_dirty());
    }

    #[test]
    fn backspaces_and_deletes_each_join_into_one_step() {
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(4));
        d.deleted(3..4, 0).unwrap();
        d.deleted(2..3, 50).unwrap();
        d.deleted(1..2, 100).unwrap();
        assert_eq!(d.text(), "aef");
        assert_eq!(d.selection(), Selection::caret(1));
        d.undo().unwrap();
        assert_eq!(d.text(), "abcdef");
        assert_eq!(d.selection(), Selection::caret(4));
        d.redo().unwrap();
        assert_eq!(d.text(), "aef");
        assert_eq!(d.selection(), Selection::caret(1));
        assert!(d.can_undo());
        d.undo().unwrap();
        assert!(!d.can_undo());

        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(1));
        d.deleted(1..2, 0).unwrap();
        d.deleted(1..2, 50).unwrap();
        assert_eq!(d.text(), "adef");
        assert_eq!(d.selection(), Selection::caret(1));
        d.undo().unwrap();
        assert_eq!(d.text(), "abcdef");
        assert!(!d.can_undo());
    }

    #[test]
    fn deleting_does_not_join_across_a_jump_a_pause_a_selection_or_another_kind() {
        let steps = |d: &mut Doc| {
            let mut n = 0;
            while d.undo().unwrap() {
                n += 1;
            }
            n
        };
        // a caret jump
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(6));
        d.deleted(5..6, 0).unwrap();
        d.set_selection(Selection::caret(2));
        d.deleted(1..2, 10).unwrap();
        assert_eq!(steps(&mut d), 2);
        // a pause
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(6));
        d.deleted(5..6, 0).unwrap();
        d.deleted(4..5, 5000).unwrap();
        assert_eq!(steps(&mut d), 2);
        // a selection delete, after and before
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection { anchor: 3, head: 5 });
        d.deleted(3..5, 0).unwrap();
        d.deleted(2..3, 10).unwrap();
        assert_eq!(d.text(), "abf");
        assert_eq!(steps(&mut d), 2);
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(6));
        d.deleted(5..6, 0).unwrap();
        d.set_selection(Selection { anchor: 3, head: 4 });
        d.deleted(3..4, 10).unwrap();
        assert_eq!(steps(&mut d), 2);
        // Backspace then Delete
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(3));
        d.deleted(2..3, 0).unwrap();
        d.deleted(2..3, 10).unwrap();
        assert_eq!(steps(&mut d), 2);
        // typing in between, either way round
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(3));
        d.deleted(2..3, 0).unwrap();
        d.typed("x", 10).unwrap();
        d.deleted(1..2, 20).unwrap();
        assert_eq!(steps(&mut d), 3);
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(3));
        d.typed("x", 0).unwrap();
        d.deleted(3..4, 10).unwrap();
        assert_eq!(steps(&mut d), 2);
        // a seal
        let mut d = Doc::new("abcdef");
        d.set_selection(Selection::caret(3));
        d.deleted(2..3, 0).unwrap();
        d.seal();
        d.deleted(1..2, 10).unwrap();
        assert_eq!(steps(&mut d), 2);
        // an empty range is nothing, a bad one is an error
        let mut d = Doc::new("a");
        d.deleted(0..0, 0).unwrap();
        assert!(!d.can_undo());
        assert_eq!(d.deleted(0..9, 0), Err(Error::Range));
    }

    #[test]
    fn a_saved_delete_run_is_not_extended() {
        let mut d = Doc::new("abcd");
        d.set_selection(Selection::caret(4));
        d.deleted(3..4, 0).unwrap();
        d.mark_saved();
        d.deleted(2..3, 10).unwrap();
        assert!(d.is_dirty());
        d.undo().unwrap();
        assert!(!d.is_dirty());
        assert_eq!(d.text(), "abc");
    }

    #[test]
    fn a_bad_edit_changes_nothing_and_a_new_edit_drops_the_redo_steps() {
        let mut d = Doc::new("abc");
        let bad = vec![Edit { range: 0..1, text: "X".into() }, Edit { range: 9..9, text: "Y".into() }];
        assert_eq!(d.apply(Transaction::new(bad, Selection::default(), Selection::default())), Err(Error::Range));
        assert_eq!(d.text(), "abc");
        assert!(!d.can_undo());
        d.typed("d", 0).unwrap();
        d.undo().unwrap();
        assert!(d.can_redo());
        d.typed("e", 10).unwrap();
        assert!(!d.can_redo());
        assert_eq!(Buffer::new("あ").replace(1..3, "x"), Err(Error::Range));
    }

    struct Log(&'static str, Arc<Mutex<Vec<String>>>, bool);
    impl Effect for Log {
        fn undo(&mut self) -> Result<(), String> {
            if self.2 {
                return Err("busy".into());
            }
            self.1.lock().unwrap().push(format!("undo {}", self.0));
            Ok(())
        }
        fn redo(&mut self) -> Result<(), String> {
            self.1.lock().unwrap().push(format!("redo {}", self.0));
            Ok(())
        }
    }

    #[test]
    fn effects_undo_in_reverse_and_redo_in_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut d = Doc::new("");
        let tx = Transaction::new(vec![Edit { range: 0..0, text: "![](a.webp)".into() }], Selection::caret(0), Selection::caret(11))
            .with_effect(Box::new(Log("a", log.clone(), false)))
            .with_effect(Box::new(Log("b", log.clone(), false)));
        d.apply(tx).unwrap();
        d.undo().unwrap();
        d.redo().unwrap();
        assert_eq!(*log.lock().unwrap(), ["undo b", "undo a", "redo a", "redo b"]);
        assert_eq!(d.text(), "![](a.webp)");
    }

    #[test]
    fn a_failing_effect_leaves_the_document_as_it_was() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut d = Doc::new("");
        let tx = Transaction::new(vec![Edit { range: 0..0, text: "x".into() }], Selection::caret(0), Selection::caret(1))
            .with_effect(Box::new(Log("a", log.clone(), true)))
            .with_effect(Box::new(Log("b", log.clone(), false)));
        d.apply(tx).unwrap();
        assert_eq!(d.undo(), Err(Error::Effect("busy".into())));
        assert_eq!(d.text(), "x");
        assert!(d.can_undo() && !d.can_redo());
        assert_eq!(*log.lock().unwrap(), ["undo b", "redo b"]);
    }
}
