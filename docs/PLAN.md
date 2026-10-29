# MRE — Master Plan

> Version 1.0 · 2026-10-30 · Status: **M0 complete, M1 in progress**

## 1. Positioning

MRE is a **retained, reactive, Lua-first UI engine** for application software:
dashboards, editors, tools, settings panels, data browsers, kiosks, and
instrumentation. Not games, not 3D, not a website builder.

The claim we intend to earn, in this order:

1. **Fastest incremental frame** of any lightweight UI toolkit — measured, not
   asserted, against egui/iced/Slint on an identical scene.
2. **Zero idle cost** — an app doing nothing consumes nothing.
3. **Apps written in Lua** with no Rust in the loop, and no performance cliff for
   doing so.
4. **Feature parity with egui**, then a superset: layout, animation, i18n and
   accessibility are engine concerns, not bolt-ons.

"Super light" is a hard constraint, not an adjective: ≤ 2 MB core binary,
≤ 96 B/node, zero allocations per steady-state frame.

## 2. Non-goals

Explicitly out of scope, so the design does not drift:

- 3D scene graphs, PBR, game loops.
- Being a browser (no HTML/CSS engine, no DOM).
- A visual designer (a scripting-first engine with a future inspector is enough).
- Shipping APKs/iOS builds from this laptop. CI compiles them; devices are out of
  scope here.

## 3. The five architectural bets

| # | Bet | Why it wins | What it costs |
|---|---|---|---|
| 1 | **Retained + reactive**, not immediate-mode | Work is O(changed), not O(UI). egui re-runs the whole tree every frame | Must solve invalidation correctly; state lives in a graph |
| 2 | **Scoped re-render + fine-grained bindings** | Structural changes re-render a component; hot properties (scroll, animation, hover) bind directly to scene nodes and never re-render | Two binding paths to keep coherent |
| 3 | **Zero-allocation steady state** | Predictable frame times, no GC-shaped stutter in a GC-free language | Arenas, pools, caches everywhere; CI gate enforces |
| 4 | **Lua as a first-class host, not a binding** | Apps are data + behaviour; Rust is the platform. Hot reload, no compile step | Must design a scripting API that is fast and safe |
| 5 | **Deterministic frames** | Golden-image and animation tests become possible; regressions are caught, not argued about | Fixed-timestep animation and injectable clocks |

## 4. System architecture

```
                    ┌──────────────────────────────────────────┐
   app.lua ────────>│ mre-lua      reactive graph, vnodes,     │
                    │              hot reload, sandbox         │
                    └───────────────┬──────────────────────────┘
                                    │ patches
                    ┌───────────────▼──────────────────────────┐
                    │ mre-scene    retained tree, dirty flags, │
                    │              display lists                │
                    └───┬───────────────┬──────────────┬───────┘
                        │               │              │
              ┌─────────▼─────┐ ┌───────▼──────┐ ┌─────▼──────┐
              │ mre-layout    │ │ mre-anim     │ │ mre-text   │
              │ flex/grid,    │ │ springs,     │ │ shaping,   │
              │ measure cache │ │ timelines    │ │ atlas      │
              └─────────┬─────┘ └───────┬──────┘ └─────┬──────┘
                        └───────────────┼──────────────┘
                    ┌───────────────────▼──────────────────────┐
                    │ mre-paint    batching, clips, layers,     │
                    │              effects, damage              │
                    └───────────────────┬──────────────────────┘
                    ┌───────────────────▼──────────────────────┐
                    │ mre-gpu   wgpu (Vulkan/Metal/DX12/WebGPU) │
                    │           + software raster fallback      │
                    └───────────────────┬──────────────────────┘
                    ┌───────────────────▼──────────────────────┐
                    │ mre-window + mre-input + mre-a11y         │
                    │ winit, gestures, IME, AccessKit           │
                    └──────────────────────────────────────────┘
```

Dependency rule: **arrows only point down.** `mre-scene` never sees a GPU;
`mre-lua` never sees a window. This keeps the interesting parts unit-testable in
under a second and is what makes a headless golden-image pipeline possible.

### Crate map

| Crate | Milestone | Responsibility |
|---|---|---|
| `mre-core` | M0 ✅ | ids, arena, geometry, colour |
| `mre-perf` | M0 ✅ | budget contract, allocation counting, percentiles |
| `mre-scene` | M0 ✅ | retained tree, dirty flags, display lists |
| `mre-layout` | M1 | flex + grid + absolute, measure cache, incremental |
| `mre-text` | M1 | shaping, font cache, glyph atlas, bidi, line breaking |
| `mre-raster` | M1 | software rasteriser: the reference implementation |
| `mre-testkit` | M1 | golden images, scene fixtures, deterministic clock |
| `mre-gpu` | M2 | wgpu backend implementing the paint interface |
| `mre-window` | M2 | event loop, multi-window, DPI, safe areas |
| `mre-paint` | M2 | batching, clip stack, layers, blur/shadow |
| `mre-anim` | M3 | springs, tweens, timelines, layout animation |
| `mre-input` | M3 | pointer/touch/pen, gestures, keyboard, IME, drag&drop |
| `mre-a11y` | M3 | AccessKit tree, focus order, announcements |
| `mre-lua` | M4 | reactive graph, vnodes, bindings, hot reload |
| `mre-widgets` | M5 | the standard library |
| `mre-capi` | M6 | C ABI for Swift / Kotlin / C# / Python |
| `mre-cli` | M4 | `mre run`, `mre watch`, `mre build`, `mre doctor` |

