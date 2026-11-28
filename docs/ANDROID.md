# Android

MRE ships as an Android **NativeActivity**. You do not need to write Java or
Kotlin: Android loads a shared library named `main`, and the engine runs the
whole event loop from there.

## Build

The CI job `cross.yml` → `aarch64-linux-android` builds the full engine and
`libmain.so` with NDK r27c:

```bash
export NDK=/path/to/android-ndk
TOOLCHAIN="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android24-clang++"
export CC_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android24-clang++"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
export RUSTFLAGS="-C link-arg=-llog"
cargo build --release --target aarch64-linux-android -p mre-android
```

Copy `target/aarch64-linux-android/release/libmain.so` to
`app/src/main/jniLibs/arm64-v8a/libmain.so`.

## APK skeleton

`AndroidManifest.xml`:

```xml
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
  <uses-feature android:glEsVersion="0x00030002" android:required="false" />
  <application android:label="MRE" android:hasCode="false">
    <activity android:name="android.app.NativeActivity"
              android:configChanges="orientation|screenSize|keyboard|keyboardHidden|screenLayout|uiMode|density"
              android:exported="true">
      <meta-data android:name="android.app.lib_name" android:value="main" />
      <intent-filter>
        <action android:name="android.intent.action.MAIN" />
        <category android:name="android.intent.category.LAUNCHER" />
      </intent-filter>
    </activity>
  </application>
</manifest>
```

`android:hasCode="false"` is correct: there is no bytecode. Add
`android.permission.INTERNET` only if your Lua program needs it.

## Your app

Put the Lua program at `app/src/main/assets/main.lua`. It is read through the
Android asset manager at startup. If it is missing, the bundled dashboard demo
runs, so the first build is never a blank screen.

Roboto is embedded in the library, so text works without installing fonts.

Colours, spacing, navigation and behaviour are all Lua data: call
`mre.set_theme("dark")` (or `"light"` / `"system"`) to switch the native Rust
palette at runtime, and use `mre.tokens` / `mre.sheet` to restyle nodes. The
prelude ships a fallback style layer, so the same program also runs in unit tests
where `framework/init.lua` is not loaded.

## Layout, density and system bars

