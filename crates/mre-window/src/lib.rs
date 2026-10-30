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
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
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
        last_tick: Instant::now(),
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
    last_tick: Instant,
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
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Pressed {
                    self.pressed = true;
                } else if self.pressed {
                    self.pressed = false;
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
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;

        if self.app.runtime().animating() {
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
