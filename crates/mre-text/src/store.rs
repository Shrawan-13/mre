//! A text store that plugs into layout.
//!
//! Layout refers to text by [`TextId`]; the store owns the strings and styles and
//! implements [`TextMetrics`] by shaping and caching. This is the only bridge
//! between the layout and text crates.

use crate::engine::TextEngine;
use crate::shape::ShapedText;
use crate::style::TextStyle;
use mre_core::Size;
use mre_layout::{TextId, TextMetrics};
use std::sync::Arc;

/// One piece of text owned by a [`TextStore`].
#[derive(Clone, Debug)]
pub struct TextRun {
    /// The string itself.
    pub text: String,
    /// How to shape and lay it out.
    pub style: TextStyle,
}

/// Owns every string the UI can display.
pub struct TextStore {
    runs: Vec<TextRun>,
    engine: TextEngine,
}

impl Default for TextStore {
    fn default() -> Self {
        Self::new(TextEngine::new())
    }
}

impl TextStore {
    /// Creates a store around an engine.
    #[must_use]
    pub fn new(engine: TextEngine) -> Self {
        Self {
            runs: Vec::new(),
            engine,
        }
    }

    /// Creates a store with the platform's default fonts.
    #[must_use]
    pub fn with_system_fonts() -> Self {
        let mut engine = TextEngine::new();
        engine.load_system();
        Self::new(engine)
    }

    /// The shaping engine.
    #[must_use]
    pub const fn engine(&self) -> &TextEngine {
        &self.engine
    }

    /// The shaping engine, mutably.
    pub fn engine_mut(&mut self) -> &mut TextEngine {
        &mut self.engine
    }

    /// Number of text runs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    /// True when no text has been added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Drops every run. The shaping engine and its cache are kept, so a full
    /// view rebuild re-uses shaped text instead of re-shaping it.
    pub fn clear(&mut self) {
        self.runs.clear();
    }

    /// Adds a run and returns its handle.
    pub fn add(&mut self, text: impl Into<String>, style: TextStyle) -> TextId {
        let id = TextId(u32::try_from(self.runs.len()).expect("text store exceeds 4 GiB runs"));
        self.runs.push(TextRun {
            text: text.into(),
            style,
        });
        id
    }

    /// Replaces the string of an existing run.
    ///
    /// The shaped cache is keyed on content, so the old shape becomes garbage
    /// and the new one is computed on demand.
    pub fn set_text(&mut self, id: TextId, text: impl Into<String>) {
        if let Some(run) = self.runs.get_mut(id.0 as usize) {
            run.text = text.into();
        }
    }

    /// Replaces the style of an existing run.
    pub fn set_style(&mut self, id: TextId, style: TextStyle) {
        if let Some(run) = self.runs.get_mut(id.0 as usize) {
            run.style = style;
        }
    }

    /// The string for a run.
    #[must_use]
    pub fn text(&self, id: TextId) -> &str {
        self.runs
            .get(id.0 as usize)
            .map_or("", |run| run.text.as_str())
    }

    /// The style for a run.
    #[must_use]
    pub fn style(&self, id: TextId) -> Option<&TextStyle> {
        self.runs.get(id.0 as usize).map(|run| &run.style)
    }

    /// Shapes a run to a width, reusing the engine cache.
    pub fn shape(&mut self, id: TextId, max_width: f32) -> Arc<ShapedText> {
        let Some(run) = self.runs.get(id.0 as usize) else {
            return Arc::new(ShapedText::default());
        };
        self.engine.shape(&run.text, &run.style, max_width)
    }
}

impl TextMetrics for TextStore {
    fn measure(&mut self, text: TextId, max_width: f32) -> Size {
        self.shape(text, max_width).size()
    }
}
