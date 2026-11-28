//! The Android event loop. Everything here is `target_os = "android"` only;
//! the density maths lives in `crate::layout` so it can be unit-tested on a
//! desktop CI runner.

use crate::dev::{self, Bundle, DevConfig};
use crate::frame_rate::FrameClock;
use crate::ime::{self, ImeEvent};
use crate::layout::{app_size, content_insets};
use crate::plugin;
use mre_core::{Insets, Size};
use mre_gpu::GpuBackend;
use mre_paint::Backend as _;
use mre_runtime::{App, Modifiers};
use mre_text::{FontDesc, TextEngine, TextStore};
use std::collections::HashMap;
use std::ffi::CString;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::WindowExtAndroid as _;
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

/// A visible failure screen. A dev client with a pairing file must never
/// silently fall back to the bundled demo: that looks like a successful
/// connection and hides the real problem.
fn dev_error_source(config: &DevConfig) -> String {
    format!(
        r##"
mre.app {{
  view = function()
    return mre.column {{
      padding = 28,
      gap = 12,
      background = "#0B1120",
      mre.text {{
        "Dev client cannot reach mre dev",
        size = 24, weight = 700, color = "#F87171",
      }},
      mre.text {{
        "Target: {}:{}",
        size = 15, color = "#E2E8F0",
      }},
      mre.text {{
        "Start mre dev on the PC, or pair over USB with mre dev --usb.",
        size = 14, color = "#94A3B8",
      }},
    }}
  end,
}}
"##,
        config.host, config.port
    )
}

/// State owned by the dev-server watcher. The long-poll runs on a background
/// thread; only the resulting `ImeEvent::Reload` touches the engine.
struct DevRunner {
    config: DevConfig,
    revision: u64,
    modules: Arc<RwLock<HashMap<String, String>>>,
    stop: Arc<AtomicBool>,
}

