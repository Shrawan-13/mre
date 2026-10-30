# MRE

A lightweight, retained-mode, multiplatform UI engine for applications — with
**Lua as the application language**, and a performance contract that cannot
silently regress.

> **Status: pre-alpha (M2).** The perf harness, the scene, the reactive Lua
> graph, layout, shaping, the CPU rasteriser, widgets, the Lua application
> runtime and a desktop window host exist. A 50,000-row dashboard, an editable
> form, a control gallery and a spring-bound animation all render end to end,
> entirely from Lua. GPU, gestures, the remaining widget library and the
> platform accessibility bridges are next — see [docs/PLAN.md](docs/PLAN.md)
> and [docs/FEATURE-PARITY.md](docs/FEATURE-PARITY.md).

## What this is

A rendering engine and widget substrate for desktop, mobile and web apps, built
around four claims that will each be measured before they are stated:

1. **Fastest incremental frame** of any lightweight toolkit — proven against
   egui/iced/Slint on one identical scene, not asserted.
2. **Zero idle cost** — an app doing nothing runs no user code, allocates
   nothing, and requests no frame.
3. **Apps written in Lua**, with no performance cliff for choosing to.
4. **Feature parity with egui**, then a superset: layout, animation, i18n and
   accessibility are engine concerns, not bolt-ons.

"Super light" is a constraint with numbers attached, not an adjective.

| Property | Target | Gate |
|---|---|---|
| Steady-state allocations per frame | `0` | `crates/mre-bench/tests/gate_alloc.rs` |
| Scene node size | 64 B (one cache line) | `crates/mre-bench/tests/gate_structure.rs` |
| 50k-row table, virtualised | < 64 KiB of scene memory | `crates/mre-bench/tests/gate_structure.rs` |
| Draw calls, reference scene | <= 64 | `crates/mre-bench/tests/gate_structure.rs` |
| Frame time, 120 Hz | p99 <= 8.33 ms | `perf/budget.toml` + bench |
| Idle cost | zero computations, zero frames | `mre-lua` tests + bench |
| Compile time, warm | < 5 s | tracked, not yet gated |
| Core binary | <= 2 MB | `release.yml` (planned) |

## Write an app in Lua

```lua
-- examples/dashboard.lua, abridged
local ticks = mre.signal(0)
local progress = mre.signal(0)
local animate = mre.animate(progress, { stiffness = 220, damping = 24 })

mre.app {
  view = function()
    return mre.column {
      padding = 20,
      mre.text { "Refreshed " .. ticks() .. " times", size = 20 },
      mre.column {
        width = 160, height = 3, background = "#E5E7EB",
        mre.column { width = progress() * 160, height = 3, background = "#2563EB" },
      },
      mre.button {
        "Refresh",
        on_click = function()
          ticks(ticks() + 1)
          animate(math.min(1.0, (ticks()) / 10))
        end,
      },
    }
  end,
}
```

Run it on the desktop:

```bash
cargo run -p mre-window --bin mre -- examples/dashboard.lua
```

Render the same app headlessly, with no display and no driver:

```bash
cargo run -p mre-runtime --example render -- examples/dashboard.lua /tmp/dashboard.png 900 760
```

The API is specified in [docs/LUA-API.md](docs/LUA-API.md). The implementation
covers signals, effects, memos, `untracked`, batching, cycle detection, the
declarative vnode surface, springs and click routing. Components, fine-grained
property bindings, gestures and hot reload are M4.

## Layout

```
crates/
  mre-core/    ids, arena, geometry, colour, viewport/insets  (no deps, no unsafe)
  mre-perf/    budget contract, allocation counter, statistics
  mre-scene/   retained tree, dirty flags, display lists, reference table
  mre-layout/  measure/arrange engine: flex, grid, stack, absolute, caching
  mre-text/    HarfBuzz shaping, fallback, line breaking, shaped cache
  mre-raster/  deterministic CPU reference rasteriser + glyph cache
  mre-widgets/ declarative views, themes, layout-to-paint bridge
  mre-anim/    frame-rate independent springs and easings
  mre-lua/     reactive graph + Lua host
  mre-runtime/ Lua apps: view conversion, event routing, painting
  mre-window/  desktop host: winit + software presenter (`mre` binary)
  mre-bench/   benchmark runner + the performance gates
examples/
  counter.lua  a reactive app, in Lua
  dashboard.lua a data-dense table, in Lua
  table.lua    50,000 virtualised rows with a scrollbar
  gallery.lua  composed control gallery
  form.lua     editable text fields with validation and submit
  bindings.lua animation bound to paint properties, no rebuilds
perf/
  budget.toml      THE performance contract, compiled in at build time
  scenes/table.toml the reference scene every engine must render
  results/         machine-readable run output (gitignored)
baselines/         per-engine adapters for comparison (not workspace members)
docs/
  PLAN.md            the full roadmap, milestones and exit criteria
  LUA-API.md         the scripting contract
  INPUT.md           pointer, gestures, keyboard, IME, accessibility
  FEATURE-PARITY.md  the egui parity matrix
  ARCHITECTURE.md    layering and dependency rules
  PERF-CONTRACT.md   how budgets are enforced
  COMPARISON.md      the fairness protocol and current results
```

