//! The application shell: Lua in, pixels out.
//!
//! One `frame` does exactly three things: flush reactive work, rebuild the view
//! if the tree changed, and (later) paint. Nothing polls, nothing ticks at 60 Hz
//! while idle.

use crate::convert::{Handlers, convert_root};
use mre_core::{Insets, Point, Size, Viewport};
use mre_lua::Runtime;
use mre_raster::Canvas;
use mre_text::TextStore;
use mre_widgets::{Theme, Ui};

/// Anything that can go wrong while driving an app.
#[derive(Debug)]
pub enum Error {
    /// The Lua program raised an error.
    Lua(mlua::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lua(error) => write!(f, "lua: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lua(error) => Some(error),
        }
    }
}

impl From<mlua::Error> for Error {
    fn from(error: mlua::Error) -> Self {
        Self::Lua(error)
    }
}

/// A running Lua application.
pub struct App {
    runtime: Runtime,
    ui: Ui,
    theme: Theme,
    viewport: Viewport,
    canvas: Canvas,
    handlers: Handlers,
    force_rebuild: bool,
    dirty: bool,
    last_runs: u64,
    rebuilds: u64,
    layout_runs: u64,
    frames: u64,
}

impl App {
    /// Runs a Lua program using the platform's fonts.
    ///
    /// If the platform exposes no fonts, text renders as nothing. Use
    /// [`App::with_text_store`] to supply a font set explicitly.
    ///
    /// # Errors
    ///
    /// Returns the Lua error if the program fails to load or render.
    pub fn new(source: &str) -> Result<Self, Error> {
        Self::with_text_store(source, TextStore::with_system_fonts())
    }

    /// Runs a Lua program with an explicit text store.
    ///
    /// # Errors
    ///
    /// Returns the Lua error if the program fails to load or render.
    pub fn with_text_store(source: &str, texts: TextStore) -> Result<Self, Error> {
        let runtime = Runtime::new()?;
        let mut app = Self {
            runtime,
            ui: Ui::new(texts),
            theme: Theme::light(),
            viewport: Viewport::new(Size::new(800.0, 600.0)),
            canvas: Canvas::new(800, 600),
            handlers: Handlers::new(),
            force_rebuild: true,
            dirty: true,
            last_runs: 0,
            rebuilds: 0,
            layout_runs: 0,
            frames: 0,
        };
        app.runtime.exec(source)?;
        app.frame()?;
        Ok(app)
    }

    /// The reactive runtime.
    #[must_use]
    pub const fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// The reactive runtime, mutably.
    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    /// The interface state.
    #[must_use]
    pub const fn ui(&self) -> &Ui {
        &self.ui
    }

    /// The interface state, mutably.
    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }

    /// The active theme.
    #[must_use]
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replaces the theme and forces a rebuild, because colours are resolved
    /// while converting the view.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.ui.set_theme(self.theme.clone());
        self.force_rebuild = true;
        self.dirty = true;
    }

    /// The logical viewport.
    #[must_use]
    pub const fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// Resizes the surface. Physical pixels in, logical layout out.
    ///
    /// This is the single entry point for window resize, DPI change and
    /// rotation. Calling it on every frame is harmless: layout is cached.
    pub fn resize(&mut self, physical: Size, scale: f32, insets: Insets) {
        self.viewport = Viewport::from_physical(physical, scale).with_insets(insets);
        self.apply_viewport();
    }

    /// Rotates the viewport, swapping the safe-area insets with it.
    ///
    /// Desktop hosts normally report rotation as a resize; mobile hosts can call
    /// this directly when the device orientation changes.
    pub fn rotate(&mut self) {
        self.viewport.rotate();
        self.apply_viewport();
    }

    fn apply_viewport(&mut self) {
        let physical = self.viewport.physical();
        self.canvas = Canvas::new(
            u32::try_from(physical.w.max(1.0) as i64).unwrap_or(1),
            u32::try_from(physical.h.max(1.0) as i64).unwrap_or(1),
        );
        self.canvas.set_scale(self.viewport.scale);
        self.ui.layout_viewport(&self.viewport);
        self.dirty = true;
    }

    /// Flushes reactive work and rebuilds the view if it changed.
    ///
    /// # Errors
    ///
    /// Returns a Lua error from an effect or a corrupted view.
    pub fn frame(&mut self) -> Result<(), Error> {
        self.frames += 1;

        let before = self.runtime.stats().total_runs;
        self.runtime.flush()?;
        let after = self.runtime.stats().total_runs;

        if after != before || self.force_rebuild {
            self.rebuild()?;
            self.force_rebuild = false;
            self.last_runs = after;
        }
        Ok(())
    }

    fn rebuild(&mut self) -> Result<(), Error> {
        let Some(table) = self.runtime.view()? else {
            return Ok(());
        };
        let converted = convert_root(&table, &self.theme)?;
        self.ui.set_view(&converted.view);
        self.ui.layout_viewport(&self.viewport);
        self.handlers = converted.handlers;
        self.rebuilds += 1;
        self.layout_runs += 1;
        self.dirty = true;
        Ok(())
    }

    /// Paints the current layout and returns the canvas.
    pub fn paint(&mut self) -> &Canvas {
        self.ui.paint(&mut self.canvas);
        self.dirty = false;
        &self.canvas
    }

    /// Advances animations by `dt` seconds and rebuilds if anything moved.
    ///
    /// Returns `true` while any spring is still running.
    ///
    /// # Errors
    ///
    /// Returns a Lua error raised by an animated signal.
    pub fn tick(&mut self, dt: f32) -> Result<bool, Error> {
        let active = self.runtime.tick(dt)?;
        self.frame()?;
        Ok(active)
    }

    /// True when something changed since the last paint, a reactive effect is
    /// pending, or an animation is still moving.
    ///
    /// A platform loop uses this to skip frames entirely when idle.
    #[must_use]
    pub fn needs_frame(&self) -> bool {
        self.dirty || self.runtime.stats().dirty > 0 || self.runtime.animating()
    }

    /// Sends a click at a logical position to the deepest node with a handler.
    ///
    /// Returns `true` when a handler ran.
    ///
    /// # Errors
    ///
    /// Returns the handler's Lua error.
    pub fn click(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        let Some(mut node) = self.ui.hit_test(Point::new(x, y)) else {
            return Ok(false);
        };

        loop {
            let key = self.ui.key(node).map(str::to_string);
            if let Some(key) = key {
                if let Some(handler) = self.handlers.get(&key) {
                    handler.call::<()>(())?;
                    self.runtime.flush()?;
                    self.force_rebuild = true;
                    self.frame()?;
                    return Ok(true);
                }
            }
            match self.ui.tree().parent(node) {
                Some(parent) => node = parent,
                None => break,
            }
        }
        Ok(false)
    }

    /// The key of the deepest node at a point, for tests and tooling.
    #[must_use]
    pub fn key_at(&self, x: f32, y: f32) -> Option<&str> {
        let node = self.ui.hit_test(Point::new(x, y))?;
        self.ui.key(node)
    }

    /// Diagnostics.
    #[must_use]
    pub fn stats(&self) -> AppStats {
        AppStats {
            frames: self.frames,
            rebuilds: self.rebuilds,
            layout_runs: self.layout_runs,
            nodes: self.ui.node_count(),
        }
    }
}

/// Counters for an app session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppStats {
    /// Frames driven.
    pub frames: u64,
    /// Times the view was rebuilt from Lua.
    pub rebuilds: u64,
    /// Layout passes.
    pub layout_runs: u64,
    /// Layout nodes in the current tree.
    pub nodes: usize,
}
