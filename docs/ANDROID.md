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

There is no software presenter on Android: the device needs a usable GPU
driver. That is a deliberate trade-off; adding an ANativeWindow software
presenter is possible later without touching the engine.
