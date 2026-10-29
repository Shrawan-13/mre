//! Text for MRE: shaping, fallback, line breaking and a shaped-run cache.
//!
//! ```
//! use mre_text::{FontDesc, TextStyle, TextEngine};
//!
//! let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/Roboto-Regular.ttf");
//! let mut engine = TextEngine::new();
//! engine.add_font_file(path, FontDesc::new("Roboto")).unwrap();
//!
//! let shaped = engine.shape("Hello, world", &TextStyle::new(16.0), f32::INFINITY);
//! assert!(shaped.width > 0.0);
//! assert_eq!(shaped.lines.len(), 1);
//! ```

pub mod engine;
pub mod shape;
pub mod store;
pub mod style;

pub use engine::{FontDesc, FontId, Stats, TextEngine};
pub use shape::{Glyph, ShapedText, TextLine};
pub use store::{TextRun, TextStore};
pub use style::{TextAlign, TextStyle, Wrap};
