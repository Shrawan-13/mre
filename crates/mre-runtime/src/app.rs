//! The application shell: Lua in, pixels out.
//!
//! One `frame` does exactly three things: flush reactive work, rebuild the view
//! if the tree changed, and (later) paint. Nothing polls, nothing ticks at 60 Hz
//! while idle.

use crate::convert::{Binding, ConvertedView, Handlers, convert_root};
use mre_core::{Insets, Point, Size, Viewport};
use mre_lua::Runtime;
use mre_raster::Canvas;
use mre_text::TextStore;
use mre_widgets::ui::EditOp;
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
    bindings: Vec<Binding>,
    force_rebuild: bool,
    dirty: bool,
    last_runs: u64,
    drag_target: Option<String>,
    selecting: bool,
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
            handlers: Handlers::default(),
            bindings: Vec::new(),
            force_rebuild: true,
            dirty: true,
            last_runs: 0,
            drag_target: None,
            selecting: false,
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

        // Apply animated properties straight from the springs. No Lua runs and
        // no view is rebuilt: this is the tier that keeps a hover, a scroll or
        // a spring from re-executing the component that declared it.
        if !self.bindings.is_empty() {
            let values = self.runtime.springs();
            let mut changed = false;
            for binding in &self.bindings {
                if let Some((_, value)) = values.iter().find(|(id, _)| *id == binding.spring) {
                    changed |= self.ui.set_animated(&binding.key, binding.property, *value);
                }
            }
            self.dirty |= changed;
        }
        Ok(())
    }

    fn rebuild(&mut self) -> Result<(), Error> {
        let Some(table) = self.runtime.view()? else {
            return Ok(());
        };
        let profile = std::env::var_os("MRE_PROFILE").is_some();
        let start = std::time::Instant::now();
        let ConvertedView {
            view,
            handlers,
            bindings,
        } = convert_root(&table, &self.theme)?;
        let converted_at = start.elapsed();
        self.ui.set_view(&view);
        let set_view_at = start.elapsed();
        self.ui.layout_viewport(&self.viewport);
        if profile {
            eprintln!(
                "rebuild: convert {:?}, set_view {:?}, layout {:?}",
                converted_at,
                set_view_at - converted_at,
                start.elapsed() - set_view_at
            );
        }
        self.handlers = handlers;
        self.bindings = bindings;
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

    /// Moves the pointer. Returns `true` when hover state changed.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        let changed = self.ui.pointer_move(Point::new(x, y));
        self.dirty |= changed;
        changed
    }

    /// Presses the pointer. Returns `true` when the pressed target changed.
    pub fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        let changed =
            self.ui.pointer_move(Point::new(x, y)) | self.ui.pointer_down(Point::new(x, y));
        self.ui.focus_at(Point::new(x, y));
        self.selecting = self.ui.input_at(Point::new(x, y)).is_some();
        if self.selecting {
            self.ui.place_caret(Point::new(x, y), false);
        }
        self.drag_target = self.ui.pointer_up_target();
        self.dirty = true;
        changed
    }

    /// Inserts text into the focused field.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn insert_text(&mut self, text: &str) -> Result<bool, Error> {
        if !self.ui.insert_text(text) {
            return Ok(false);
        }
        let key = self.ui.focused().map(str::to_string);
        self.after_edit(key)?;
        Ok(true)
    }

    /// Applies an editing key to the focused field.
    ///
    /// Recognised names are `backspace`, `delete`, `left`, `right`, `home`,
    /// `end`, `enter`, `escape` and `tab`. Returns `true` when something
    /// changed.
    ///
    /// # Errors
    ///
    /// Returns a handler's Lua error.
    pub fn key(&mut self, name: &str, shift: bool) -> Result<bool, Error> {
        let op = match name {
            "backspace" => Some(EditOp::Backspace),
            "delete" => Some(EditOp::Delete),
            "left" => Some(EditOp::Left),
            "right" => Some(EditOp::Right),
            "home" => Some(EditOp::Home),
            "end" => Some(EditOp::End),
            _ => None,
        };
        if let Some(op) = op {
            if !self.ui.edit(op, shift) {
                return Ok(false);
            }
            let key = self.ui.focused().map(str::to_string);
            self.after_edit(key)?;
            return Ok(true);
        }

        match name {
            "escape" => {
                let changed = self.ui.clear_focus();
                self.dirty |= changed;
                Ok(changed)
            }
            "tab" => {
                let changed = self.ui.focus_next(!shift);
                self.dirty |= changed;
                Ok(changed)
            }
            "enter" | "space" => {
                let Some(key) = self.ui.focused().map(str::to_string) else {
                    return Ok(false);
                };
                if self.ui.input_text(&key).is_some() {
                    if name == "space" {
                        return self.insert_text(" ");
                    }
                    let Some(handler) = self.handlers.submit.get(&key).cloned() else {
                        return Ok(false);
                    };
                    let value = self.ui.input_text(&key).unwrap_or_default().to_string();
                    handler.call::<()>(value)?;
                    self.runtime.flush()?;
                    self.force_rebuild = true;
                    self.frame()?;
                    return Ok(true);
                }
                let Some(handler) = self.handlers.click.get(&key).cloned() else {
                    return Ok(false);
                };
                handler.call::<()>((0.0, 0.0))?;
                self.runtime.flush()?;
                self.force_rebuild = true;
                self.frame()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn after_edit(&mut self, key: Option<String>) -> Result<(), Error> {
        self.dirty = true;
        let Some(key) = key else {
            return Ok(());
        };
        let Some(handler) = self.handlers.change.get(&key).cloned() else {
            return Ok(());
        };
        let value = self.ui.input_text(&key).unwrap_or_default().to_string();
        handler.call::<()>(value)?;
        self.runtime.flush()?;
        self.force_rebuild = true;
        self.frame()?;
        Ok(())
    }

    /// The accessibility tree for the current frame.
    #[must_use]
    pub fn semantics(&self) -> Vec<mre_widgets::Semantics> {
        self.ui.semantics()
    }

    /// Replaces the selection in the focused field with pasted text.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn paste(&mut self, text: &str) -> Result<bool, Error> {
        self.insert_text(text)
    }

    /// Selects everything in the focused field.
    pub fn select_all(&mut self) -> bool {
        let changed = self.ui.select_all();
        self.dirty |= changed;
        changed
    }

    /// The selected text in the focused field, for a clipboard write.
    #[must_use]
    pub fn copied_text(&self) -> Option<String> {
        self.ui.selected_text()
    }

    /// Cuts the selection and returns it.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn cut(&mut self) -> Result<Option<String>, Error> {
        let Some(cut) = self.ui.cut() else {
            return Ok(None);
        };
        let key = self.ui.focused().map(str::to_string);
        self.after_edit(key)?;
        Ok(Some(cut))
    }

    /// True when the focused node is an editable field.
    #[must_use]
    pub fn is_editing(&self) -> bool {
        self.ui
            .focused()
            .is_some_and(|key| self.ui.input_text(key).is_some())
    }

    /// The focused field key, if any.
    #[must_use]
    pub fn focused(&self) -> Option<&str> {
        self.ui.focused()
    }

    /// Moves the pointer, dispatching a drag to the node captured on press.
    ///
    /// `x` and `y` are logical screen coordinates; the handler receives them
    /// relative to the dragged node.
    ///
    /// # Errors
    ///
    /// Returns the drag handler's Lua error.
    pub fn pointer_drag(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        self.pointer_move(x, y);
        if self.selecting {
            let changed = self.ui.place_caret(Point::new(x, y), true);
            self.dirty |= changed;
            return Ok(changed);
        }
        let Some(key) = self.drag_target.clone() else {
            return Ok(false);
        };
        let Some(rect) = self.ui.rect_of_key(&key) else {
            return Ok(false);
        };
        let Some(handler) = self.handlers.drag.get(&key).cloned() else {
            return Ok(false);
        };
        handler.call::<()>((f64::from(x - rect.x), f64::from(y - rect.y)))?;
        self.runtime.flush()?;
        self.frame()?;
        Ok(true)
    }

    /// Releases the pointer and runs the target's click handler, if any.
    ///
    /// # Errors
    ///
    /// Returns the handler's Lua error.
    pub fn pointer_up(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        self.ui.pointer_move(Point::new(x, y));
        self.drag_target = None;
        self.selecting = false;
        self.dirty = true;
        let Some(key) = self.ui.pointer_up() else {
            return Ok(false);
        };
        let Some(handler) = self.handlers.click.get(&key).cloned() else {
            return Ok(false);
        };
        let rect = self.ui.rect_of_key(&key).unwrap_or_default();
        handler.call::<()>((f64::from(x - rect.x), f64::from(y - rect.y)))?;
        self.runtime.flush()?;
        self.force_rebuild = true;
        self.frame()?;
        Ok(true)
    }

    /// Sends a complete click at a logical position.
    ///
    /// Returns `true` when a handler ran.
    ///
    /// # Errors
    ///
    /// Returns the handler's Lua error.
    pub fn click(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        self.pointer_move(x, y);
        self.pointer_down(x, y);
        self.pointer_up(x, y)
    }

    /// Sends a scroll event at a logical position to the nearest node with a
    /// scroll handler.
    ///
    /// `dx` and `dy` are logical pixels; positive `dy` means "scroll down", so
    /// content moves up. Returns `true` when a handler ran.
    ///
    /// # Errors
    ///
    /// Returns the handler's Lua error.
    pub fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> Result<bool, Error> {
        let Some(mut node) = self.ui.hit_test(Point::new(x, y)) else {
            return Ok(false);
        };

        loop {
            let key = self.ui.key(node).map(str::to_string);
            if let Some(key) = key {
                if let Some(handler) = self.handlers.scroll.get(&key) {
                    handler.call::<()>((f64::from(dx), f64::from(dy)))?;
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
