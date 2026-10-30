//! Editable text state.
//!
//! Carets are counted in characters, not bytes, so a field holding an emoji or
//! an Arabic word cannot be corrupted by editing. The state is deliberately
//! small and plain: selection and rich text belong in a later milestone, and
//! pretending they exist now would just make the simple case slower.

/// The editable state of one text field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputState {
    /// Current contents.
    pub text: String,
    /// Caret position, in characters from the start.
    pub caret: usize,
}

impl InputState {
    /// A field containing `text`, with the caret at the end.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let caret = text.chars().count();
        Self { text, caret }
    }

    /// Replaces the contents from outside the editor, keeping the caret valid.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.caret.min(self.len());
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

    /// Inserts text at the caret, ignoring control characters.
    ///
    /// Returns `true` when something was inserted.
    pub fn insert(&mut self, text: &str) -> bool {
        let inserted: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        if inserted.is_empty() {
            return false;
        }
        let mut chars: Vec<char> = self.text.chars().collect();
        let at = self.caret.min(chars.len());
        let count = inserted.len();
        chars.splice(at..at, inserted);
        self.text = chars.into_iter().collect();
        self.caret = at + count;
        true
    }

    /// Deletes the character before the caret.
    pub fn backspace(&mut self) -> bool {
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
        true
    }

    /// Deletes the character after the caret.
    pub fn delete(&mut self) -> bool {
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
        if self.caret == 0 {
            return false;
        }
        self.caret -= 1;
        true
    }

    /// Moves the caret one character right.
    pub fn right(&mut self) -> bool {
        if self.caret >= self.len() {
            return false;
        }
        self.caret += 1;
        true
    }

    /// Moves the caret to the start.
    pub fn home(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        self.caret = 0;
        true
    }

    /// Moves the caret to the end.
    pub fn end(&mut self) -> bool {
        let end = self.len();
        if self.caret == end {
            return false;
        }
        self.caret = end;
        true
    }

    /// Selects everything by moving the caret, without a selection model yet.
    pub fn clear(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.text.clear();
        self.caret = 0;
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
}
