//! The application shell: Lua in, pixels out.
//!
//! One `frame` does exactly three things: flush reactive work, rebuild the view
//! if the tree changed, and (later) paint. Nothing polls, nothing ticks at 60 Hz
//! while idle.

use crate::convert::{Binding, ConvertedView, Handlers, convert_root};
use mre_core::Rect;
use mre_core::{Insets, Point, Size, Viewport};
use mre_lua::Runtime;
use mre_paint::Backend;
use mre_raster::Canvas;
use mre_text::TextStore;
use mre_widgets::ui::EditOp;
use mre_widgets::{Theme, Ui};
use std::time::Instant;

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
    /// A palette change requested from Lua, applied at frame start.
    theme_request: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    dirty: bool,
    last_runs: u64,
    drag_target: Option<String>,
    selecting: bool,
    rebuilds: u64,
    layout_runs: u64,
    frames: u64,
    /// Where the pointer went down, for the click-versus-drag decision.
    press_origin: Option<Point>,
    /// Whether the pointer moved far enough to cancel the click.
    pressed_moved: bool,
    /// The scroll container a touch started on.
    touch_scroll: Option<String>,
    /// The last touch position, for the scroll delta.
    touch_last: Option<Point>,
    /// A scroll fling in progress.
    fling: Option<Fling>,
    /// The node the pointer was over last time it moved.
    hovered: Option<String>,
    /// The last click and how many clicks in a row it was, for the
    /// double- and triple-click detection.
    last_click: Option<(Instant, Point, u32)>,
    /// Whether the last click landed on a field.
    last_click_in_field: bool,
}

/// The modifier keys held down while a key was pressed.
///
/// The editing keys care about these: control moves by word, shift extends
/// the selection, and alt does the same as control on the platforms where
/// that is the convention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift is held.
    pub shift: bool,
    /// Control is held.
    pub ctrl: bool,
    /// Alt is held.
    pub alt: bool,
}

impl Modifiers {
    /// True when the key means "by word".
    #[must_use]
    pub fn word(&self) -> bool {
        self.ctrl || self.alt
    }
}

/// A scroll fling: the container that scrolls and how fast.
#[derive(Clone, Debug)]
struct Fling {
    /// The node whose scroll handler receives the deltas.
    key: String,
    /// Velocity in logical pixels per second.
    vx: f32,
    /// Velocity in logical pixels per second.
    vy: f32,
}

/// How far the pointer may travel before a press stops being a click.
const CLICK_SLOP: f32 = 6.0;

/// How long two clicks may be apart and still count as a double click.
const DOUBLE_CLICK_TIME: std::time::Duration = std::time::Duration::from_millis(400);

/// Below this speed a fling has stopped.
const FLING_MIN_SPEED: f32 = 24.0;