## 5. Lua: the application language

Full specification in [`LUA-API.md`](LUA-API.md). The shape:

```lua
local mre = require("mre")

local Counter = mre.component(function()
  local count = mre.signal(0)
  local pop = mre.spring(1.0, { stiffness = 240, damping = 22 })

  mre.effect(function() pop.target = 1 + count() * 0.04 end)

  return mre.column {
    gap = 16, padding = 24,
    mre.text { "Clicked " .. count() .. " times", size = 20 },
    mre.button {
      label = "Increment",
      on_click = function() count(count() + 1) end,
      transform = { scale = pop() },
    },
  }
end)

mre.mount(Counter)
```

Three binding tiers, chosen by what changes:

| Tier | Mechanism | Cost when changed | Used for |
|---|---|---|---|
| **Structural** | component re-render + vnode diff | O(changed subtree) | lists, conditionals, text content |
| **Property** | `mre.bind(fn)` attaches a computation to one scene property | O(1), no re-render | colour, opacity, transforms |
| **Animatable** | engine-driven, writes the property each frame | O(1) + integration | anything an animation touches |

Scroll, hover and animation **must** use tier 2/3. If a scroll triggers a
component re-render, the zero-allocation gate fails, which is the intended
feedback mechanism.

## 6. Input and interaction model

Full model in [`INPUT.md`](INPUT.md).

- **One pointer abstraction** over mouse, touch and pen, carrying pressure and
  tilt. No `mouse_*` vs `touch_*` split in the API.
- **Gesture recognition in the engine**: tap, double-tap, long-press, pan, fling,
  pinch, rotate, swipe — with tunable thresholds and edge/palm rejection.
- **Text input that is actually correct**: IME composition, dead keys, cluster
  boundaries, bidi, RTL, emoji, selection, undo/redo, and a clipboard that
  round-trips rich text.
- **Keyboard**: scoped shortcut registry with conflict detection, spatial
  navigation for TV/remote, and a visible focus ring that is never suppressed.
- **Drag and drop**: internal (widget → widget) and external (files, text, images).
- **Accessibility** is a first-class output: an AccessKit tree maintained
  alongside the scene, with roles, labels, and live regions.

## 7. Animation model

- **Springs by default.** Interruptible, velocity-preserving. A spring retargeted
  mid-flight never jumps.
- **Tweens and keyframe timelines** for scripted motion.
- **Layout animation**: animating between two layout results without re-laying
  out every frame.
- **Shared-element transitions** keyed by a stable id.
- **Deterministic**: fixed-step integration, injectable clock, so animation tests
  are reproducible.
- **Cheap when idle**: the scheduler requests a frame only while something is
  actually moving. No animation, no frame, no battery.

## 8. Layout and responsiveness

- Flexbox (the subset that matters), grid, absolute, and z-stack.
- Constraint-based sizing with an **intrinsic-size measure cache** keyed by
  `(node, constraints)` — the single biggest layout win in retained toolkits.
- Incremental: only dirty subtrees are laid out.
- Responsive primitives: breakpoints, container queries, density modes.
- Correct by default: fractional DPI, safe-area insets, RTL mirroring, logical
  pixels everywhere, physical pixels only at the GPU boundary.

## 9. Rendering model

- **`mre-raster` first** — a software rasteriser that is the reference
  implementation. It makes golden-image testing possible with no GPU and proves
  correctness independently of any driver.
- **`mre-gpu` second** — wgpu backend covering Vulkan, Metal, DX12 and WebGPU
  from one shader set. Instanced quads for rects, a glyph atlas for text, an SDF
  path for icons, separated passes for blur and shadow.
- **Batching is the renderer's job, never the widget's.** Draw calls come from
  sorted batch keys; the gate is ≤ 64 for the reference scene.
- **Damage-driven**, with layers as the escape hatch: a composited subtree gets a
  cached texture and is re-rendered only when dirty.

## 10. Accessibility

AccessKit from M3, not "later". Roles, names, values, states, focus order,
screen-reader announcements, and a text-input protocol. The tree is derived from
the scene, so it cannot drift out of sync with what is on screen.

## 11. Platform matrix

| Platform | Backend | Input | Status |
|---|---|---|---|
| Linux | Vulkan (wgpu) + software | mouse, keyboard, touch, pen | M2 |
| Windows | DX12 (wgpu) + software | mouse, keyboard, touch, pen | M2 |
| macOS | Metal (wgpu) + software | trackpad, keyboard | M2 |
| Web (wasm32) | WebGPU, WebGL2 fallback | pointer, touch, keyboard | M7 |
| Android | Vulkan (wgpu) | touch, pen, back gesture, IME | M7 (CI compile only) |
| iOS | Metal (wgpu) | touch, keyboard, safe areas | M7 (CI compile only) |