Android's rule is: **design in dp, draw in px**. One dp is one physical pixel
at 160 dpi, and `px = dp * (dpi / 160)` on any other display
([Android: support different pixel densities](https://developer.android.com/training/multiscreen/screendensities)).
MRE applies that rule once, in `Viewport::from_physical`:

```
window (px) ──App::resize(physical, scale)──> Viewport (dp) ──> layout ──> raster × scale
```

The Android host must pass the **physical** window size. Dividing before the
call divides twice, so on a 3x phone a 1080 px screen becomes a 120 dp layout
— everything piles up in the top-left corner. That was a real bug; it is fixed
in `crates/mre-android/src/layout.rs`, whose unit tests assert that a
1080x2400 px / 3x window produces a 360x800 dp viewport. The gallery test
`layout_is_density_independent` checks the same geometry at 1x, 2x and 3x.

System bars and the keyboard are handled the same way: winit's
`WindowExtAndroid::content_rect()` gives the unobstructed area in pixels;
`layout::content_insets` converts it to logical insets passed to
`App::resize`, and the host re-publishes them whenever the rect changes (the
IME can move it without a resize event). See
[Android: window insets](https://developer.android.com/develop/ui/views/layout/insets).

For good layouts in your own Lua apps:

- write sizes in dp (MRE's logical units), never in device pixels;
- prefer `mre.row`/`mre.column` with `gap`, `padding`, `grow` and `align`
  over absolute positions;
- put long content in `mre.scroll { content, offset, on_scroll }` rather than
  assuming a fixed screen height;
- check your app at a few logical sizes (360x640, 411x731, 600x960) — the
  engine's unit tests do this without a device.

## How it runs

- The wgpu renderer is used when the device exposes a compatible Vulkan
  adapter. Startup logs the adapter name; if it says `SOFTWARE ADAPTER`, the
  device is falling back to software Vulkan and will be slower.
- Touch events from winit go straight into the engine's touch/scroll/fling
  path. Hardware keys and IME commits are forwarded to the same editing code
  as on the desktop.
- The paint contract, damage tracking, layout, Lua runtime and performance
  gates are shared with every other host — Android is a host, not a fork.

## Ready-made gallery APK

`cross.yml` also assembles an APK for you:

- downloads/uses `libmain.so` from the same job,
- copies `examples/android_gallery.lua` into `assets/main.lua`,
- runs Gradle from the template in `android/`,
- uploads `app-debug.apk` as an artifact.

The shipped `examples/android_gallery.lua` is a mobile-first showcase with six
destinations in a bottom menu:

- **Controls** — counter, reset, toggle, checkbox, slider, progress and an IME
  text field that echoes what you type.
- **Layout** — flex directions, `display = "none"`, a responsive grid, viewport
  readout and a genuine window-level overlay.
- **Motion** — a velocity-preserving spring, a tween driven by the engine
  clock, timers and a determinism readout.
- **Widgets** — tabs, `mre.select`, badges, dialogs and an error boundary.
- **Checks** — a **Run checks** button that executes five assertions in Lua and
  shows `5/5 passed`.
- **About** — `mre.theme()`, safe-area insets, viewport, diagnostics and a
  runtime theme switcher.

The default palette is dark; the **Theme** button in the top bar switches
between dark and light without a rebuild. Every colour is a named theme colour,
so one call recolours the whole app.

Those exact clicks are automated in
`crates/mre-runtime/tests/android_gallery.rs`, which drives the app through
the same hit test a finger uses — no emulator needed to prove it is interactive.

To build it yourself:

```bash
cd android
gradle assembleDebug          # or open android/ in Android Studio
```

Then copy the `.so` and Lua file as CI does, or just use the APK artifact from
the latest `cross` run. To ship your own app, replace `assets/main.lua`; the
engine needs no Java or Kotlin changes.

## Keyboard and text editing

`NativeActivity` has no `GameTextInput`, and winit drops Android `TextEvent`s,
so a full IME cannot be built from the event loop alone. The APK template
therefore hosts a transparent, focused `EditText` in a small `NativeActivity`
subclass (`dev.mre.MreActivity`). Android treats it as a normal text view, so
the soft keyboard gets real composition, suggestions, selection handles,
select all, and copy/paste.

Per the Android `InputConnection` contract, the editor receives edits from the
IME through `commitText`, `setComposingText`, `deleteSurroundingText`,
`deleteSurroundingTextInCodePoints`, `replaceText` and raw
`sendKeyEvent(KEYCODE_DEL)` calls — not through a single callback. The template
subclasses `EditText` and wraps its `InputConnection` in an
`InputConnectionWrapper`; every mutating operation runs the real editor edit
first, then reports the complete resulting state (text, selection, composing
span) through JNI to `App::set_text`. This is what makes backspace work,
including on an empty field, and avoids double-reporting from `TextWatcher`.
`TextWatcher` remains only as a diagnostic log.

The host mirrors the engine's text, selection and focus back into the view, but
only when they actually differ and without replacing text during composition,
which keeps the IME's composing spans intact. UTF-16 offsets from Android are
converted to and from Unicode scalar indices for Rust. IME actions
(Done/Search/Go) submit the field.

While a field is focused, the `EditText`/`InputConnection` is the single source
of truth; hardware keys that would duplicate an IME insertion are ignored.
When no field is focused, navigation keys go through the normal winit path.

Safe area **is** applied: the host converts `content_rect()` to logical insets,
`Viewport::content()` keeps normal layout inside them, and Lua can read them via
`mre.safe_area()`. It is applied exactly once, by the layout root; the gallery
does **not** add it again as root padding. The About page prints the live values.
`mre.safe_area()` is reactive, so keyboard and rotation changes rebuild only the
nodes that read it.

Android 15 (SDK 35) forces apps to draw edge-to-edge. `NativeActivity` has no
`WindowInsetsCompat` bridge, so the template's theme opts out with
`android:windowOptOutEdgeToEdgeEnforcement=true`: the system draws opaque bars
and the header can never sit underneath the status bar. Insets are still read
from `onContentRectChanged` for the keyboard and for any remaining overlap. A
dialog scrim is an `overlay = true` node anchored to `Viewport::full()`, so it
dims the entire surface including those bands without any negative-offset
arithmetic in Lua.

`mre.select` keeps its own signal state when an app supplies no handlers, so it
opens and changes value out of the box.

## Dev client and product flavours

The Gradle template builds two variants:

| Variant | Application id | Launcher | Lua source |
|---|---|---|---|
| `dev` | `<applicationId>.dev` | `dev.mre.dev.DevPairActivity` | fetched from `mre dev` over LAN |
| `prod` | `<applicationId>` | `dev.mre.MreActivity` | `assets/main.lua` in the APK |

The dev variant is a real QR-pairing client. `DevPairActivity` uses ZXing's
`ScanContract`, accepts `mre://dev?host=...&port=...&token=...` deep links,
writes the pairing to `getFilesDir()/dev-config.txt`, and launches the native
renderer. The Rust host reads that file through
`AndroidApp::internal_data_path()`, fetches the bundle with a token header,
and long-polls `/api/wait` on a background thread. New revisions arrive as a
winit user event and are applied by `App::reload` on the event-loop thread.

Cleartext HTTP is enabled only in the `dev` source set via a network security
config. Release apps have no scanner dependency and no cleartext exception.

See [docs/TOOLING.md](TOOLING.md) for the CLI, protocol and plugin bridge.

## Limits

There is no software presenter on Android: the device needs a usable GPU
driver. CI builds and uploads the dev-client APK, but it does not run it on an
emulator or device — that still needs a manual install from the artifact and a
QR scan from `mre dev`.
