//! The desktop host.
//!
//! One window, one event loop, one present path. Every platform change that
//! matters to layout (resize, DPI change, rotation) arrives as a resize, so
//! there is a single place where the physical surface is translated into a
//! logical viewport.
//!
//! The presenter here is `softbuffer`, which copies the CPU canvas to the
//! window. That keeps the engine runnable on any machine while the GPU backend
//! lands; the app loop and the canvas contract do not change.

use mre_core::{Insets, Size};
use mre_runtime::{App, Modifiers};
use std::num::NonZeroU32;
// wgpu needs an adapter-safe window handle; `Arc` is Send+Sync on every host
// we support, `Rc` is not.
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(feature = "gpu")]
use mre_gpu::GpuBackend;
#[cfg(feature = "gpu")]
use mre_paint::Backend as _;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

/// Runs an app until the window closes.
///
/// # Errors
///
/// Returns an error if the event loop or window cannot be created.
pub fn run(app: App, title: &str) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoop::new()?;
    let mut runner = Runner {
        app,
        title: title.to_string(),
        window: None,
        surface: None,
        buffer_size: (0, 0),
        cursor: (0.0, 0.0),
        pressed: false,
        clipboard: arboard::Clipboard::new().ok(),
        modifiers: ModifiersState::empty(),
        last_tick: Instant::now(),
        tracker: Tracker::default(),
        #[cfg(feature = "gpu")]
        gpu: None,
        #[cfg(feature = "gpu")]
        gpu_size: (0, 0),
        #[cfg(feature = "gpu")]
        gpu_scale: 1.0,
    };
    event_loop.run_app(&mut runner)?;
    Ok(())
}

/// A shortcut the window handles itself, because it is about the window
/// rather than about the field under the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    Copy,
    Cut,
    Paste,
    Undo,
    Redo,
    SelectAll,
}

struct Runner {
    app: App,
    title: String,
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
    /// The hardware path, when the machine has a usable adapter. Failure to
    /// create it is not fatal: the software presenter stays available.
    #[cfg(feature = "gpu")]
    gpu: Option<GpuBackend>,
    #[cfg(feature = "gpu")]
    gpu_size: (u32, u32),
    /// Density the backend was last configured with. A DPI change reuses the
    /// same physical surface but must rescale every paint operation.
    #[cfg(feature = "gpu")]
    gpu_scale: f32,
    /// The size the present surface was last allocated at.
    ///
    /// After a reallocation the buffer's contents are undefined — X11 with no
    /// compositor happily shows whatever memory happens to be in it, recycled
    /// from other windows — so a size change forces a full repaint.
    buffer_size: (u32, u32),
    cursor: (f64, f64),
    pressed: bool,
    clipboard: Option<arboard::Clipboard>,
    modifiers: ModifiersState,
    last_tick: Instant,
    tracker: Tracker,
}

/// Recent pointer positions, so a release can carry a velocity.
///
/// A flick is a gesture, not a position: the engine needs to know how fast the
/// finger was moving when it left the screen.
#[derive(Default)]
struct Tracker {
    samples: Vec<(Instant, f32, f32)>,
}

impl Tracker {
    /// How long a sample stays relevant.
    const WINDOW: Duration = Duration::from_millis(120);

    /// Records a position.
    fn push(&mut self, now: Instant, x: f32, y: f32) {
        self.samples.push((now, x, y));
        while let Some((time, _, _)) = self.samples.first() {
            if now.duration_since(*time) > Self::WINDOW {
                self.samples.remove(0);
            } else {
                break;
            }
        }
    }

    /// Drops every sample.
    fn clear(&mut self) {
        self.samples.clear();
    }

    /// The velocity in logical pixels per second.
    fn velocity(&self) -> (f32, f32) {
        let Some((first_time, first_x, first_y)) = self.samples.first() else {
            return (0.0, 0.0);
        };
        let Some((last_time, last_x, last_y)) = self.samples.last() else {
            return (0.0, 0.0);
        };
        let elapsed = last_time
            .duration_since(*first_time)
            .as_secs_f32()
            .max(0.001);
        ((last_x - first_x) / elapsed, (last_y - first_y) / elapsed)
    }
}

impl Runner {
    fn sync_surface(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        self.app.resize(
            Size::new(size.width as f32, size.height as f32),
            scale,
            Insets::ZERO,
        );
    }