`mre-capi` exposes a C ABI so Swift, Kotlin, C# and Python can host the engine
without a Rust toolchain.

## 12. Performance system (already built, extended every milestone)

`perf/budget.toml` is compiled into the engine. Gates in CI:

| Gate | Enforced by | Threshold |
|---|---|---|
| Steady-state allocations | `gate_alloc.rs` | exactly 0 |
| Node size | `gate_structure.rs` | 64 B |
| Virtualised scene memory | `gate_structure.rs` | ≤ 64 KiB @ 50k rows |
| Draw calls | `gate_structure.rs` | ≤ 64 |
| Frame p99 @120 Hz | `bench.yml` (dedicated runner) | ≤ 8.33 ms |
| Idle frames per second | `bench.yml` | 0 |
| Compile time, warm | `compile-time` job | < 5 s |
| Binary size | `release.yml` | ≤ 2 MB |

New gates added per milestone: golden-image diff (M1), GPU draw-call counts (M2),
animation determinism (M3), Lua re-render count per interaction (M4).

## 13. Milestones and exit criteria

| M | Name | Exit criteria |
|---|---|---|
| **M0** ✅ | Foundation & perf harness | zero-alloc gate, structural gates, reference workload, egui adapter |
| **M1** | Layout, text, software raster | text renders correctly to PNG; golden-image suite; §8 and §9 (raster) done |
| **M2** | GPU + window + paint | the reference table runs in a real window at 120 Hz; draw calls ≤ 64 |
| **M3** | Animation, input, a11y | gestures, IME, focus, AccessKit; springs deterministic |
| **M4** | Lua host | the counter example runs from Lua; hot reload; re-render count gated |
| **M5** | Widget library | egui feature parity matrix green |
| **M6** | C ABI + bindings | a C program and a Python script render a window |
| **M7** | Web, Android, iOS | wasm demo; CI compiles Android and iOS |
| **M8** | Release | docs site, examples, 1.0 |

## 14. Feature parity with egui

Tracked in [`FEATURE-PARITY.md`](FEATURE-PARITY.md). Summary: 14 categories, 61
features. The rule is not "match the list" — it is **match the behaviour and beat
the frame time**. A checkbox without a benchmark is not parity.

## 15. CI and heavy testing

The laptop is for editing and fast tests. Everything expensive runs on GitHub
Actions (see `.github/workflows/`):

| Workflow | Runs | Purpose |
|---|---|---|
| `ci.yml` | every push | fmt, clippy `-D warnings`, unit + perf gates, 3-OS matrix |
| `bench.yml` | per PR + nightly | criterion + instruction counts, regression > 3% fails |
| `golden.yml` | per PR | headless render → PNG diff across platforms |
| `cross.yml` | nightly | wasm32, Android (cargo-ndk), iOS compile |
| `fuzz.yml` | nightly | layout, text, Lua parser, scene mutations |
| `sanitizers.yml` | nightly | ASan/UBSan on `mre-raster`, Miri on `mre-core` |
| `release.yml` | tags | binaries, wasm bundle, size gate |

## 16. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Reactive invalidation bugs (stale UI) | Correctness, trust | Deterministic frame tests; "dirty set must be empty after frame" assertion |
| Lua binding overhead in the hot path | Loses bet #4 | Tier-2/3 bindings bypass Lua entirely each frame |
| Text shaping is harder than expected | Schedule | Start with a correct-but-simple shaper, upgrade behind a trait |
| `wgpu` compile time | Dev loop | Feature-gated; the core builds and tests without it |
| Scope creep into "framework" | Never ships | Non-goals, milestone exit criteria, feature matrix |
| Benchmarks flatter us | Credibility | Comparison protocol with scope markers and published losses |

## 17. Decision log

| # | Decision | Alternatives | Rationale |
|---|---|---|---|
| 1 | Rust core | Go, C#, Zig | Determinism, no GC, provable zero-alloc |
| 2 | Retained + reactive | Immediate mode | O(changed) instead of O(UI); the whole thesis |
| 3 | Scoped re-render + property bindings | Pure vdom; pure fine-grained | Balances correctness and hot-path cost |
| 4 | Lua 5.4 embedded via `mlua` | LuaJIT, Wren, JS | Small, embeddable, well-understood, fast enough; LuaJIT as an option later |
| 5 | wgpu | Custom Vulkan/Metal, OpenGL | One backend for 5 platforms; the only sane multiplatform choice |
| 6 | Software rasteriser as reference | GPU-only | Golden tests without a GPU; correctness independent of drivers |
| 7 | 64-byte nodes in a generational arena | `Rc<RefCell>` tree | Cache-linear, no refcounts, no leaks |
| 8 | Budget file compiled in | Runtime config | Budgets become part of the program; cannot drift |
