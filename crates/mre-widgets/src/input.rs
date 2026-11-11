//! Editable text state.
//!
//! Carets are counted in characters, not bytes, so a field holding an emoji or
//! an Arabic word cannot be corrupted by editing. A caret and an anchor model
//! selection: when they differ the range between them is selected, which is the
//! same model every text editor uses. The state is deliberately small and
//! plain; rich text belongs in a later milestone.

/// The editable state of one text field.
#[derive(Clone, Debug, Default)]
pub struct InputState {
    /// Current contents.
    pub text: String,
    /// Caret position, in characters from the start.
    pub caret: usize,
    /// Selection anchor. Equal to [`Self::caret`] when nothing is selected.
    pub anchor: usize,
    /// Text being composed by an input method, shown at the caret but not yet
    /// part of the value.
    pub preedit: String,
    /// Saved states, oldest first.
    undo: Vec<Snapshot>,
    /// States that were undone, newest last.
    redo: Vec<Snapshot>,
    /// The kind of the edit in progress, used to group keystrokes.
    last_edit: Option<EditKind>,
    /// How long the caret has been in its current phase, in seconds.
    ///
    /// The caret blinks while the field is focused and the app is idle. The
    /// phase restarts whenever the caret moves or the text changes, so the
    /// caret is always visible right after a keystroke.
    blink: f32,
}

/// What kind of edit happened.
///
/// Consecutive edits of the same kind share one undo step, so a word typed one
/// letter at a time is undone in one go, while a word typed, deleted and typed
/// again is undone in three.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    /// Typing or pasting.
    Insert,
    /// Replacing a selection.
    Replace,
    /// Deleting characters.
    Delete,
}

/// How long a full blink cycle takes, in seconds.
///
/// The caret is visible for the first half of the cycle and hidden for the
/// second, so it blinks once a second — the pace every editor uses.
const BLINK_PERIOD: f32 = 1.0;

/// The start of the word before `index`.
///
/// Whitespace is skipped first, so the caret lands on the start of the word
/// rather than on the space in front of it.
fn word_start_before(chars: &[char], index: usize) -> usize {
    let mut start = index.min(chars.len());
    while start > 0 && chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    start
}

/// The start of the word after `index`.
///
/// The rest of the word is skipped first and the whitespace after it second,
/// so the caret lands on the next word rather than on the space before it.
fn word_end_after(chars: &[char], index: usize) -> usize {
    let mut end = index.min(chars.len());
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    while end < chars.len() && chars[end].is_whitespace() {
        end += 1;
    }
    end
}

/// A saved editing state.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    caret: usize,
    anchor: usize,
}

impl PartialEq for InputState {
    /// Compares the visible state only: the undo history is not part of what
    /// the user sees.
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
            && self.caret == other.caret
            && self.anchor == other.anchor
            && self.preedit == other.preedit
    }
}

impl Eq for InputState {}