## Quick start

```bash
cargo test --workspace                        # every perf gate + Lua tests
cargo run --release -p mre-bench              # benchmark the reference scene
cargo run --release -p mre-bench -- --count-allocs
cargo run --release -p mre-bench -- --strict  # fail if the contract is violated
cargo run --release -p mre-bench -- compare   # table vs. baseline engines
cargo run -p mre-window --bin mre -- examples/dashboard.lua
cargo run -p mre-runtime --example render -- examples/dashboard.lua /tmp/dashboard.png
cargo run -p mre-runtime --example render -- examples/form.lua /tmp/form.png 420 360
cargo run -p mre-runtime --example semantics -- examples/form.lua
```

`MRE_SCROLL=<px>` renders a scrolled frame, and `MRE_TICKS=<n>` advances
animations before capturing, which is how visual regressions are checked
without a display.

## Heavy testing lives in CI, not on a laptop

Benchmarks, sanitizers, Miri and cross-platform builds run on GitHub Actions so
the development machine stays for editing and fast tests.

| Workflow | Trigger | What it does |
|---|---|---|
| `ci.yml` | every push | fmt, clippy `-D warnings`, docs, gates, Lua smoke, 3-OS test matrix |
| `bench.yml` | PR + nightly | strict contract, interleaved A/B/A/B against egui, artifact upload |
| `nightly.yml` | nightly | Miri, AddressSanitizer, ten-run sustained stress |
| `cross.yml` | nightly + PR | wasm32 build; Android and iOS compile smoke |

## What works today

- **Layout** — flex row/column, grid, stack, absolute insets, percentages,
  min/max, quantised constraint caching, hit testing.
- **Text** — HarfBuzz shaping, font fallback, line breaking, bidi detection,
  shaped-text caching, glyph atlas; RTL right-aligns, single line.
- **Paint** — deterministic CPU rasteriser with rounded rects, strokes, paths,
  exact coverage-mask clipping (text and rounded shapes included), display
  density, and a physical-pixel canvas.
- **Widgets** — text, buttons, text fields, checkboxes, sliders, toggles,
  progress, dividers, icons, virtualised lists and scroll containers with
  thumbs.
- **Input** — pointer move/down/up, hover and press feedback that never
  rebuilds, press capture and drag with cancel, wheel scroll with clamping,
  click-to-focus, Tab order, Escape blur, single-line editing with
  character-counted carets, password mode, `on_change`/`on_submit`, keyboard
  button activation, IME commit.
- **Animation** — spring-first, fixed-step, velocity-preserving retargeting;
  springs can write signals *or* drive paint properties directly, in which case
  an animating frame runs no Lua and rebuilds no view.
- **Accessibility** — a semantic tree with roles, labels, values, bounds and
  focus, derived from the same tree that paints.
- **Responsiveness** — resize, rotation, DPI and safe-area changes all arrive as
  a new `Viewport`; rotation swaps insets; layout and paint rerun, the view is
  not rebuilt.

## Why the harness came first

A performance goal written in a README is a wish. A performance goal that fails
the build is a property.

So before the first triangle: a counting allocator that turns "zero allocations
per frame" into an assertion, a budget file compiled into constants, percentile
reporting that makes a 40 ms hitch impossible to hide behind a 4 ms mean, and a
reference workload real enough that optimising for it optimises for real apps.

## Design commitments

1. **No `unsafe` in the engine crates.** `unsafe` is confined to allocator
   instrumentation, where it is unavoidable and audited.
2. **No allocation in a steady-state frame.** Not "few". Zero.
3. **One arena, no `Rc<RefCell>` trees.** Nodes are 64 bytes and cache-linear.
4. **Retained and reactive.** Work is O(changed), not O(UI).
5. **Dirty flags, not full rebuilds.** An unchanged subtree costs one pointer.
6. **Compile speed is a feature.** Small crates, dependency-light core, no proc
   macros in the engine path.
7. **Honest benchmarks.** Interleaved runs, published losses, scope caveats next
   to every number. See [docs/COMPARISON.md](docs/COMPARISON.md).

## Licence

MIT OR Apache-2.0.
