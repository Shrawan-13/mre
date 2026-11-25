//! Android host for MRE.
//!
//! The APK only needs `android.app.NativeActivity` and this library as
//! `libmain.so`; there is no Java/Kotlin glue to write. Put your Lua program
//! in `app/src/main/assets/main.lua`. If it is missing, the bundled dashboard
//! demo runs instead, so a first build shows something immediately.
//!
//! Rendering goes through the same [`mre_paint::Backend`] as the desktop host.
//! Modern Android devices expose Vulkan to wgpu; on devices without it, the
//! app logs the failure instead of silently showing a blank window.

#![cfg(target_os = "android")]

use mre_core::{Insets, Size};
use mre_gpu::GpuBackend;
use mre_paint::Backend as _;
use mre_runtime::{App, Modifiers};
use mre_text::{FontDesc, TextEngine, TextStore};
use std::ffi::CString;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp as WinitAndroidApp;
use winit::window::{Window, WindowId};

/// Lua program loaded from the APK assets.
const MAIN_LUA: &str = "main.lua";

fn lua_source(app: &WinitAndroidApp) -> String {
    let Ok(name) = CString::new(MAIN_LUA) else {
        return demo();
    };
    let Some(mut asset) = app.asset_manager().open(name.as_c_str()) else {
        eprintln!("mre-android: no assets/{MAIN_LUA}; using the bundled demo");
        return demo();
    };
    let mut bytes = Vec::new();
    match asset.read_to_end(&mut bytes) {
        Ok(_) => match String::from_utf8(bytes) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("mre-android: assets/{MAIN_LUA} is not UTF-8: {error}");
                demo()
            }
        },
        Err(error) => {
            eprintln!("mre-android: could not read assets/{MAIN_LUA}: {error}");
            demo()
        }
    }
}

fn demo() -> String {
    include_str!("../../../examples/dashboard.lua").to_string()
}

/// Fonts are embedded: Android's asset manager has no `fontdb` filesystem
/// view, and a missing font would make text rendering silently empty.
fn text_store() -> TextStore {
    let mut engine = TextEngine::new();
    let font: &[u8] = include_bytes!("../../../assets/fonts/Roboto-Regular.ttf");
    engine.add_font(font.to_vec(), 0, FontDesc::new("Roboto"));
    TextStore::new(engine)
}

struct Runner {
    app: App,
    window: Option<Arc<Window>>,
    gpu: Option<GpuBackend>,
    gpu_size: (u32, u32),
    last_tick: Instant,
    last_touch: Option<(Instant, f32, f32)>,
}

impl Runner {
    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn modifiers(&self) -> Modifiers {
        // NativeActivity routes hardware modifier keys through winit; software
        // keyboards report them as text, handled in `key_with`/`insert_text`.
        Modifiers::default()
    }
}

impl ApplicationHandler for Runner {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Ok(window) = event_loop.create_window(Window::default_attributes()) else {
            eprintln!("mre-android: window creation failed");
            event_loop.exit();
            return;
        };
        let window = Arc::new(window);
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let physical = (size.width.max(1), size.height.max(1));

