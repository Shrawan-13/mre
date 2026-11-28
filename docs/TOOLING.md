# MRE tooling: CLI, projects, dev server, plugins

MRE is a framework, not just a rendering engine. The `mre` CLI is written in Go
so it can be distributed as a single static binary on macOS, Linux and Windows.
The engine stays Rust; the CLI only scaffolds projects, watches files, serves
Lua bundles and drives the platform build tools.

## Install

```bash
go install github.com/Shrawan-13/mre/cli@latest
```

Prebuilt binaries are attached to each `cli/v*` release tag. The Go module lives
in `cli/`, so releases must be tagged `cli/v0.1.0` (not `v0.1.0`) for Go modules
to resolve them.

## Commands

| Command | What it does |
|---|---|
| `mre new <name>` | Creates a complete project from the embedded starter. |
| `mre dev` | Starts the file watcher + LAN dev server, prints a pairing QR. |
| `mre run` | Runs the app in a desktop window through `mre-window`. |
| `mre build android` | Cross-compiles `libmain.so`, copies Lua assets, assembles the **dev client** APK. |
| `mre build android --prod` | Assembles a release APK with Lua baked into assets. |
| `mre test` | Compile-checks every Lua file and runs the engine test suite. |
| `mre devices` | Lists connected Android devices via `adb`. |
| `mre log` | Streams events from a connected dev client. |
| `mre doctor` | Checks cargo, gradle, adb, java, the NDK and the manifest. |
| `mre plugin new <name>` | Scaffolds a native plugin on Android and its Lua wrapper. |
| `mre completion <shell>` | Generates shell completions. |

`mre build` finds the engine checkout by walking up from the project directory
(or by reading `MRE_ENGINE`). A generated project therefore contains only app
code, Lua framework overrides, assets and a Gradle template — no copy of the
engine source.

## Project layout

```
mre.toml          manifest: name, engine version, entry, Android id, plugins
mre.lock          (future) pinned module versions; today mre.toml pins the engine
src/main.lua      app entry point
framework/        Lua-only theme / widget / style overrides
assets/           static files
plugins/          native plugin sources and Lua wrappers
tests/            Lua smoke tests
android/          generated Gradle template, including two product flavours
```

`mre.toml` is deliberately small:

```toml
name = "demo"
engine = "0.1.0"
entry = "src/main.lua"

[android]
applicationId = "dev.mre.demo"
label = "demo"
plugins = "battery, haptics"

[dev]
port = 8080
assets = "assets"
```

The Android template has two flavours:

- **`dev`** (`dev.mre.<app>.dev`) — the QR-pairing dev client. It contains no
  app Lua. It launches `DevPairActivity`, scans the QR, writes a pairing file to
  app-private storage and starts the native renderer.
- **`prod`** — the release app. `src/main.lua` and `framework/*.lua` are copied
  into `assets/` and the launcher is `MreActivity` directly.

The repo's own `android/` directory is kept byte-identical to the embedded
template by `cli/internal/templates/app_test.go`, so CI's APK and a user's
generated APK are built from the same Gradle sources.

## Dev server

`mre dev`:

1. Reads `mre.toml` and builds an immutable bundle of `src/main.lua`,
   `framework/*.lua` and `plugins/*/init.lua`.
2. Generates a random 32-hex-character token, writes it to `.mre/dev.token`,
   and prints both a terminal QR and `mre-dev-qr.png`.
3. Watches the project with `fsnotify`. Relevant events are debounced for
   120 ms; each successful rebuild increments a revision and publishes a new
   bundle.
4. Serves HTTP on `0.0.0.0:8080` by default.

The QR payload is:

```
mre://dev?host=192.168.1.12&port=8080&token=<32 hex chars>&project=demo
```

If the phone is acting as a Wi‑Fi hotspot (or is otherwise on mobile data), its
apps may have no route to the PC's LAN address even though `adb` can ping it.
Use USB instead:

```bash
mre dev --usb
```

This runs `adb reverse tcp:8080 tcp:8080` and puts `127.0.0.1` in the QR, so the
connection goes through the USB daemon. It works regardless of the phone's
network mode. A dev client that cannot reach the server shows a visible error
screen instead of silently falling back to the bundled demo.