struct Runner {
    app: Option<App>,
    android_app: Option<WinitAndroidApp>,
    keyboard_visible: bool,
    window: Option<Arc<Window>>,
    gpu: Option<GpuBackend>,
    gpu_size: (u32, u32),
    gpu_scale: f32,
    last_content: (i32, i32, i32, i32),
    base_insets: Option<Insets>,
    base_size: Option<(u32, u32)>,
    last_ime_sync: Option<(String, usize, usize, bool)>,
    last_tick: Instant,
    last_vsync_ns: i64,
    vsync_attached: bool,
    frame_clock: FrameClock,
    fps_start: Instant,
    frames_rendered: u64,
    tick_total: Duration,
    paint_total: Duration,
    last_touch: Option<(Instant, f32, f32)>,
    dev: Option<DevRunner>,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            app: None,
            android_app: None,
            keyboard_visible: false,
            window: None,
            gpu: None,
            gpu_size: (0, 0),
            gpu_scale: 1.0,
            last_content: (0, 0, 0, 0),
            base_insets: None,
            base_size: None,
            last_ime_sync: None,
            last_tick: Instant::now(),
            last_vsync_ns: 0,
            vsync_attached: false,
            frame_clock: FrameClock::new(),
            fps_start: Instant::now(),
            frames_rendered: 0,
            tick_total: Duration::ZERO,
            paint_total: Duration::ZERO,
            last_touch: None,
            dev: None,
        }
    }
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

    /// The single place where Android pixels and dp meet. Window size and
    /// content rect are physical; `App::resize` wants physical too and derives
    /// the logical viewport by dividing once.
    fn apply_size(&mut self, physical: (u32, u32)) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let scale = window.scale_factor() as f32;
        let rect = window.content_rect();
        let content = (rect.left, rect.top, rect.right, rect.bottom);
        self.last_content = content;
        let raw = content_insets(
            content,
            Size::new(physical.0 as f32, physical.1 as f32),
            scale,
        );
        // With adjustResize, showing the IME can pan the window: the content
        // rect's top edge moves down, which would read as a larger status-bar
        // inset and push the whole app lower. Keep a baseline measured without
        // the keyboard and let only the bottom inset follow the IME. A rotation
        // swaps the baseline sides, exactly like Viewport::rotate does.
        let landscape = physical.0 > physical.1;
        let rotated = self.base_size.is_some_and(|(w, h)| (w > h) != landscape);
        let mut base = self.base_insets.unwrap_or(raw);
        if rotated {
            base = Insets::new(base.left, base.top, base.right, base.bottom);
        }
        let keyboard = self
            .base_insets
            .is_some_and(|base| raw.bottom > base.bottom + 60.0)
            || (self.base_insets.is_none() && raw.bottom > 100.0);
        let insets = if keyboard {
            Insets::new(base.top, base.right, raw.bottom, base.left)
        } else {
            self.base_insets = Some(raw);
            self.base_size = Some(physical);
            raw
        };
        eprintln!(
            "mre-android: surface {}x{} px @ {scale}x, insets {:?}",
            physical.0, physical.1, insets
        );
        if let Some(gpu) = self.gpu.as_mut() {
            if (scale - self.gpu_scale).abs() > f32::EPSILON {
                gpu.set_scale(scale);
                self.gpu_scale = scale;
            }
            if physical != self.gpu_size {
                gpu.resize(physical.0, physical.1);
                self.gpu_size = physical;
            }
        }
        if let Some(app) = self.app.as_mut() {
            app.resize(
                app_size(Size::new(physical.0 as f32, physical.1 as f32), scale),
                scale,
                insets,
            );
            app.repaint_all();
        }
        self.request_redraw();
    }

    /// Fetches and applies a new Lua bundle from `mre dev`. Called on the
    /// event-loop thread so the reactive graph is never touched off-thread.
    fn apply_bundle(&mut self, reported_revision: u64) {
        let result = {
            let Some(dev) = self.dev.as_mut() else { return };
            dev::fetch_bundle(&dev.config).map(|bundle| {
                dev.revision = bundle.revision;
                if let Ok(mut modules) = dev.modules.write() {
                    *modules = bundle.modules.clone();
                }
                (dev.config.clone(), bundle)
            })
        };

        match result {
            Ok((config, bundle)) => {
                let Some(source) = bundle.entry() else {
                    eprintln!("mre-android: dev bundle has no main.lua");
                    return;
                };
                match self.app.as_mut() {
                    Some(app) => {
                        if let Err(error) = app.reload(source) {
                            eprintln!("mre-android: hot reload failed: {error}");
                            dev::post_event(&config, &format!("error reload: {error}"));
                            return;
                        }
                        dev::post_event(
                            &config,
                            &format!(
                                "applied revision {} (poll {})",
                                bundle.revision, reported_revision
                            ),
                        );
                        eprintln!("mre-android: applied dev revision {}", bundle.revision);
                    }
                    None => return,
                }
            }
            Err(error) => {
                eprintln!("mre-android: dev bundle fetch failed: {error}");
                if let Some(dev) = self.dev.as_ref() {
                    dev::post_event(&dev.config, &format!("error fetch: {error}"));
                }
            }
        }
        self.request_redraw();
    }
}

impl ApplicationHandler<ImeEvent> for Runner {
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
        // Vote for the high refresh rate *before* wgpu creates the Vulkan
        // swapchain: the swapchain samples the window's refresh duration when
        // it is configured, and a 60 Hz sample would lock FIFO to 60 Hz even
        // after the display later switches to 120.
        FrameClock::request_high_rate(&window);
        let size = window.inner_size();
        let scale = window.scale_factor() as f32;
        let physical = (size.width.max(1), size.height.max(1));