        match GpuBackend::for_window(window.clone(), physical.0, physical.1, scale) {
            Ok(gpu) => {
                eprintln!("mre-android: GPU backend active — {}", gpu.info());
                self.gpu = Some(gpu);
                self.gpu_size = physical;
            }
            Err(error) => {
                // No software presenter exists on Android: a blank window is
                // worse than a clear diagnostic, so say it out loud.
                eprintln!("mre-android: GPU unavailable ({error}) — nothing can present");
            }
        }
        self.window = Some(window);
        self.app.resize(
            Size::new(size.width as f32, size.height as f32),
            scale,
            Insets::ZERO,
        );
        self.app.repaint_all();
        self.request_redraw();
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) | WindowEvent::ScaleFactorChanged { .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor() as f32);
                let physical = self
                    .window
                    .as_ref()
                    .map_or(size, |window| window.inner_size());
                let physical = (physical.width.max(1), physical.height.max(1));
                if let Some(gpu) = self.gpu.as_mut() {
                    if physical != self.gpu_size {
                        gpu.resize(physical.0, physical.1);
                        self.gpu_size = physical;
                    }
                }
                self.app.resize(
                    Size::new(physical.0 as f32 / scale, physical.1 as f32 / scale),
                    scale,
                    Insets::ZERO,
                );
                self.app.repaint_all();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.app.frame() {
                    eprintln!("mre-android: frame failed: {error}");
                    return;
                }
                if let Some(gpu) = self.gpu.as_mut() {
                    let _ = self.app.paint_with(gpu);
                }
            }
            WindowEvent::Touch(touch) => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor() as f32);
                let x = touch.location.x as f32 / scale;
                let y = touch.location.y as f32 / scale;
                match touch.phase {
                    TouchPhase::Started => {
                        self.last_touch = Some((Instant::now(), x, y));
                        self.app.touch_start(x, y);
                    }
                    TouchPhase::Moved => {
                        self.app.touch_move(x, y);
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        let (vx, vy) =
                            self.last_touch
                                .take()
                                .map_or((0.0, 0.0), |(at, from_x, from_y)| {
                                    let dt = at.elapsed().as_secs_f32().max(0.001);
                                    ((x - from_x) / dt, (y - from_y) / dt)
                                });
                        let _ = self.app.touch_end(x, y, vx, vy);
                    }
                }
                self.request_redraw();
            }
            WindowEvent::KeyboardInput { event: key, .. } if key.state == ElementState::Pressed => {
                // The engine owns text editing; the host only forwards keys.
                // Named keys are routed through the same table as the desktop.
                if let Some(name) = key_name(&key) {
                    let _ = self.app.key_with(name, self.modifiers());
                } else if let Key::Character(text) = &key.key {
                    let _ = self.app.insert_text(text);
                }
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;

        if self.app.animating() || self.app.has_focused_input() {
            let _ = self.app.tick(dt);
            self.request_redraw();
            event_loop.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(16)));
            return;
        }
        if self.app.needs_frame() {
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Keys the editing layer understands. Unknown keys fall through to text,
/// which is how an IME commits a composition.
fn key_name(event: &KeyEvent) -> Option<&'static str> {
    match &event.key {
        Key::Named(NamedKey::Backspace) => Some("backspace"),
        Key::Named(NamedKey::Delete) => Some("delete"),
        Key::Named(NamedKey::ArrowLeft) => Some("left"),
        Key::Named(NamedKey::ArrowRight) => Some("right"),
        Key::Named(NamedKey::Home) => Some("home"),
        Key::Named(NamedKey::End) => Some("end"),
        Key::Named(NamedKey::Enter) => Some("enter"),
        Key::Named(NamedKey::Escape) => Some("escape"),
        Key::Named(NamedKey::Tab) => Some("tab"),
        Key::Named(NamedKey::Space) => Some("space"),
        _ => None,
    }
}

/// Entry point called by `android.app.NativeActivity` on its main-loop thread.
#[unsafe(no_mangle)]
fn android_main(app: WinitAndroidApp) {
    let event_loop = match EventLoop::builder().with_android_app(app.clone()).build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("mre-android: event loop failed: {error}");
            return;
        }
    };
    let source = lua_source(&app);
    let state = match App::with_text_store(&source, text_store()) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("mre-android: Lua program failed: {error}");
            return;
        }
    };
    let mut runner = Runner {
        app: state,
        window: None,
        gpu: None,
        gpu_size: (0, 0),
        last_tick: Instant::now(),
        last_touch: None,
    };
    if let Err(error) = event_loop.run_app(&mut runner) {
        eprintln!("mre-android: event loop exited with {error}");
    }
}
