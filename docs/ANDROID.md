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

The gallery has three clickable pages — **Controls**, **Checks** and **About**:
taps, toggles, a checkbox, a slider, a progress bar, a text field, and a
**Run checks** button that executes five assertions in Lua and shows
`5/5 passed`. Those exact clicks are automated in
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

## Limits

There is no software presenter on Android: the device needs a usable GPU
driver. That is a deliberate trade-off; adding an ANativeWindow software
presenter is possible later without touching the engine. CI builds and uploads
the APK, but it does not run it on an emulator or device — that still needs a
manual install from the artifact.