    /// True when the app's surface does not match the window.
    ///
    /// The window is resized by the window manager, and the app has to be
    /// told; if that message is ever missed, the canvas and the window
    /// disagree and the frame is presented at the wrong size. Checking is
    /// cheap, and it makes the host self-healing.
    fn surface_is_stale(&self) -> bool {
        let Some(window) = &self.window else {
            return false;
        };
        let size = window.inner_size();
        let canvas = self.app.canvas();
        canvas.width() != size.width || canvas.height() != size.height
    }

    fn draw(&mut self) {
        if self.window.is_none() {
            return;
        }
        if self.surface_is_stale() {
            self.sync_surface();
        }
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        #[cfg(feature = "gpu")]
        let scale = window.scale_factor() as f32;

        #[cfg(feature = "gpu")]
        if let Some(gpu) = self.gpu.as_mut() {
            if (scale - self.gpu_scale).abs() > f32::EPSILON {
                gpu.set_scale(scale);
                self.gpu_scale = scale;
                self.app.repaint_all();
            }
            if (size.width, size.height) != self.gpu_size {
                gpu.resize(size.width, size.height);
                self.gpu_size = (size.width, size.height);
                // A resized surface has undefined contents: repaint everything.
                self.app.repaint_all();
            }
            if let Err(error) = self.app.frame() {
                eprintln!("frame failed: {error}");
                return;
            }
            // One paint walk, one submit. The regions come back in physical
            // pixels but the GPU presents the whole persistent texture, so a
            // host-visible damage list is not needed here.
            let _ = self.app.paint_with(gpu);
            return;
        }

        // A reallocation is imminent: the new buffer's contents are undefined,
        // so the whole canvas has to be painted again, logical damage or not.
        if (size.width, size.height) != self.buffer_size {
            self.app.repaint_all();
        }
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        if surface
            .resize(
                NonZeroU32::new(size.width).unwrap_or(NonZeroU32::new(1).expect("non-zero")),
                NonZeroU32::new(size.height).unwrap_or(NonZeroU32::new(1).expect("non-zero")),
            )
            .is_err()
        {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };

        let mut copied = false;
        {
            // The view is rebuilt before it is painted: a signal may have
            // changed since the last frame.
            if let Err(error) = self.app.frame() {
                eprintln!("frame failed: {error}");
                return;
            }
            let regions = self.app.paint_region();
            let canvas = self.app.canvas();
            let width = canvas.width() as usize;
            let height = canvas.height() as usize;
            let pixels = canvas.pixels();

            // Only the damaged regions are copied: the rest of the buffer
            // still holds the previous frame, which is unchanged. An empty
            // region list still presents below — on X11 the server may have
            // thrown the window's content away (any move, occlusion or
            // workspace switch without a compositor), and the buffer is the
            // only copy of it.
            for region in &regions {
                copied = true;
                let x0 = region.x.floor().max(0.0) as usize;
                let y0 = region.y.floor().max(0.0) as usize;
                let x1 = ((region.x + region.w).ceil() as usize).min(width);
                let y1 = ((region.y + region.h).ceil() as usize).min(height);
                for y in y0..y1 {
                    let source = &pixels[y * width * 4 + x0 * 4..y * width * 4 + x1 * 4];
                    let start = y * width + x0;
                    for (index, source) in source.chunks_exact(4).enumerate() {
                        // softbuffer wants 0x00RRGGBB. The canvas is opaque
                        // after the background fill, so dropping alpha is
                        // lossless.
                        buffer[start + index] = (u32::from(source[0]) << 16)
                            | (u32::from(source[1]) << 8)
                            | u32::from(source[2]);
                    }
                }
            }
        }

        let _ = buffer.present();
        if copied {
            self.buffer_size = (size.width, size.height);
        }
    }

