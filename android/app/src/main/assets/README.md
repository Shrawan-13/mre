Place your Lua application here as `main.lua`.

The CI workflow copies `examples/android_gallery.lua` to this path before
building the APK. The gallery has three clickable pages (Controls, Checks,
About) and a "Run checks" button that runs five assertions inside the app.

If this file is missing, the engine falls back to a bundled dashboard demo, so
the first APK you build is never a blank screen.
