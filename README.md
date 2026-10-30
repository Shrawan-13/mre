# MRE

A lightweight, retained-mode, multiplatform UI engine for applications — with
**Lua as the application language**, and a performance contract that cannot
silently regress.

> **Status: pre-alpha (M2 in progress).** The perf harness, the scene, the
> reactive Lua graph, layout, shaping, the CPU rasteriser, widgets, the Lua
> application runtime and a desktop window host exist. A dashboard written
> entirely in Lua renders end to end. GPU, gestures and the remaining widget
> library are next — see the milestone plan in
> [docs/PLAN.md](docs/PLAN.md).

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
```

## Heavy testing lives in CI, not on a laptop

Benchmarks, sanitizers, Miri and cross-platform builds run on GitHub Actions so
the development machine stays for editing and fast tests.

| Workflow | Trigger | What it does |
|---|---|---|
| `ci.yml` | every push | fmt, clippy `-D warnings`, docs, gates, Lua smoke, 3-OS test matrix |
| `bench.yml` | PR + nightly | strict contract, interleaved A/B/A/B against egui, artifact upload |
| `nightly.yml` | nightly | Miri, AddressSanitizer, ten-run sustained stress |
| `cross.yml` | nightly + PR | wasm32 build; Android and iOS compile smoke |

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