Only the pairing QR carries the token. Every API request must include
`X-Mre-Token: <token>`; the server compares it in constant time. Token, session
id and content hashes make the protocol deterministic and safe on a shared LAN.

### Endpoints

| Endpoint | Purpose |
|---|---|
| `GET /api/manifest` | Protocol, revision, bundle hash, module count, session. |
| `GET /api/wait?since=N` | Long-poll (25 s). `200` with a new revision, `304` if unchanged. |
| `GET /api/bundle` | Length-prefixed immutable bundle. |
| `POST /api/events` | Client telemetry (`hello`, `applied`, `error`). |
| `GET /api/logs?since=N` | Long-poll log stream for `mre log`. |
| `GET /` | Small status page. |

The bundle format is intentionally simple:

```
MRE/1 <revision> <count>\n
<name-byte-length> <name> <body-byte-length>\n
<body bytes>\n
...
```

The Rust client parses this with `std::net` and a `BufReader`; no HTTP crate or
JSON parser is linked into the engine. Content hashes cover module names and
bodies in sorted order, never mtimes.

### Hot reload semantics

The dev client long-polls on a background thread and posts an
`ImeEvent::Reload` to the winit event loop. The engine fetches the new bundle
on the UI thread, replaces the module map, then calls `App::reload(entry)`.

`Runtime::reload` rebuilds the Lua VM with the same loader, restores the
viewport, clock, theme and insets published by the host, and re-executes the
entry exactly once. The previous reactive graph is discarded, so effects cannot
accumulate across reloads. This is the same guarantee Flutter calls *hot
reload* (state preserved) versus *hot restart* (everything fresh), but made
explicit:

- `mre.persist(key, initial)` creates a signal whose value is captured and
  restored across reloads. It is the only state that survives.
- Plain `mre.signal(initial)` starts from `initial` again on every reload.
- Theme, styles and widgets live in Lua, so changing them never needs a Rust
  rebuild or a new APK.

This is deliberately narrower than React Native's Fast Refresh: it never tries
to guess which local upvalues are "component state". A reload is deterministic
because identity is explicit. Apps that need more survive with `mre.persist`.

Compile errors never replace a running app: the CLI rejects a bundle without a
valid entry and the client keeps the last good bundle on failure. `mre test`
catches the same errors before a reload is attempted.

## Native plugins

Lua owns composition. When an app genuinely needs a platform API, add a plugin:

```bash
mre plugin new battery
```

This creates:

```
plugins/battery/init.lua
plugins/battery/android/dev/mre/plugin/BatteryPlugin.java
android/app/src/main/java/dev/mre/plugin/BatteryPlugin.java
```

and regenerates `Plugins.java` and `mre.toml`. Implement the Java method and
call it from Lua:

```lua
mre.plugins.battery.level = function()
  return tonumber(mre.platform_call("battery", "level", "")) or 0
end
```

The bridge is one static JNI method,
`dev.mre.MrePluginRegistry.call(name, method, args) -> String`. Every call is
logged; failures return `""` instead of throwing into a frame. The interface is
string-in/string-out on purpose: it is easy to audit, easy to log and keeps the
engine free of a JSON dependency. Structured values can be layered on later
without changing the ABI.

Desktop builds see `mre.platform_call` as a no-op returning `""`, so golden
tests stay deterministic. iOS can implement the same bridge in Swift later; the
Lua API does not change.

## Distribution

- `cargo install` is not used: the CLI is Go, installed with `go install` or
  downloaded from GitHub Releases.
- Rust crates are published from the monorepo as `mre-core`, `mre-runtime`,
  `mre-lua`, `mre-android`, etc. The engine and CLI are versioned together in
  `cli/internal/version`.
- CI runs `go vet`, `go test` and `go build` in the `cli` job, and cross-builds
  the APK in `cross.yml`. The Android template is shared between both.

## What is intentionally not here

- No visual designer: apps are Lua-first by design.
- No OTA code push into release apps: dev reload is for development only.
- No emulator in CI: device testing stays manual via `adb`.
- No implicit state capture: determinism beats convenience.
