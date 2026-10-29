//! Widgets for MRE: declarative views, themes and painting.
//!
//! A view is data, not behaviour. [`Ui::set_view`] converts it into a layout
//! tree, [`Ui::layout`] computes geometry, and [`Ui::paint`] draws. Keeping the
//! three phases separate is what lets a resize re-run only layout and a theme
//! change re-run only paint.

pub mod theme;
pub mod ui;
pub mod view;

pub use theme::Theme;
pub use ui::{Ui, UiNode};
pub use view::{Props, View, ViewKind};
