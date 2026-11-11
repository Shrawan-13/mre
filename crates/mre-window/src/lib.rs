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
use mre_runtime::App;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};
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
        cursor: (0.0, 0.0),
        pressed: false,
        clipboard: arboard::Clipboard::new().ok(),
        modifiers: ModifiersState::empty(),
        last_tick: Instant::now(),
        tracker: Tracker::default(),
    };
    event_loop.run_app(&mut runner)?;
    Ok(())
}

struct Runner {
    app: App,
    title: String,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
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

    fn draw(&mut self) {
        let Some(window) = &self.window else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
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

        {
            // The view is rebuilt before it is painted: a signal may have
            // changed since the last frame.
            if let Err(error) = self.app.frame() {
                eprintln!("frame failed: {error}");
                return;
            }
            let canvas = self.app.paint();
            for (target, source) in buffer.iter_mut().zip(canvas.pixels().chunks_exact(4)) {
                // softbuffer wants 0x00RRGGBB. The canvas is opaque after the
                // background fill, so dropping alpha is lossless.
                *target = (u32::from(source[0]) << 16)
                    | (u32::from(source[1]) << 8)
                    | u32::from(source[2]);
            }
        }

        let _ = buffer.present();
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
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
            Ok(window) => Rc::new(window),
            Err(error) => {
                eprintln!("window creation failed: {error}");
                event_loop.exit();
                return;
            }
        };
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
                let shift = self.modifiers.shift_key();
                let ctrl = self.modifiers.control_key();
                if ctrl {
                    let outcome = match &event.logical_key {
                        Key::Character(text) if text.as_str() == "c" || text.as_str() == "C" => {
                            if let (Some(clipboard), Some(selected)) =
                                (self.clipboard.as_mut(), self.app.copied_text())
                            {
                                let _ = clipboard.set_text(selected);
                            }
                            Ok(false)
                        }
                        Key::Character(text) if text.as_str() == "x" || text.as_str() == "X" => {
                            match self.app.cut() {
                                Ok(Some(cut)) => {
                                    if let Some(clipboard) = self.clipboard.as_mut() {
                                        let _ = clipboard.set_text(cut);
                                    }
                                    Ok(true)
                                }
                                Ok(None) => Ok(false),
                                Err(error) => Err(error),
                            }
                        }
                        Key::Character(text) if text.as_str() == "v" || text.as_str() == "V" => {
                            let pasted = self
                                .clipboard
                                .as_mut()
                                .and_then(|clipboard| clipboard.get_text().ok());
                            match pasted {
                                Some(text) => self.app.paste(&text),
                                None => Ok(false),
                            }
                        }
                        Key::Character(text) if text.as_str() == "a" || text.as_str() == "A" => {
                            Ok(self.app.select_all())
                        }
                        Key::Character(text) if text.as_str() == "z" || text.as_str() == "Z" => {
                            if shift {
                                self.app.redo()
                            } else {
                                self.app.undo()
                            }
                        }
                        Key::Character(text) if text.as_str() == "y" || text.as_str() == "Y" => {
                            self.app.redo()
                        }
                        _ => Ok(false),
                    };
                    match outcome {
                        Ok(true) => self.request_redraw(),
                        Ok(false) => {}
                        Err(error) => eprintln!("shortcut failed: {error}"),
                    }
                    return;
                }
                let outcome = match &event.logical_key {
                    Key::Named(NamedKey::Backspace) => self.app.key("backspace", shift),
                    Key::Named(NamedKey::Delete) => self.app.key("delete", shift),
                    Key::Named(NamedKey::ArrowLeft) => self.app.key("left", shift),
                    Key::Named(NamedKey::ArrowRight) => self.app.key("right", shift),
                    Key::Named(NamedKey::Home) => self.app.key("home", shift),
                    Key::Named(NamedKey::End) => self.app.key("end", shift),
                    Key::Named(NamedKey::Enter) => self.app.key("enter", shift),
                    Key::Named(NamedKey::Escape) => self.app.key("escape", shift),
                    Key::Named(NamedKey::Tab) => self.app.key("tab", shift),
                    Key::Named(NamedKey::Space) => {
                        if self.app.is_editing() {
                            self.app.insert_text(" ")
                        } else {
                            self.app.key("space", shift)
                        }
                    }
                    _ => match event.text.as_deref() {
                        Some(text) => self.app.insert_text(text),
                        None => Ok(false),
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
                if state == ElementState::Pressed {
                    self.pressed = true;
                    self.tracker.clear();
                } else if self.pressed {
                    self.pressed = false;
                    self.tracker.clear();
                    let scale = self
                        .window
                        .as_ref()
                        .map_or(1.0, |window| window.scale_factor())
                        as f32;
                    let x = self.cursor.0 as f32 / scale;
                    let y = self.cursor.1 as f32 / scale;
                    if let Err(error) = self.app.click(x, y) {
                        eprintln!("click handler failed: {error}");
                    }
                    self.request_redraw();
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

        if self.app.needs_frame() {
            self.request_redraw();
        }
        // No polling: an idle app with no animation costs nothing.
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}