        match GpuBackend::for_window(window.clone(), physical.0, physical.1, scale) {
            Ok(gpu) => {
                eprintln!("mre-android: GPU backend active — {}", gpu.info());
                self.gpu = Some(gpu);
                self.gpu_size = physical;
                self.gpu_scale = scale;
            }
            Err(error) => {
                // No software presenter exists on Android: a blank window is
                // worse than a clear diagnostic, so say it out loud.
                eprintln!("mre-android: GPU unavailable ({error}) — nothing can present");
            }
        }
        FrameClock::request_high_rate(&window);
        eprintln!(
            "mre-android: vsync {:?} ({:.1} Hz)",
            self.frame_clock.period(),
            1.0 / self.frame_clock.period().as_secs_f32()
        );
        if !self.vsync_attached {
            if let Some(proxy) = ime::proxy() {
                self.frame_clock.attach_vsync(proxy);
                self.vsync_attached = true;
            }
        }
        self.window = Some(window);
        self.apply_size(physical);
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: ImeEvent) {
        match event {
            ImeEvent::Back => {
                if let Some(app) = self.app.as_mut() {
                    if let Err(error) = app.back() {
                        eprintln!("mre-android: back failed: {error}");
                    }
                }
                self.request_redraw();
                return;
            }
            ImeEvent::Vsync(frame_time_ns) => {
                // Flutter's VsyncWaiter advances the frame using the timestamp
                // Choreographer supplied, not wall-clock time at wakeup.
                let period = self.frame_clock.period().as_secs_f32();
                let mut dt = if self.last_vsync_ns > 0 && frame_time_ns > self.last_vsync_ns {
                    (frame_time_ns - self.last_vsync_ns) as f32 / 1_000_000_000.0
                } else {
                    period
                };
                if dt <= 0.0 || dt > 0.25 {
                    dt = period;
                }
                self.last_vsync_ns = frame_time_ns;
                self.last_tick = Instant::now();
                if let Some(app) = self.app.as_mut() {
                    let started = Instant::now();
                    let _ = app.tick(dt.min(0.25));
                    self.tick_total += started.elapsed();
                }
                self.request_redraw();
                return;
            }
            ImeEvent::Reload { revision } => {
                self.apply_bundle(revision);
                return;
            }
            ImeEvent::Text {
                text,
                start,
                end,
                composing_start,
                composing_end,
            } => {
                let Some(app) = self.app.as_mut() else { return };
                let preedit: String = text
                    .chars()
                    .skip(composing_start)
                    .take(composing_end.saturating_sub(composing_start))
                    .collect();
                if let Err(error) = app.set_text(&text, start, end, &preedit) {
                    eprintln!("mre-android: IME event failed: {error}");
                }
            }
            ImeEvent::Action(action) => {
                let Some(app) = self.app.as_mut() else { return };
                // EditorInfo actions: 2=Go, 3=Search, 4=Send, 5=Next,
                // 6=Done. Next simply leaves focus alone.
                let changed = if action == 5 {
                    app.key("tab", false)
                } else {
                    app.key("enter", false)
                };
                if let Err(error) = changed {
                    eprintln!("mre-android: IME event failed: {error}");
                }
            }
        }
        self.request_redraw();
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(dev) = &self.dev {
            dev.stop.store(true, Ordering::Relaxed);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.apply_size((size.width.max(1), size.height.max(1)));
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let physical = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size())
                    .unwrap_or_default();
                self.apply_size((physical.width.max(1), physical.height.max(1)));
            }
            WindowEvent::RedrawRequested => {
                let mut elapsed = Duration::ZERO;
                if let Some(app) = self.app.as_mut() {
                    let started = Instant::now();
                    if let Err(error) = app.frame() {
                        eprintln!("mre-android: frame failed: {error}");
                    } else if let Some(gpu) = self.gpu.as_mut() {
                        let _ = app.paint_with(gpu);
                    }
                    elapsed = started.elapsed();
                }
                self.frames_rendered += 1;
                self.paint_total += elapsed;
            }
            WindowEvent::Touch(touch) => {
                let scale = self
                    .window
                    .as_ref()
                    .map_or(1.0, |window| window.scale_factor() as f32);
                // MotionEvent coordinates are physical pixels; layout is dp.
                let x = touch.location.x as f32 / scale;
                let y = touch.location.y as f32 / scale;
                let Some(app) = self.app.as_mut() else { return };
                match touch.phase {
                    TouchPhase::Started => {
                        self.last_touch = Some((Instant::now(), x, y));
                        app.touch_start(x, y);
                    }
                    TouchPhase::Moved => {
                        app.touch_move(x, y);
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        let (vx, vy) =
                            self.last_touch
                                .take()
                                .map_or((0.0, 0.0), |(at, from_x, from_y)| {
                                    let dt = at.elapsed().as_secs_f32().max(0.001);
                                    ((x - from_x) / dt, (y - from_y) / dt)
                                });
                        let _ = app.touch_end(x, y, vx, vy);
                    }
                }
                self.request_redraw();
            }
            // While a field is focused the transparent EditText owns the
            // keyboard. It reports the full text through TextWatcher; handling
            // the same key here as well would insert it twice.
            WindowEvent::KeyboardInput { event: key, .. }
                if key.state == ElementState::Pressed && !self.keyboard_visible =>
            {
                let mods = self.modifiers();
                let Some(app) = self.app.as_mut() else { return };
                if matches!(
                    &key.logical_key,
                    Key::Named(NamedKey::Escape) | Key::Named(NamedKey::GoBack)
                ) {
                    let _ = app.back();
                } else if let Some(name) = key_name(&key) {
                    let _ = app.key_with(name, mods);
                } else if let Some(text) = key.text.as_deref() {
                    let _ = app.insert_text(text);
                }
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Mirror the focused editor into the real Android EditText. It owns
        // composition and selection, and shows/hides the keyboard for us. Only
        // sync when something actually changed: rewriting the text on every
        // frame would restart the IME's composition.
        let focused = self
            .app
            .as_ref()
            .and_then(|app| app.focused_input_state())
            .map(|(text, start, end)| (text, start, end, true))
            .unwrap_or_default();
        if self.last_ime_sync.as_ref() != Some(&focused) {
            if let Some(android) = &self.android_app {
                ime::sync(android, &focused.0, focused.1, focused.2, focused.3);
            }
            self.keyboard_visible = focused.3;
            self.last_ime_sync = Some(focused);
        }

        let now = Instant::now();

        // Report the *measured* cadence, not just Choreographer's target. The
        // developer-options overlay shows what the display actually chose.
        let reported = now;
        if reported.duration_since(self.fps_start) >= Duration::from_secs(2) {
            let seconds = reported.duration_since(self.fps_start).as_secs_f32();
            let frames = self.frames_rendered.max(1) as f32;
            eprintln!(
                "mre-android: {:.1} rendered fps | tick {:.2} ms | paint {:.2} ms | vsync {:?}",
                frames / seconds,
                self.tick_total.as_secs_f32() * 1000.0 / frames,
                self.paint_total.as_secs_f32() * 1000.0 / frames,
                self.frame_clock.period(),
            );
            self.fps_start = reported;
            self.frames_rendered = 0;
            self.tick_total = Duration::ZERO;
            self.paint_total = Duration::ZERO;
        }

        // System bars and the IME change the content rect without necessarily
        // sending Resized. Republish insets when that happens.
        if let Some(window) = self.window.as_ref() {
            let rect = window.content_rect();
            let content = (rect.left, rect.top, rect.right, rect.bottom);
            if content != self.last_content {
                let size = window.inner_size();
                self.apply_size((size.width.max(1), size.height.max(1)));
            }
        }

        // Pacing is owned by Choreographer now, exactly like Flutter's
        // VsyncWaiter. We only say whether the next vsync should produce a
        // frame; we no longer wake ourselves on a timer and spin the queue.
        let mut keep_pacing = false;
        let mut needs_redraw = false;
        if let Some(app) = self.app.as_ref() {
            keep_pacing = app.animating() || app.has_focused_input();
            if !keep_pacing {
                needs_redraw = app.needs_frame();
            }
        }
        if keep_pacing {
            self.frame_clock.request_frames();
        } else {
            self.frame_clock.stop_frames();
        }
        if needs_redraw {
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// Keys the editing layer understands. Unknown keys fall through to text,
/// which is how an IME commits a composition.
fn key_name(event: &KeyEvent) -> Option<&'static str> {
    match &event.logical_key {
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

/// Called by `android.app.NativeActivity` on its main-loop thread.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: WinitAndroidApp) {
    let event_loop = match EventLoop::<ImeEvent>::with_user_event()
        .with_android_app(app.clone())
        .build()
    {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("mre-android: event loop failed: {error}");
            return;
        }
    };
    ime::install(event_loop.create_proxy());

    // A dev client fetches its app over the network; a release APK reads the
    // Lua program from assets. Both paths end at the same Runtime loader.
    let config = dev::read_config(&app);
    let initial_bundle = config.as_ref().and_then(|config| {
        eprintln!(
            "mre-android: dev client pairing with {}:{} ({})",
            config.host, config.port, config.project
        );
        match dev::fetch_bundle(config) {
            Ok(bundle) => Some(bundle),
            Err(error) => {
                eprintln!("mre-android: initial dev bundle failed: {error}");
                None
            }
        }
    });
    let modules = Arc::new(RwLock::new(HashMap::new()));
    if let Some(bundle) = &initial_bundle {
        if let Ok(mut map) = modules.write() {
            *map = bundle.modules.clone();
        }
        if let Some(config) = &config {
            dev::post_event(config, &format!("hello {}", config.project));
        }
    }
    let source = if let Some(entry) = initial_bundle.as_ref().and_then(Bundle::entry) {
        entry.to_string()
    } else if let Some(config) = &config {
        dev_error_source(config)
    } else {
        lua_source(&app)
    };

    let assets = app.clone();
    let loader_modules = Arc::clone(&modules);
    let mut state = match App::with_text_store_and_loader(&source, text_store(), move |name| {
        for candidate in [
            name.to_string(),
            format!("{name}.lua"),
            format!("framework/{name}.lua"),
        ] {
            if let Some(source) = loader_modules.read().ok()?.get(&candidate) {
                return Some(source.clone());
            }
        }
        let relative = name.strip_prefix("framework/").unwrap_or(name);
        let path = if name.starts_with("plugins/") {
            format!("{name}.lua")
        } else {
            format!("framework/{relative}.lua")
        };
        let Ok(path) = CString::new(path) else {
            return None;
        };
        let mut asset = assets.asset_manager().open(path.as_c_str())?;
        let mut bytes = Vec::new();
        asset.read_to_end(&mut bytes).ok()?;
        String::from_utf8(bytes).ok()
    }) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("mre-android: Lua program failed: {error}");
            return;
        }
    };
    state.set_platform_call(plugin::bridge(&app));

    let dev_runner = config.map(|config| {
        let stop = Arc::new(AtomicBool::new(false));
        if let Some(bundle) = &initial_bundle {
            dev::post_event(&config, &format!("applied revision {}", bundle.revision));
        }
        DevRunner {
            revision: initial_bundle.as_ref().map_or(0, |bundle| bundle.revision),
            config,
            modules: Arc::clone(&modules),
            stop,
        }
    });
    let mut runner = Runner {
        app: Some(state),
        android_app: Some(app.clone()),
        dev: dev_runner,
        ..Runner::default()
    };

    if let Some(dev) = runner.dev.as_ref() {
        let config = dev.config.clone();
        let proxy = ime::proxy().expect("event loop proxy installed");
        let stop = Arc::clone(&dev.stop);
        let mut cursor = dev.revision;
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match dev::wait(&config, cursor) {
                    Ok(dev::Wait::Changed(revision)) => {
                        cursor = revision;
                        if proxy.send_event(ImeEvent::Reload { revision }).is_err() {
                            break;
                        }
                    }
                    Ok(dev::Wait::Unchanged) => {}
                    Err(error) => {
                        eprintln!("mre-android: dev poll failed: {error}");
                        thread::sleep(Duration::from_secs(2));
                    }
                }
            }
        });
    }

    if let Err(error) = event_loop.run_app(&mut runner) {
        eprintln!("mre-android: event loop exited with {error}");
    }
}
