//! Display frame pacing for Android.
//!
//! Android displays are not 60 Hz any more: 90 and 120 Hz panels are common and
//! the rate can change at runtime. The old host used a hardcoded 16 ms wakeup,
//! which silently pinned every app to 60 Hz. This module asks Choreographer for
//! the real vsync period and tells SurfaceFlinger that a scrolling UI intends
//! to render fast.
//!
//! Everything is loaded dynamically because the NDK entry points arrived in
//! API 30/31 while the APK still supports API 24.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_void};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

#[link(name = "android")]
unsafe extern "C" {
    fn android_get_device_api_level() -> i32;
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: i32 = 2;
const RTLD_LOCAL: i32 = 0;
const DEFAULT_PERIOD_NS: u64 = 16_666_667;

type GetChoreographer = unsafe extern "C" fn() -> *mut c_void;
type RegisterRefresh = unsafe extern "C" fn(*mut c_void, RefreshCallback, *mut c_void);
type UnregisterRefresh = unsafe extern "C" fn(*mut c_void, RefreshCallback, *mut c_void);
type RefreshCallback = extern "C" fn(i64, *mut c_void);
type SetFrameRate = unsafe extern "C" fn(*mut c_void, f32, i8) -> i32;
type SetFrameRateStrategy = unsafe extern "C" fn(*mut c_void, f32, i8, i8) -> i32;

unsafe fn library() -> *mut c_void {
    unsafe {
        dlopen(
            b"libandroid.so\0".as_ptr() as *const c_char,
            RTLD_NOW | RTLD_LOCAL,
        )
    }
}

unsafe fn symbol(handle: *mut c_void, name: &[u8]) -> Option<*mut c_void> {
    if handle.is_null() {
        return None;
    }
    let pointer = unsafe { dlsym(handle, name.as_ptr() as *const c_char) };
    (!pointer.is_null()).then_some(pointer)
}

unsafe fn function<T>(pointer: *mut c_void) -> T {
    unsafe { std::mem::transmute_copy::<*mut c_void, T>(&pointer) }
}

extern "C" fn refresh_rate_changed(period_ns: i64, data: *mut c_void) {
    if period_ns <= 0 {
        return;
    }
    // The pointer is the Arc<AtomicU64> owned by FrameClock. The callback may
    // arrive once during registration and again whenever the system switches
    // display mode; in both cases it only writes an atomic.
    unsafe {
        (&*(data as *const AtomicU64)).store(period_ns as u64, Ordering::Relaxed);
    }
}

/// Owns the Choreographer refresh-rate subscription.
pub struct FrameClock {
    period: Arc<AtomicU64>,
    choreographer: *mut c_void,
    unregister: Option<UnregisterRefresh>,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameClock {
    pub fn new() -> Self {
        let mut clock = Self {
            period: Arc::new(AtomicU64::new(DEFAULT_PERIOD_NS)),
            choreographer: std::ptr::null_mut(),
            unregister: None,
        };
        if unsafe { android_get_device_api_level() } < 30 {
            return clock;
        }
        unsafe {
            let lib = library();
            let (Some(get), Some(register), Some(unregister)) = (
                symbol(lib, b"AChoreographer_getInstance\0"),
                symbol(lib, b"AChoreographer_registerRefreshRateCallback\0"),
                symbol(lib, b"AChoreographer_unregisterRefreshRateCallback\0"),
            ) else {
                return clock;
            };
            let get: GetChoreographer = function(get);
            let register: RegisterRefresh = function(register);
            let unregister: UnregisterRefresh = function(unregister);
            let choreographer = get();
            if choreographer.is_null() {
                return clock;
            }
            let data = Arc::as_ptr(&clock.period) as *mut c_void;
            register(choreographer, refresh_rate_changed, data);
            clock.choreographer = choreographer;
            clock.unregister = Some(unregister);
        }
        clock
    }

    /// Current vsync period. Choreographer guarantees an initial callback on
    /// registration, so this converges within one frame of startup.
    #[must_use]
    pub fn period(&self) -> Duration {
        Duration::from_nanos(self.period.load(Ordering::Relaxed).max(1))
    }

    /// Requests a high refresh rate for this window. 120 Hz is a hint, not a
    /// demand: the system picks the closest supported mode and user power
    /// settings still win. `DEFAULT` compatibility is the game/UI setting that
    /// lets the compositor run at any multiple without judder.
    pub fn request_high_rate(window: &winit::window::Window) {
        if unsafe { android_get_device_api_level() } < 30 {
            return;
        }
        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AndroidNdk(native) = handle.as_raw() else {
            return;
        };
        let surface = native.a_native_window.as_ptr();
        unsafe {
            let lib = library();
            // AT_LEAST (2) is the UI/animation/scrolling compatibility: the
            // display must be at least this fast. DEFAULT (0) is for games and
            // lets the power manager happily park us back at 60 Hz.
            if let Some(strategy) = symbol(lib, b"ANativeWindow_setFrameRateWithChangeStrategy\0") {
                let set: SetFrameRateStrategy = function(strategy);
                let at_least = set(surface, 120.0, 2, 1);
                let fallback = if at_least == 0 {
                    at_least
                } else {
                    set(surface, 120.0, 0, 1)
                };
                eprintln!(
                    "mre-android: frame-rate hint 120Hz at_least={at_least} fallback={fallback}"
                );
            } else if let Some(plain) = symbol(lib, b"ANativeWindow_setFrameRate\0") {
                let set: SetFrameRate = function(plain);
                let at_least = set(surface, 120.0, 2);
                let fallback = if at_least == 0 {
                    at_least
                } else {
                    set(surface, 120.0, 0)
                };
                eprintln!(
                    "mre-android: frame-rate hint 120Hz at_least={at_least} fallback={fallback}"
                );
            }
        }
    }
}

impl Drop for FrameClock {
    fn drop(&mut self) {
        if let Some(unregister) = self.unregister {
            if !self.choreographer.is_null() {
                unsafe {
                    unregister(
                        self.choreographer,
                        refresh_rate_changed,
                        Arc::as_ptr(&self.period) as *mut c_void,
                    );
                }
            }
        }
    }
}
