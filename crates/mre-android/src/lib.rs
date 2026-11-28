//! Android host for MRE.
//!
//! The APK template pairs this library as `libmain.so` with a small
//! `NativeActivity` subclass that hosts a transparent `EditText` for full IME
//! support. Put your Lua program in `app/src/main/assets/main.lua`. If it is
//! missing, the bundled dashboard demo runs instead, so a first build shows
//! something immediately.
//!
//! Rendering goes through the same [`mre_paint::Backend`] as the desktop host.
//! Modern Android devices expose Vulkan to wgpu; on devices without it, the
//! app logs the failure instead of silently showing a blank window.
//!
//! Density and safe-area conversion live in [`layout`] and are deliberately
//! host-independent so CI can test the exact maths a phone depends on.

pub mod layout;

#[cfg(target_os = "android")]
mod host;

#[cfg(target_os = "android")]
mod ime;

#[cfg(target_os = "android")]
pub mod dev;

#[cfg(target_os = "android")]
mod plugin;