    /// The modifier keys held down right now.
    fn modifiers(&self) -> Modifiers {
        Modifiers {
            shift: self.modifiers.shift_key(),
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

/// `MRE_BACKEND=software` opts out of the hardware path. Anything else uses
/// it when a compatible adapter exists.
#[cfg(feature = "gpu")]
fn gpu_requested() -> bool {
    !matches!(
        std::env::var("MRE_BACKEND").as_deref(),
        Ok("software" | "cpu" | "soft")
    )
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title(&self.title)
            .with_inner_size(winit::dpi::LogicalSize::new(900.0, 760.0));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("window creation failed: {error}");
                event_loop.exit();
                return;
            }
        };

        #[cfg(feature = "gpu")]
        if gpu_requested() {
            let size = window.inner_size();
            let scale = window.scale_factor() as f32;
            match GpuBackend::for_window(window.clone(), size.width, size.height, scale) {
                Ok(gpu) => {
                    eprintln!("MRE: GPU backend active — {}", gpu.info());
                    self.gpu = Some(gpu);
                    self.gpu_size = (size.width, size.height);
                    self.gpu_scale = scale;
                    self.window = Some(window);
                    self.sync_surface();
                    self.request_redraw();
                    return;
                }
                Err(error) => {
                    eprintln!("MRE: GPU unavailable ({error}); using the software backend");
                }
            }
        }

        let context = match softbuffer::Context::new(window.clone()) {
            Ok(context) => context,
            Err(error) => {
                eprintln!("present context failed: {error}");
                event_loop.exit();
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(surface) => surface,
            Err(error) => {
                eprintln!("present surface failed: {error}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window);
        self.surface = Some(surface);
        self.sync_surface();
        self.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.sync_surface();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                let mods = self.modifiers();
                let shift = mods.shift;
                let ctrl = mods.ctrl;

                // The clipboard and the undo history belong to the window, not
                // to the field, so they are handled here. Every other
                // control shortcut — the ones that move by word, or delete a
                // word — belongs to the field, and falls through to the app
                // below with its modifiers.
                if ctrl {
                    let shortcut = match &event.logical_key {
                        Key::Character(text) => match text.as_str() {
                            "c" | "C" => Some(Shortcut::Copy),
                            "x" | "X" => Some(Shortcut::Cut),
                            "v" | "V" => Some(Shortcut::Paste),
                            "z" | "Z" => Some(if shift {
                                Shortcut::Redo
                            } else {
                                Shortcut::Undo
                            }),
                            "y" | "Y" => Some(Shortcut::Redo),
                            "a" | "A" => Some(Shortcut::SelectAll),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(shortcut) = shortcut {
                        let outcome = match shortcut {
                            Shortcut::Copy => {
                                if let (Some(clipboard), Some(selected)) =
                                    (self.clipboard.as_mut(), self.app.copied_text())
                                {
                                    let _ = clipboard.set_text(selected);
                                }
                                Ok(false)
                            }
                            Shortcut::Cut => match self.app.cut() {
                                Ok(Some(cut)) => {
                                    if let Some(clipboard) = self.clipboard.as_mut() {
                                        let _ = clipboard.set_text(cut);
                                    }
                                    Ok(true)
                                }
                                Ok(None) => Ok(false),
                                Err(error) => Err(error),
                            },
                            Shortcut::Paste => {
                                let pasted = self
                                    .clipboard
                                    .as_mut()
                                    .and_then(|clipboard| clipboard.get_text().ok());
                                match pasted {
                                    Some(text) => self.app.paste(&text),
                                    None => Ok(false),
                                }
                            }
                            Shortcut::Undo => self.app.undo(),
                            Shortcut::Redo => self.app.redo(),
                            Shortcut::SelectAll => Ok(self.app.select_all()),
                        };
                        match outcome {
                            Ok(true) => self.request_redraw(),
                            Ok(false) => {}
                            Err(error) => eprintln!("shortcut failed: {error}"),
                        }
                        return;
                    }
                }

                let outcome = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => self.app.key_with("backspace", mods),
                    Key::Named(NamedKey::Delete) => self.app.key_with("delete", mods),
                    Key::Named(NamedKey::ArrowLeft) => self.app.key_with("left", mods),
                    Key::Named(NamedKey::ArrowRight) => self.app.key_with("right", mods),
                    Key::Named(NamedKey::Home) => self.app.key_with("home", mods),
                    Key::Named(NamedKey::End) => self.app.key_with("end", mods),
                    Key::Named(NamedKey::Enter) => self.app.key_with("enter", mods),
                    Key::Named(NamedKey::Escape) => self.app.key_with("escape", mods),
                    Key::Named(NamedKey::Tab) => self.app.key_with("tab", mods),
                    Key::Named(NamedKey::Space) => {
                        if self.app.is_editing() {
                            self.app.insert_text(" ")
                        } else {
                            self.app.key_with("space", mods)
                        }
                    }
                    _ => match event.text.as_deref() {
                        // A control shortcut must not type a letter.
                        Some(text) if !ctrl && !mods.alt => self.app.insert_text(text),
                        _ => Ok(false),
                    },
                };
                match outcome {
                    Ok(true) => self.request_redraw(),
                    Ok(false) => {}
                    Err(error) => eprintln!("key handler failed: {error}"),
                }
            }
            WindowEvent::Ime(Ime::Preedit(text, _cursor)) => {
                // The composition is shown at the caret but is not part of the
                // value until the input method commits it.
                if let Err(error) = self.app.set_preedit(&text) {
                    eprintln!("input failed: {error}");
                }
                self.request_redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                let _ = self.app.set_preedit("");
                if let Err(error) = self.app.insert_text(&text) {
                    eprintln!("input failed: {error}");
                }
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor()) as f32;
                // Positive `dy` means "scroll down" everywhere above this line.
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 40.0, y * 40.0),
                    MouseScrollDelta::PixelDelta(position) => {
                        (position.x as f32 / scale, position.y as f32 / scale)
                    }
                };
                let x = self.cursor.0 as f32 / scale;
                let y = self.cursor.1 as f32 / scale;
                if let Err(error) = self.app.scroll(x, y, dx, -dy) {
                    eprintln!("scroll handler failed: {error}");
                }
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor()) as f32;
                let x = position.x as f32 / scale;
                let y = position.y as f32 / scale;
                let outcome = if self.pressed {
                    self.app
                        .pointer_drag(x, y)
                        .map_err(|error| error.to_string())
                } else {
                    Ok(self.app.pointer_move(x, y))
                };
                match outcome {
                    Ok(true) => self.request_redraw(),
                    Ok(false) => {}
                    Err(error) => eprintln!("drag handler failed: {error}"),
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor()) as f32;
                let x = self.cursor.0 as f32 / scale;
                let y = self.cursor.1 as f32 / scale;
                match state {
                    ElementState::Pressed => {
                        self.pressed = true;
                        self.tracker.clear();
                        // The press is forwarded on its own: it places the
                        // caret, and the drag that follows extends the
                        // selection. Sending only the release would make
                        // every drag a click, and text could not be selected.
                        self.app.pointer_down_with(x, y, self.modifiers());
                        self.request_redraw();
                    }
                    ElementState::Released => {
                        if self.pressed {
                            self.pressed = false;
                            self.tracker.clear();
                            if let Err(error) = self.app.pointer_up(x, y) {
                                eprintln!("click handler failed: {error}");
                            }
                            self.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::Touch(touch) => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor()) as f32;
                let x = touch.location.x as f32 / scale;
                let y = touch.location.y as f32 / scale;
                let now = Instant::now();
                match touch.phase {
                    TouchPhase::Started => {
                        self.tracker.clear();
                        self.tracker.push(now, x, y);
                        self.app.touch_start(x, y);
                        self.request_redraw();
                    }
                    TouchPhase::Moved => {
                        self.tracker.push(now, x, y);
                        if let Err(error) = self.app.touch_move(x, y) {
                            eprintln!("scroll handler failed: {error}");
                        }
                        self.request_redraw();
                    }
                    TouchPhase::Ended => {
                        self.tracker.push(now, x, y);
                        let (vx, vy) = self.tracker.velocity();
                        if let Err(error) = self.app.touch_end(x, y, vx, vy) {
                            eprintln!("touch failed: {error}");
                        }
                        self.tracker.clear();
                        self.request_redraw();
                    }
                    TouchPhase::Cancelled => {
                        self.tracker.clear();
                        let _ = self.app.touch_end(x, y, 0.0, 0.0);
                        self.request_redraw();
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;

        if self.app.animating() {
            if let Err(error) = self.app.tick(dt) {
                eprintln!("animation failed: {error}");
            }
            self.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(16)));
            return;
        }

        if self.app.has_focused_input() {
            // A focused field blinks. The loop keeps ticking, but only asks
            // for a repaint when the caret actually changed, which is twice a
            // second rather than sixty.
            match self.app.tick(dt) {
                Ok(true) => self.request_redraw(),
                Ok(false) => {}
                Err(error) => eprintln!("tick failed: {error}"),
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(16)));
            return;
        }

        if self.app.needs_frame() {
            self.request_redraw();
        }
        // No polling: an idle app with no animation costs nothing.
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
