//! MRE's application layer: run a Lua program, get pixels.
//!
//! ```no_run
//! use mre_runtime::App;
//!
//! let source = r#"
//!   local count = mre.signal(0)
//!   mre.app {
//!     view = function()
//!       return mre.column {
//!         padding = 24,
//!         mre.text { "Count: " .. count(), size = 24 },
//!         mre.button { "Increment", on_click = function() count(count() + 1) end },
//!       }
//!     end,
//!   }
//! "#;
//!
//! let mut app = App::new(source).unwrap();
//! app.resize(mre_core::Size::new(800.0, 600.0), 1.0, mre_core::Insets::ZERO);
//! app.frame().unwrap();
//! let canvas = app.paint();
//! assert!(canvas.width() > 0);
//! ```

pub mod app;
pub mod convert;

pub use app::{App, AppStats, Error};
pub use convert::{ConvertedView, Handlers, convert, convert_root};
