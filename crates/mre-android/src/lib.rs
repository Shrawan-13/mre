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
//!
//! Density and safe-area conversion live in [`layout`] and are deliberately
//! host-independent so CI can test the exact maths a phone depends on.

pub mod layout;

#[cfg(target_os = "android")]
mod host;