impl InputState {
    /// A field containing `text`, with the caret at the end.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.chars().count();
        Self {
            text,
            caret,
            anchor: caret,
            preedit: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            blink: 0.0,
        }
    }

    /// Replaces the contents from outside the editor, keeping the caret valid.
    ///
    /// A value that arrives from outside is a new starting point, so the undo
    /// history is dropped.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.caret.min(self.len());
        self.anchor = self.anchor.min(self.len());
        if self.anchor == self.caret {
            self.anchor = self.caret;
        }
        self.undo.clear();
        self.redo.clear();
        self.last_edit = None;
    }

    /// Whether there is something to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is something to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last edit.
    ///
    /// Returns `true` when the field changed.
    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.restore(snapshot);
        self.last_edit = None;
        true
    }

    /// Redoes the last undone edit.
    ///
    /// Returns `true` when the field changed.
    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.restore(snapshot);
        self.last_edit = None;
        true
    }

    /// The current state, as an undo snapshot.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            caret: self.caret,
            anchor: self.anchor,
        }
    }

    /// Restores a snapshot.
    fn restore(&mut self, snapshot: Snapshot) {
        self.text = snapshot.text;
        self.caret = snapshot.caret.min(self.len());
        self.anchor = snapshot.anchor.min(self.len());
    }

    /// Starts an edit, saving the current state unless it continues the edit
    /// already in progress.
    fn begin_edit(&mut self, kind: EditKind) {
        if self.last_edit == Some(kind) {
            return;
        }
        self.undo.push(self.snapshot());
        self.redo.clear();
        self.last_edit = Some(kind);
    }

    /// Length in characters.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// True when the field is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The text before the caret.
    #[must_use]
    pub fn caret_prefix(&self) -> String {
        self.text.chars().take(self.caret).collect()
    }

    /// The selected character range, `(start, end)`, or `None`.
    #[must_use]
    pub fn selection(&self) -> Option<(usize, usize)> {
        let start = self.caret.min(self.anchor);
        let end = self.caret.max(self.anchor);
        (start != end).then_some((start, end))
    }

    /// The selected text.
    #[must_use]
    pub fn selected(&self) -> String {
        match self.selection() {
            Some((start, end)) => self.text.chars().skip(start).take(end - start).collect(),
            None => String::new(),
        }
    }

    /// Selects everything.
    pub fn select_all(&mut self) -> bool {
        if self.text.is_empty() || self.selection() == Some((0, self.len())) {
            return false;
        }
        self.anchor = 0;
        self.caret = self.len();
        true
    }

    /// Moves the caret to a character index, optionally extending the selection.
    ///
    /// Returns `true` when the caret or the anchor moved.
    pub fn set_caret(&mut self, caret: usize, extend: bool) -> bool {
        let caret = caret.min(self.len());
        let anchor = if extend { self.anchor } else { caret };
        if self.caret == caret && self.anchor == anchor {
            return false;
        }
        self.caret = caret;
        self.anchor = anchor;
        // Moving the caret ends the group of edits that undo together, and
        // brings the caret back: a caret that blinks on while it moves is
        // impossible to follow.
        self.last_edit = None;
        self.restart_blink();
        true
    }

    /// Removes the selected range, leaving the caret where it started.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        let mut chars: Vec<char> = self.text.chars().collect();
        chars.drain(start..end);
        self.text = chars.into_iter().collect();
        self.caret = start;
        self.anchor = start;
        true
    }

    /// Inserts text at the caret, replacing the selection, ignoring control
    /// characters.
    ///
    /// Returns `true` when something was inserted.
    pub fn insert(&mut self, text: &str) -> bool {
        self.restart_blink();
        let inserted: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        if inserted.is_empty() {
            return false;
        }
        let kind = if self.selection().is_some() {
            EditKind::Replace
        } else {
            EditKind::Insert
        };
        self.begin_edit(kind);
        self.delete_selection();
        let mut chars: Vec<char> = self.text.chars().collect();
        let at = self.caret.min(chars.len());
        let count = inserted.len();
        chars.splice(at..at, inserted);
        self.text = chars.into_iter().collect();
        self.caret = at + count;
        self.anchor = self.caret;
        true
    }

    /// Deletes the selection, or the character before the caret.
    pub fn backspace(&mut self) -> bool {
        self.restart_blink();
        if self.selection().is_some() {
            self.begin_edit(EditKind::Delete);
            return self.delete_selection();
        }
        if self.caret == 0 {
            return false;
        }
        self.begin_edit(EditKind::Delete);
        let mut chars: Vec<char> = self.text.chars().collect();
        let at = (self.caret - 1).min(chars.len().saturating_sub(1));
        if at < chars.len() {
            chars.remove(at);
        }
        self.text = chars.into_iter().collect();
        self.caret -= 1;
        self.anchor = self.caret;
        true
    }

    /// Deletes the selection, or the character after the caret.
    pub fn delete(&mut self) -> bool {
        self.restart_blink();
        if self.selection().is_some() {
            self.begin_edit(EditKind::Delete);
            return self.delete_selection();
        }
        if self.caret >= self.len() {
            return false;
        }
        self.begin_edit(EditKind::Delete);
        let mut chars: Vec<char> = self.text.chars().collect();
        chars.remove(self.caret);
        self.text = chars.into_iter().collect();
        true
    }

    /// Moves the caret one character left.
    pub fn left(&mut self) -> bool {
        // A selection collapses to its start, the way every text editor
        // behaves: pressing left once takes the caret to where the selection
        // began, and again moves one character.
        if let Some((start, _)) = self.selection() {
            return self.set_caret(start, false);
        }
        self.move_left(false)
    }

    /// Moves the caret one character left, keeping the selection anchor.
    pub fn left_extend(&mut self) -> bool {
        self.move_left(true)
    }

    fn move_left(&mut self, extend: bool) -> bool {
        if self.caret == 0 {
            return false;
        }
        self.set_caret(self.caret - 1, extend)
    }

    /// Moves the caret one character right.
    pub fn right(&mut self) -> bool {
        // A selection collapses to its end.
        if let Some((_, end)) = self.selection() {
            return self.set_caret(end, false);
        }
        self.move_right(false)
    }

    /// Moves the caret one character right, keeping the selection anchor.
    pub fn right_extend(&mut self) -> bool {
        self.move_right(true)
    }

    fn move_right(&mut self, extend: bool) -> bool {
        if self.caret >= self.len() {
            return false;
        }
        self.set_caret(self.caret + 1, extend)
    }

    /// Moves the caret to the start of the previous word.
    pub fn word_left(&mut self) -> bool {
        if let Some((start, _)) = self.selection() {
            return self.set_caret(start, false);
        }
        let target = word_start_before(&self.chars(), self.caret);
        self.set_caret(target, false)
    }

    /// Moves the caret to the start of the previous word, keeping the anchor.
    pub fn word_left_extend(&mut self) -> bool {
        let target = word_start_before(&self.chars(), self.caret);
        self.set_caret(target, true)
    }

    /// Moves the caret to the start of the next word.
    pub fn word_right(&mut self) -> bool {
        if let Some((_, end)) = self.selection() {
            return self.set_caret(end, false);
        }
        let target = word_end_after(&self.chars(), self.caret);
        self.set_caret(target, false)
    }

    /// Moves the caret to the start of the next word, keeping the anchor.
    pub fn word_right_extend(&mut self) -> bool {
        let target = word_end_after(&self.chars(), self.caret);
        self.set_caret(target, true)
    }

    /// Deletes the word before the caret, or the selection.
    pub fn delete_word_backward(&mut self) -> bool {
        self.restart_blink();
        if self.selection().is_some() {
            self.begin_edit(EditKind::Delete);
            return self.delete_selection();
        }
        if self.caret == 0 {
            return false;
        }
        self.begin_edit(EditKind::Delete);
        let start = word_start_before(&self.chars(), self.caret);
        let mut chars: Vec<char> = self.text.chars().collect();
        chars.drain(start..self.caret);
        self.text = chars.into_iter().collect();
        self.caret = start;
        self.anchor = start;
        true
    }

    /// Deletes the word after the caret, or the selection.
    pub fn delete_word_forward(&mut self) -> bool {
        self.restart_blink();
        if self.selection().is_some() {
            self.begin_edit(EditKind::Delete);
            return self.delete_selection();
        }
        let end = word_end_after(&self.chars(), self.caret);
        if end <= self.caret {
            return false;
        }
        self.begin_edit(EditKind::Delete);
        let mut chars: Vec<char> = self.text.chars().collect();
        chars.drain(self.caret..end);
        self.text = chars.into_iter().collect();
        self.anchor = self.caret;
        true
    }

    /// Selects the word around a character index.
    ///
    /// This is what a double click does. The index is clamped to the text.
    pub fn select_word_at(&mut self, index: usize) -> bool {
        let chars = self.chars();
        if chars.is_empty() {
            return false;
        }
        let index = index.min(chars.len() - 1);
        let is_word = |c: char| !c.is_whitespace();
        if !is_word(chars[index]) {
            // Clicking a space selects the word after it, if there is one.
            let mut start = index;
            while start < chars.len() && !is_word(chars[start]) {
                start += 1;
            }
            if start >= chars.len() {
                return false;
            }
            let mut end = start;
            while end < chars.len() && is_word(chars[end]) {
                end += 1;
            }
            return self.select_range(start, end);
        }
        let mut start = index;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        let mut end = index + 1;
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        self.select_range(start, end)
    }

    /// Selects the range between two character indices.
    fn select_range(&mut self, start: usize, end: usize) -> bool {
        if self.anchor == start && self.caret == end {
            return false;
        }
        self.anchor = start;
        self.caret = end;
        self.last_edit = None;
        self.restart_blink();
        true
    }

    /// The characters of the field.
    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    /// Advances the caret's blink by `dt` seconds.
    ///
    /// Returns `true` when the caret appeared or disappeared, which is the
    /// only thing a repaint has to know.
    pub fn tick_blink(&mut self, dt: f32) -> bool {
        let was_visible = self.caret_visible();
        self.blink = (self.blink + dt.max(0.0)) % BLINK_PERIOD;
        was_visible != self.caret_visible()
    }

    /// Whether the caret is in its visible phase.
    #[must_use]
    pub fn caret_visible(&self) -> bool {
        self.blink < BLINK_PERIOD / 2.0
    }

    /// Restarts the blink, so the caret is visible again.
    pub fn restart_blink(&mut self) {
        self.blink = 0.0;
    }

    /// Moves the caret to the start.
    pub fn home(&mut self) -> bool {
        self.set_caret(0, false)
    }

    /// Moves the caret to the start, keeping the selection anchor.
    pub fn home_extend(&mut self) -> bool {
        self.set_caret(0, true)
    }

    /// Moves the caret to the end.
    pub fn end(&mut self) -> bool {
        self.set_caret(self.len(), false)
    }

    /// Moves the caret to the end, keeping the selection anchor.
    pub fn end_extend(&mut self) -> bool {
        self.set_caret(self.len(), true)
    }

    /// Removes everything.
    pub fn clear(&mut self) -> bool {
        self.restart_blink();
        if self.text.is_empty() {
            return false;
        }
        self.begin_edit(EditKind::Replace);
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_appends_at_the_caret() {
        let mut state = InputState::new("ab");
        state.insert("c");
        assert_eq!(state.text, "abc");
        assert_eq!(state.caret, 3);
    }

    #[test]
    fn insertion_happens_where_the_caret_is() {
        let mut state = InputState::new("ac");
        state.left();
        state.insert("b");
        assert_eq!(state.text, "abc");
        assert_eq!(state.caret, 2);
    }

    #[test]
    fn backspace_and_delete_respect_characters_not_bytes() {
        let mut state = InputState::new("héllo");
        state.backspace();
        assert_eq!(state.text, "héll");

        state.home();
        state.delete();
        assert_eq!(state.text, "éll");

        let mut arabic = InputState::new("مرحبا");
        arabic.backspace();
        assert_eq!(arabic.text.chars().count(), 4);
        assert_eq!(arabic.caret, 4);
    }

    #[test]
    fn control_characters_are_ignored() {
        let mut state = InputState::new("");
        assert!(!state.insert("\n\t"));
        assert!(state.text.is_empty());
    }

    #[test]
    fn movement_is_clamped() {
        let mut state = InputState::new("ab");
        assert!(!state.right());
        assert_eq!(state.caret, 2);
        state.left();
        state.left();
        assert!(!state.left());
        assert_eq!(state.caret, 0);
        assert!(!state.backspace());
        assert!(state.delete());
        assert_eq!(state.text, "b");
    }

    #[test]
    fn setting_text_keeps_the_caret_in_range() {
        let mut state = InputState::new("abcdef");
        state.set_text("ab");
        assert_eq!(state.caret, 2);
    }

    #[test]
    fn the_caret_prefix_splits_unicode_safely() {
        let mut state = InputState::new("a🌍b");
        state.home();
        state.right();
        state.right();
        assert_eq!(state.caret_prefix(), "a🌍");
    }

    #[test]
    fn shift_arrows_and_select_all_build_a_selection() {
        let mut state = InputState::new("hello");
        state.home();
        assert!(state.right_extend());
        assert!(state.right_extend());
        assert_eq!(state.selection(), Some((0, 2)));
        assert_eq!(state.selected(), "he");

        state.select_all();
        assert_eq!(state.selection(), Some((0, 5)));
    }

    #[test]
    fn typing_replaces_the_selection() {
        let mut state = InputState::new("hello");
        state.home();
        state.right_extend();
        state.right_extend();
        assert!(state.insert("YE"));
        assert_eq!(state.text, "YEllo");
        assert_eq!(state.selection(), None);
    }

    #[test]
    fn backspace_deletes_the_selection_before_the_caret() {
        let mut state = InputState::new("hello");
        state.home();
        state.right_extend();
        state.right_extend();
        assert!(state.backspace());
        assert_eq!(state.text, "llo");
        assert_eq!(state.caret, 0);
    }

    #[test]
    fn moving_without_shift_collapses_the_selection() {
        let mut state = InputState::new("hello");
        state.home();
        state.right_extend();
        state.right_extend();
        assert!(state.end());
        assert_eq!(state.selection(), None);

        state.home();
        state.right_extend();
        assert!(state.left());
        assert_eq!(state.selection(), None);
        assert_eq!(state.caret, 0);
    }

    #[test]
    fn typing_is_undone_in_one_step() {
        let mut state = InputState::new("");
        state.insert("h");
        state.insert("i");
        assert_eq!(state.text, "hi");
        assert!(state.can_undo());
        assert!(state.undo());
        assert_eq!(state.text, "");
        assert!(state.can_redo());
        assert!(state.redo());
        assert_eq!(state.text, "hi");
    }

    #[test]
    fn deleting_is_undone_in_one_step() {
        let mut state = InputState::new("abcd");
        state.backspace();
        state.backspace();
        assert_eq!(state.text, "ab");
        assert!(state.undo());
        assert_eq!(state.text, "abcd");
    }

    #[test]
    fn moving_the_caret_starts_a_new_undo_group() {
        let mut state = InputState::new("");
        state.insert("ab");
        state.home();
        state.insert("x");
        assert_eq!(state.text, "xab");
        assert!(state.undo());
        assert_eq!(state.text, "ab");
        assert!(state.undo());
        assert_eq!(state.text, "");
    }

    #[test]
    fn an_external_value_clears_the_history() {
        let mut state = InputState::new("a");
        state.insert("b");
        assert!(state.can_undo());
        state.set_text("z");
        assert!(!state.can_undo());
        assert_eq!(state.text, "z");
    }

    #[test]
    fn replacing_a_selection_is_one_step() {
        let mut state = InputState::new("hello");
        state.home();
        state.right_extend();
        state.right_extend();
        assert!(state.insert("YE"));
        assert_eq!(state.text, "YEllo");
        assert!(state.undo());
        assert_eq!(state.text, "hello");
    }
}
