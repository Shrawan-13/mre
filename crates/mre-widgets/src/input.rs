//! Editable text state.
//!
//! Carets are counted in characters, not bytes, so a field holding an emoji or
//! an Arabic word cannot be corrupted by editing. A caret and an anchor model
//! selection: when they differ the range between them is selected, which is the
//! same model every text editor uses. The state is deliberately small and
//! plain; rich text belongs in a later milestone.

/// The editable state of one text field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    /// Current contents.
    pub text: String,
    /// Caret position, in characters from the start.
    pub caret: usize,
    /// Selection anchor. Equal to [`Self::caret`] when nothing is selected.
    pub anchor: usize,
}

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
        }
    }

    /// Replaces the contents from outside the editor, keeping the caret valid.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.caret.min(self.len());
        self.anchor = self.anchor.min(self.len());
        if self.anchor == self.caret {
            self.anchor = self.caret;
        }
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
        let inserted: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        if inserted.is_empty() {
            return false;
        }
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
        if self.delete_selection() {
            return true;
        }
        if self.caret == 0 {
            return false;
        }
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
        if self.delete_selection() {
            return true;
        }
        let mut chars: Vec<char> = self.text.chars().collect();
        if self.caret >= chars.len() {
            return false;
        }
        chars.remove(self.caret);
        self.text = chars.into_iter().collect();
        true
    }

    /// Moves the caret one character left.
    pub fn left(&mut self) -> bool {
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
        if self.text.is_empty() {
            return false;
        }
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
}