/// How quickly a fling loses speed, per second.
const FLING_FRICTION: f32 = 3.2;

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
        Self::with_text_store_and_loader(source, texts, |_| None)
    }

    /// Like [`Self::new`], but with a runtime Lua module loader so themes,
    /// styles and animation helpers can be edited without rebuilding a binary.
    pub fn with_lua_loader<F>(source: &str, loader: F) -> Result<Self, Error>
    where
        F: Fn(&str) -> Option<String> + 'static,
    {
        Self::with_text_store_and_loader(source, TextStore::with_system_fonts(), loader)
    }

    /// Runs a Lua program with an explicit text store and module loader.
    pub fn with_text_store_and_loader<F>(
        source: &str,
        texts: TextStore,
        loader: F,
    ) -> Result<Self, Error>
    where
        F: Fn(&str) -> Option<String> + 'static,
    {
        let runtime = Runtime::with_loader(loader)?;
        let theme_request = runtime.theme_request();
        let mut app = Self {
            runtime,
            theme_request,
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
            press_origin: None,
            pressed_moved: false,
            touch_scroll: None,
            touch_last: None,
            fling: None,
            hovered: None,
            last_click: None,
            last_click_in_field: false,
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

    /// Re-executes the app entry with a fresh reactive graph, preserving only
    /// values declared with `mre.persist`. Used by the dev client after a new
    /// Lua bundle arrives; forcing a rebuild keeps the retained tree in sync.
    pub fn reload(&mut self, source: &str) -> Result<(), Error> {
        self.runtime.reload(source)?;
        self.apply_viewport();
        self.force_rebuild = true;
        self.dirty = true;
        Ok(())
    }

    /// Installs the host callback behind `mre.platform_call`.
    pub fn set_platform_call<F>(&mut self, call: F)
    where
        F: Fn(&str, &str, &str) -> String + 'static,
    {
        self.runtime.set_platform_call(call);
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
        // The canvas was recreated, so its pixels are gone: everything has to
        // be painted again.
        self.ui.damage_all();
        self.ui.layout_viewport(&self.viewport);
        // Publish the new size to Lua: a view that reads `mre.viewport()` is
        // rebuilt, so an app can adapt its structure and not only its layout.
        self.runtime.set_viewport(
            self.viewport.size.w,
            self.viewport.size.h,
            self.viewport.scale,
        );
        let insets = self.viewport.insets;
        self.runtime
            .set_safe_area(insets.top, insets.right, insets.bottom, insets.left);
        self.dirty = true;
    }

    /// Flushes reactive work and rebuilds the view if it changed.
    ///
    /// # Errors
    ///
    /// Returns a Lua error from an effect or a corrupted view.
    pub fn frame(&mut self) -> Result<(), Error> {
        self.frames += 1;

        // A theme change is a data change, applied before the reactive flush,
        // so the view is built once against the new palette.
        let requested = self.theme_request.borrow_mut().take();
        if let Some(name) = requested {
            let theme = if name == "dark" || name == "system" {
                Theme::dark()
            } else {
                Theme::light()
            };
            self.set_theme(theme);
        }

        // Compare against the *last frame*, not against the start of this
        // flush. Runtime::tick() already flushes after advancing timers and
        // springs; measuring only this call's flush made those writes invisible
        // and left animated views stuck on their old tree.
        self.runtime.flush()?;
        let runs = self.runtime.stats().total_runs;

        if runs != self.last_runs || self.force_rebuild {
            self.rebuild()?;
            self.force_rebuild = false;
            self.last_runs = runs;
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
            diagnostics,
        } = convert_root(&table, &self.theme)?;
        self.runtime.set_diagnostics(diagnostics);
        let converted_at = start.elapsed();
        self.ui.set_view_and_layout(&view, &self.viewport);
        let set_view_at = start.elapsed();
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

    /// Rebuilds the view from Lua, as if a dependency had changed.
    ///
    /// This exists for tools and benchmarks that need to measure a warm rebuild
    /// without faking a signal; application code never needs it.
    ///
    /// # Errors
    ///
    /// Returns a Lua error from the view function.
    pub fn refresh(&mut self) -> Result<(), Error> {
        self.force_rebuild = true;
        self.frame()
    }

    /// Paints the current layout and returns the canvas.
    pub fn paint(&mut self) -> &Canvas {
        self.paint_region();
        &self.canvas
    }

    /// Marks the whole surface as needing a repaint on the next paint.
    ///
    /// For hosts: call this whenever the presented buffer's contents stop
    /// being trustworthy — a reallocated surface, a returned swapchain image,
    /// a window manager that dropped the backing store. The next
    /// `paint_region` covers the whole canvas regardless of what changed.
    pub fn repaint_all(&mut self) {
        self.ui.damage_all();
        self.dirty = true;
    }

    /// The painted surface, for hosts that present it themselves.
    #[must_use]
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Paints into an arbitrary backend and reports the painted regions in
    /// physical pixels.
    ///
    /// This is the GPU host's entry point: the damage walk, clip stack and
    /// scroll detection are shared with the software path, only the pixels
    /// go somewhere else. Exactly one [`Backend::finish`] is issued, which is
    /// the frame's single GPU submit.
    pub fn paint_with(&mut self, backend: &mut dyn Backend) -> Vec<Rect> {
        let scale = self.viewport.scale;
        let painted = self.ui.paint(backend);
        backend.finish();
        self.dirty = false;
        painted
            .unwrap_or_default()
            .into_iter()
            .map(|rect| Rect {
                x: rect.x * scale,
                y: rect.y * scale,
                w: rect.w * scale,
                h: rect.h * scale,
            })
            .collect()
    }

    /// Paints and reports the regions that were painted, in physical pixels.
    ///
    /// Empty means nothing changed, so nothing was drawn. A host that
    /// presents the canvas can copy just these regions.
    pub fn paint_region(&mut self) -> Vec<Rect> {
        // Kept separate from `paint_with` to avoid borrowing `self` twice: the
        // software canvas is a field, an external backend is not.
        let scale = self.viewport.scale;
        let painted = self.ui.paint(&mut self.canvas);
        self.dirty = false;
        painted
            .unwrap_or_default()
            .into_iter()
            .map(|rect| Rect {
                x: rect.x * scale,
                y: rect.y * scale,
                w: rect.w * scale,
                h: rect.h * scale,
            })
            .collect()
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
        self.step_fling(dt)?;
        // The caret of a focused field blinks, which is the only thing that
        // changes while an idle app waits for a key.
        let blinked = self.ui.tick_blink(dt);
        self.dirty |= blinked;
        self.frame()?;
        Ok(active || blinked)
    }

    /// True when something changed since the last paint, a reactive effect is
    /// pending, or an animation is still moving.
    ///
    /// A platform loop uses this to skip frames entirely when idle.
    #[must_use]
    pub fn needs_frame(&self) -> bool {
        self.dirty
            || self.runtime.stats().dirty > 0
            || self.runtime.animating()
            || self.fling.is_some()
            // A focused field blinks, so the loop has to keep ticking.
            || self.ui.has_focused_input()
    }

    /// True while something is animating: a spring, or a fling.
    #[must_use]
    pub fn animating(&self) -> bool {
        self.runtime.animating() || self.fling.is_some()
    }

    /// Moves the pointer. Returns `true` when hover state changed.
    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        let changed = self.ui.pointer_move(Point::new(x, y));
        self.dirty |= changed;
        self.sync_hover();
        if let Some(origin) = self.press_origin {
            let moved = (x - origin.x).hypot(y - origin.y) > CLICK_SLOP;
            self.pressed_moved |= moved;
        }
        changed
    }

    /// Presses the pointer. Returns `true` when the pressed target changed.
    pub fn pointer_down(&mut self, x: f32, y: f32) -> bool {
        self.pointer_down_with(x, y, Modifiers::default())
    }

    /// Presses the pointer with its modifiers.
    ///
    /// In a field, a double click selects the word under the pointer and
    /// shift extends the selection from the caret to the pointer.
    pub fn pointer_down_with(&mut self, x: f32, y: f32, mods: Modifiers) -> bool {
        let changed =
            self.ui.pointer_move(Point::new(x, y)) | self.ui.pointer_down(Point::new(x, y));
        self.ui.focus_at(Point::new(x, y));
        self.selecting = self.ui.input_at(Point::new(x, y)).is_some();
        let point = Point::new(x, y);
        if self.selecting {
            let clicks = self.click_count(point);
            match clicks {
                // A double click selects the word, a triple click the lot.
                2 => {
                    self.ui.select_word_at(point);
                }
                3 => {
                    self.ui.select_all();
                }
                _ => {
                    // Shift extends the selection from the caret to the
                    // pointer.
                    self.ui.place_caret(point, mods.shift);
                }
            }
            self.ui.restart_blink();
        }
        self.drag_target = self.ui.pointer_up_target();
        self.press_origin = Some(point);
        self.pressed_moved = false;
        self.dirty = true;
        changed
    }

    /// How many clicks in a row this press is.
    ///
    /// One is a click, two is a double click and three is a triple click.
    fn click_count(&mut self, point: Point) -> u32 {
        let now = Instant::now();
        let continues = self.last_click_in_field
            && self.last_click.is_some_and(|(time, origin, _)| {
                now.duration_since(time) < DOUBLE_CLICK_TIME
                    && (origin.x - point.x).hypot(origin.y - point.y) < CLICK_SLOP
            });
        let count = if continues {
            self.last_click.map_or(1, |(_, _, count)| count + 1)
        } else {
            1
        };
        self.last_click = Some((now, point, count));
        self.last_click_in_field = self.selecting;
        count
    }

    /// Pushes the hover state into Lua.
    ///
    /// Only keys that asked for a hover signal are affected, so an app that
    /// never uses `mre.hover` pays nothing.
    fn sync_hover(&mut self) {
        let current = self.ui.hovered().map(str::to_string);
        if current == self.hovered {
            return;
        }
        if let Some(previous) = self.hovered.take() {
            self.runtime.set_hover(&previous, false);
        }
        if let Some(current) = &current {
            self.runtime.set_hover(current, true);
        }
        self.hovered = current;
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

    /// Replaces the focused field's text and selection in one step.
    ///
    /// This is the entry point for a platform IME: on Android a real `EditText`
    /// owns composition and selection, and reports its complete state whenever
    /// it changes. `caret` and `anchor` are character indices; `preedit` is the
    /// uncommitted composition shown at the caret.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn set_text(
        &mut self,
        text: &str,
        caret: usize,
        anchor: usize,
        preedit: &str,
    ) -> Result<bool, Error> {
        if !self.ui.replace_input(text, caret, anchor, preedit) {
            return Ok(false);
        }
        let key = self.ui.focused().map(str::to_string);
        self.after_edit(key)?;
        Ok(true)
    }

    /// The focused field's text, selection and caret, for an IME to mirror.
    #[must_use]
    pub fn focused_input_state(&self) -> Option<(String, usize, usize)> {
        let key = self.ui.focused()?;
        let text = self.ui.input_text(key)?.to_string();
        let (start, end) = self.ui.input_selection(key).unwrap_or_else(|| {
            let caret = self.ui.input_caret(key).unwrap_or(0);
            (caret, caret)
        });
        Some((text, start, end))
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
        self.key_with(
            name,
            Modifiers {
                shift,
                ..Modifiers::default()
            },
        )
    }

    /// Applies a key with its modifiers to the focused field.
    ///
    /// Control (or alt) moves and deletes by word, shift extends the
    /// selection, and control with "a" selects everything. The recognised
    /// names are `backspace`, `delete`, `left`, `right`, `home`, `end`,
    /// `enter`, `escape`, `tab` and `a`.
    ///
    /// # Errors
    ///
    /// Returns a handler's Lua error.
    pub fn key_with(&mut self, name: &str, mods: Modifiers) -> Result<bool, Error> {
        let extend = mods.shift;
        let word = mods.word();
        let op = match name {
            "backspace" => Some(if word {
                EditOp::DeleteWordBackward
            } else {
                EditOp::Backspace
            }),
            "delete" => Some(if word {
                EditOp::DeleteWordForward
            } else {
                EditOp::Delete
            }),
            "left" => Some(if word { EditOp::WordLeft } else { EditOp::Left }),
            "right" => Some(if word {
                EditOp::WordRight
            } else {
                EditOp::Right
            }),
            "home" => Some(EditOp::Home),
            "end" => Some(EditOp::End),
            "a" if mods.ctrl => Some(EditOp::SelectAll),
            _ => None,
        };
        if let Some(op) = op {
            if !self.ui.edit(op, extend) {
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
                let changed = self.ui.focus_next(!mods.shift);
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
                    // Submitting hands control back to the app: if the view
                    // now declares a different value (a cleared form), it wins
                    // over the editor buffer, focused or not.
                    self.ui.apply_external_value(&key);
                    self.dirty = true;
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

    /// Undoes the last edit in the focused field.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn undo(&mut self) -> Result<bool, Error> {
        if !self.ui.undo() {
            return Ok(false);
        }
        let key = self.ui.focused().map(str::to_string);
        self.after_edit(key)?;
        Ok(true)
    }

    /// Redoes the last undone edit in the focused field.
    ///
    /// # Errors
    ///
    /// Returns the `on_change` handler's Lua error.
    pub fn redo(&mut self) -> Result<bool, Error> {
        if !self.ui.redo() {
            return Ok(false);
        }
        let key = self.ui.focused().map(str::to_string);
        self.after_edit(key)?;
        Ok(true)
    }

    /// Sets the text an input method is composing in the focused field.
    ///
    /// The composition is shown at the caret and is not part of the value until
    /// the input method commits it.
    ///
    /// # Errors
    ///
    /// Returns a Lua error if the frame could not be drawn.
    pub fn set_preedit(&mut self, text: &str) -> Result<bool, Error> {
        if !self.ui.set_preedit(text) {
            return Ok(false);
        }
        self.dirty = true;
        Ok(true)
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

    /// True when a field is focused, so its caret is blinking.
    #[must_use]
    pub fn has_focused_input(&self) -> bool {
        self.ui.has_focused_input()
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
        // The handler may have written a signal, so the view has to be rebuilt
        // even though the flush already ran.
        self.force_rebuild = true;
        self.frame()?;
        Ok(true)
    }

    /// Releases the pointer and runs the target's click handler, if any.
    ///
    /// A press that travelled more than a few pixels is a drag, not a click,
    /// so the click handler does not run.
    ///
    /// # Errors
    ///
    /// Returns the handler's Lua error.
    pub fn pointer_up(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        self.ui.pointer_move(Point::new(x, y));
        self.drag_target = None;
        self.selecting = false;
        self.press_origin = None;
        let moved = self.pressed_moved;
        self.pressed_moved = false;
        self.dirty = true;
        let Some(key) = self.ui.pointer_up() else {
            return Ok(false);
        };
        if moved {
            return Ok(false);
        }
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

    /// Starts a touch.
    ///
    /// A touch that lands on a scroll container scrolls it, and keeps scrolling
    /// it for as long as the finger is down, whatever is under the finger now.
    pub fn touch_start(&mut self, x: f32, y: f32) -> bool {
        self.touch_scroll = self.scroll_target(x, y);
        self.touch_last = Some(Point::new(x, y));
        self.pointer_down(x, y)
    }

    /// Moves a touch.
    ///
    /// Returns `true` when the content scrolled.
    ///
    /// # Errors
    ///
    /// Returns the scroll handler's Lua error.
    pub fn touch_move(&mut self, x: f32, y: f32) -> Result<bool, Error> {
        let last = self.touch_last.replace(Point::new(x, y));
        self.pointer_move(x, y);
        // A finger that started on a draggable widget (a slider, for example)
        // feeds the same `on_drag` a mouse drag does. Only when no drag is
        // active does the movement scroll a container.
        if self.drag_target.is_some() {
            return self.pointer_drag(x, y);
        }
        let (Some(key), Some(last)) = (self.touch_scroll.clone(), last) else {
            return Ok(false);
        };
        // A finger moving up scrolls down, so the content moves up.
        let dx = last.x - x;
        let dy = last.y - y;
        if dx == 0.0 && dy == 0.0 {
            return Ok(false);
        }
        self.scroll_key(&key, dx, dy)
    }

    /// Ends a touch.
    ///
    /// `vx` and `vy` are the velocity of the finger in logical pixels per
    /// second. A fast flick starts a fling; a slow lift does not.
    ///
    /// # Errors
    ///
    /// Returns a handler's Lua error.
    pub fn touch_end(&mut self, x: f32, y: f32, vx: f32, vy: f32) -> Result<bool, Error> {
        let scrolled = self.touch_scroll.take();
        self.touch_last = None;
        let clicked = self.pointer_up(x, y)?;
        if let Some(key) = scrolled {
            self.start_fling(&key, vx, vy);
        }
        Ok(clicked)
    }

    /// Starts a fling on the scroll container under a point.
    ///
    /// `vx` and `vy` are logical pixels per second. The runtime keeps feeding
    /// the container's scroll handler decaying deltas until the movement is too
    /// slow to see. Returns `true` when a fling started.
    ///
    /// # Errors
    ///
    /// Returns a handler's Lua error.
    pub fn fling(&mut self, x: f32, y: f32, vx: f32, vy: f32) -> Result<bool, Error> {
        let Some(key) = self.scroll_target(x, y) else {
            return Ok(false);
        };
        Ok(self.start_fling(&key, vx, vy))
    }

    /// True while a fling is still moving.
    #[must_use]
    pub fn is_flinging(&self) -> bool {
        self.fling.is_some()
    }

    /// The key of the nearest scroll container under a point.
    fn scroll_target(&self, x: f32, y: f32) -> Option<String> {
        let mut node = self.ui.hit_test(Point::new(x, y))?;
        loop {
            let key = self.ui.key(node).map(str::to_string);
            if let Some(key) = key {
                if self.handlers.scroll.contains_key(&key) {
                    return Some(key);
                }
            }
            node = self.ui.tree().parent(node)?;
        }
    }
    /// Sends a scroll delta to a known container.
    fn scroll_key(&mut self, key: &str, dx: f32, dy: f32) -> Result<bool, Error> {
        let Some(handler) = self.handlers.scroll.get(key).cloned() else {
            return Ok(false);
        };
        handler.call::<()>((f64::from(dx), f64::from(dy)))?;
        self.runtime.flush()?;
        self.force_rebuild = true;
        self.frame()?;
        Ok(true)
    }

    /// Starts a fling on a known container.
    fn start_fling(&mut self, key: &str, vx: f32, vy: f32) -> bool {
        if vx.abs() < FLING_MIN_SPEED && vy.abs() < FLING_MIN_SPEED {
            return false;
        }
        if !self.handlers.scroll.contains_key(key) {
            return false;
        }
        self.fling = Some(Fling {
            key: key.to_string(),
            vx,
            vy,
        });
        true
    }

    /// Advances a fling by one frame.
    fn step_fling(&mut self, dt: f32) -> Result<(), Error> {
        let Some(fling) = self.fling.clone() else {
            return Ok(());
        };
        let decay = (-FLING_FRICTION * dt).exp();
        let next = Fling {
            key: fling.key.clone(),
            vx: fling.vx * decay,
            vy: fling.vy * decay,
        };
        if next.vx.abs() < FLING_MIN_SPEED && next.vy.abs() < FLING_MIN_SPEED {
            self.fling = None;
        } else {
            self.fling = Some(next);
        }
        // A finger moving up scrolls down, so the content keeps moving up.
        let dx = -fling.vx * dt;
        let dy = -fling.vy * dt;
        self.scroll_key(&fling.key, dx, dy)?;
        Ok(())
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
