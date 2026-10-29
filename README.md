# MRE

A lightweight, retained-mode, multiplatform UI engine for applications — with
**Lua as the application language**, and a performance contract that cannot
silently regress.

> **Status: pre-alpha (M0 complete, M1 in progress).** The perf harness, the
> scene, the reactive Lua graph and the CI for heavy testing exist. Layout,
> text, rasterisation, GPU and widgets do not yet. That ordering is deliberate —
> see [Why the harness came first](#why-the-harness-came-first) and the
> milestone plan in [docs/PLAN.md](docs/PLAN.md).

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
local mre = require("mre")

local count = mre.signal(0)
local pop = mre.spring(1.0, { stiffness = 240, damping = 22 })

mre.effect(function() pop.target = 1 + count() * 0.04 end)

mre.mount(function()
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
```

Run the working example today:

```bash
cargo run -p mre-lua --example headless -- examples/counter.lua --interact
```

```
tree:
  (column (text "MRE counter") (text "Not clicked yet") (row (button) (button)))
stats: Stats { signals: 2, computations: 3, dirty: 0, last_flush_runs: 1, ... }

after two clicks:
  tree:  (column (text "MRE counter") (text "Clicked 2 times") (row (button) (button)))
  reruns: 3 (the view, the memo and its one observer; writes were coalesced)
  idle flush ran 0 computations (expected 0)
```

The API is specified in [docs/LUA-API.md](docs/LUA-API.md). The current
implementation covers signals, effects, memos, `untracked`, batching, cycle
detection and the declarative vnode surface. Components, fine-grained property
bindings and hot reload are M4.

## Layout

```
crates/
  mre-core/    ids, generational arena, geometry, colour       (no deps, no unsafe)
  mre-perf/    budget contract, allocation counter, statistics
  mre-scene/   retained tree, dirty flags, display lists, reference table
  mre-lua/     reactive graph + Lua host (the only heavyweight dependency)
  mre-bench/   benchmark runner + the performance gates
examples/
  counter.lua  a complete app, in Lua
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
cargo run -p mre-lua --example headless -- examples/counter.lua --interact
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
