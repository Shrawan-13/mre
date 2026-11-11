# Comparing MRE against other engines

Publishing a fast benchmark is easy. Publishing one that survives scrutiny is
not. This document is the protocol, and it is binding for any comparison we make
in public.

## Peer tiers

"Best SOTA lightweight engine" is not one thing. Beating Flutter is a different
claim from beating egui.

| Tier | Engines | What a win means |
|---|---|---|
| **A — peers** | egui, iced, Slint, Xilem/Masonry | Same class. We must equal or beat these. |
| **B — heavyweights** | Flutter (Impeller), Qt Quick | Full frameworks. Matching them is a win; beating them is a headline. |
| **C — renderers** | tiny-skia (CPU), Vello (GPU) | Benchmarks our paint layer in isolation, not the toolkit. |
| **D — platform baselines** | DOM, Canvas2D | What "an app" costs with zero engine. |

**Headline claim, stated in advance:** equal or better on every axis, and
decisively better on incremental frames and idle cost. Not a blanket "faster".

## Fairness protocol

Every published comparison must satisfy all of these.

1. **One scene spec.** `perf/scenes/table.toml` is the only definition of the
   workload. Adapters read it; nobody hard-codes a different table.
2. **Pixel-equivalence gate.** Output is compared against a golden image with a
   per-pixel tolerance. A faster engine that draws less is not faster.
3. **Identical environment.** Same machine, OS, driver, resolution, DPI, refresh
   rate, build profile (release), and vsync setting. Recorded with the results.
4. **Percentiles, never a mean.** p50 / p95 / p99 / p999 / max.
5. **Axes reported separately.** CPU frame time, GPU frame time, RSS, idle CPU,
   binary size, cold start. Never blended into one "score".
6. **Interleaved runs.** A/B/A/B/..., not all-A-then-all-B, to cancel thermal
   drift and frequency scaling. Report the median of runs.
7. **Warm-up before measurement**, and the same warm-up count for every engine.
8. **Feature parity stated.** A speed table without a feature matrix is
   marketing. Text shaping quality, AA, subpixel positioning, RTL, accessibility
   and animation support all cost time; if an engine lacks them it is not
   comparable.
9. **Measurement scope stated.** "Layout + paint on CPU" is not "end-to-end
   frame time". Every result carries its scope.
10. **Losses published.** If we lose an axis, it goes in the table with the same
    prominence as a win.

## Workloads

| ID | Workload | Stresses |
|---|---|---|
| W1 | idle | cost of doing nothing |
| W2 | scroll, 4 px/frame | virtualisation hot path |
| W3 | fling, 40 px/frame | row recycling, run-cache churn |
| W4 | cold start | allocation, font load, pipeline creation |
| W5 | 10k inserts | structural mutation |
| W6 | resize storm | layout invalidation |
| W7 | text ticker | shaped-run cache |
| W8 | 20 windows | memory scaling |

W1-W4 are implemented. W5-W8 require layout, text and windowing to exist.

## Adapter contract

Every baseline is a standalone binary in `baselines/<engine>/` that:

1. renders the scene described by `perf/scenes/table.toml`,
2. measures with the same protocol (warm-up, per-frame timing, percentiles),
3. writes JSON to `perf/results/<engine>-<unix>.json` with this schema:

```json
{
  "engine": "egui",
  "scope": "layout+paint+display_list (cpu, no gpu submit, no shaping, no rasterisation)",
  "scene": "table-50k",
  "results": [{
    "engine": "egui", "workload": "W2-scroll", "frames": 600,
    "p50_ns": 0, "p95_ns": 0, "p99_ns": 0, "p999_ns": 0, "max_ns": 0,
    "draw_calls": 0, "commands": 0, "allocs_per_frame": 0.0
  }]
}
```

4. reports the **same scope** as MRE. If an adapter cannot, it must say so in its
   `scope` field and the comparison marks the row as non-comparable.

`cargo run --release -p mre-bench -- compare` joins all result files on
`(scene, workload)` and prints one table.

## Where we expect to win, and where we expect not to

Stated before measuring, so it can be falsified.

| Axis | Expectation | Structural reason |
|---|---|---|
| Incremental / scroll frames | **win, 10-50x** | egui re-runs layout and paint for the whole tree every frame; retained + damage + recycled rows does O(visible) |
| Idle | **win, unbounded** | event-driven: no input, no frame. Immediate-mode engines repaint continuously unless explicitly throttled |
| Memory per node | **win** | 64-byte nodes in one arena vs. boxed nodes with refcounts |
| Full redraw throughput | **parity at first** | this is the incumbents' home turf; they are hand-tuned |
| Cold start vs. Qt/Flutter | **loss, initially** | they preload shared libraries; we start from a cold binary until we cut weight |
| Text shaping | **loss, initially** | HarfBuzz has a decade of scripts we do not |

## Known unfairnesses we must disclose

* **Retained vs. immediate.** Comparing our incremental frame to egui's full
  rebuild is a real user-facing advantage, but it is *also* a different
  architecture. Report both "scroll" (incremental) and "full redraw" so the
  reader can see the difference rather than infer it.
* **Feature surface.** egui, for example, ships text layout, accessibility hooks,
  and animation easing that MRE does not. Until parity, our numbers are
  "engine core only" and must be labelled as such.
* **Virtualisation support.** `show_rows` in egui is its virtualisation API; using
  it is the fair equivalent of our row pool. Using a non-virtualised egui list
  would produce a meaningless 1000x.
* **GPU backends differ.** Flutter's Impeller and Skia backends differ by up to
  2x on the same scene. Name the backend in every row.

## Running a comparison

```bash
# 1. MRE
cargo run --release -p mre-bench -- --count-allocs

# 2. Each baseline
cargo run --release --manifest-path baselines/egui/Cargo.toml
cargo run --release --manifest-path baselines/iced/Cargo.toml

# 3. Join
cargo run --release -p mre-bench -- compare
```

---

## First results — 2026-10-30

Machine: Fedora, 4 cores, release builds, `lto = "thin"`, single interleaved
session. MRE `0.1.0`, egui `0.37`.

| workload | engine | p50 | p99 | draw calls | allocs/frame |
|---|---|---|---|---|---|
| W1-idle | mre | 0.37 µs | 0.37 µs | 5 | 0.00 |
| W1-idle | egui * | 461.51 µs | 1126.44 µs | 2 | n/a |
| W2-scroll | mre | 1.81 µs | 2.67 µs | 5 | 0.00 |
| W2-scroll | egui * | 737.44 µs | 1606.96 µs | 2 | n/a |
| W3-fling | mre | 1.76 µs | 1.77 µs | 5 | n/a |
| W3-fling | egui * | 547.41 µs | 1287.70 µs | 2 | n/a |

### This is not a win, and must not be reported as one

The `*` is doing real work. The two rows measure different amounts of work:

* egui's window includes **text shaping and tessellation**, and it emits
  **5,380-6,248 vertices** per frame — proof it is generating real geometry.
* MRE's window includes **layout + display-list build only**. It emits
  **232 commands and zero triangles**. There is no renderer behind it yet.

A 600x gap between "build a list of 232 rectangles" and "shape, lay out and
tessellate a 50k-row text table" is expected arithmetic, not an achievement.
Quoting it as "MRE is 600x faster than egui" would be dishonest, and this
document exists so that claim cannot be made accidentally.

### What the numbers *do* establish

1. **The harness works end to end** — same scene, same protocol, same JSON
   schema, joined automatically by `mre-bench compare`.
2. **The idle gap is structural, not incidental.** egui rebuilds and
   re-tessellates the visible table every frame (461 µs idle). MRE's idle frame
   does zero work because damage is empty and no repaint occurs. That difference
   survives the scope caveat: it is the retained-vs-immediate distinction the
   architecture is built around.
3. **A p999 outlier is visible and explainable.** W3-fling shows a 141.90 µs
   maximum against a 1.77 µs p99 — one sample in 600. That is either scheduler
   steal on a 4-core desktop or a page fault, and it is exactly the kind of
   number a mean would have erased. It needs interleaved runs to confirm.

### What must happen before a legitimate headline claim

1. Text: shaped runs, glyph atlas, and shaping in the measured window.
2. Tessellation: real triangle output, so `vertices` is non-zero and comparable.
3. A GPU backend, so `submit` is in scope.
4. Pixel-equivalence check against egui's output for the same scene.
5. Interleaved A/B/A/B runs, median of 5, on a quiet machine.

Until all five hold, the only claim that may be made is the one in the
architecture document: *this design is built to lose less work per frame, and the
gates exist to prove it when the work is actually equivalent.*

---

## CI-verified run — GitHub Actions, 2026-10-30

The comparison now runs in the cloud (`bench.yml`, interleaved A/B/A/B, four
rounds each, same machine, same job) so the laptop is never the measuring
instrument. First artifacted result:

| workload | engine | p50 | p99 | vs best p99 | draw calls | allocs/frame |
|---|---|---|---|---|---|---|
| W1-idle | mre | 0.11 µs | 0.12 µs | 1.00x | 5 | 0.00 |
| W1-idle | egui `*` | 110.18 µs | 158.27 µs | 1308.00x | 2 | 0.00 |
| W2-scroll | mre | 0.31 µs | 0.44 µs | 1.00x | 5 | 0.00 |
| W2-scroll | egui `*` | 112.35 µs | 131.04 µs | 297.14x | 2 | 0.00 |
| W3-fling | mre | 0.31 µs | 0.44 µs | 1.00x | 5 | 0.00 |
| W3-fling | egui `*` | 123.38 µs | 131.04 µs | 346.28x | 2 | 0.00 |

`*` still marks the scope mismatch. The ratios are **not** a claim of being
300x faster; they quantify how much less work the retained path does when it has
nothing to do. The five conditions above remain unmet, and until they are met
the headline stays unclaimed.

One asymmetry worth recording: the runner is roughly 4x faster than the
development laptop on the egui side (110 µs vs 461 µs idle), which is precisely
why the protocol requires both engines on the same machine. Absolute numbers are
not portable; ratios within a run are.

---

## Same-scope run — 2026-11-11, damage tracking landed

The `mre-cpu` adapter now measures the full Lua-free widget pipeline — layout,
build, shaping and CPU rasterisation into pixels — which is the same class of
work as egui's tessellation window. Both rows sit in scope class `cpu-pixels`,
and the compare table only ranks rows that have a peer in that class.

A new workload, **W5-hover**, exercises the interaction path the damage
tracker exists for: the pointer glides down the list one row per frame and the
row under it highlights. egui's adapter runs the identical scenario (pointer
events injected, hovered row re-filled), so the row has a peer.

Machine: Fedora, 4 cores, release, interleaved within one session, quiet
enough for single-run honesty (browser was closed for these numbers).

| workload | engine | p50 | p99 | draw calls | notes |
|---|---|---|---|---|---|
| W1-idle | mre-cpu | 309 µs | 499 µs | 0 | 600/600 frames painted nothing |
| W1-idle | egui | 440 µs | 689 µs | 2 | re-tessellates every frame |
| W2-scroll | mre-cpu | 4450 µs | 5517 µs | 600 | full body repaint, CPU pixels |
| W2-scroll | egui | 460 µs | 776 µs | 2 | tessellation only |
| W3-fling | mre-cpu | 5399 µs | 7131 µs | 600 | full body repaint |
| W3-fling | egui | 501 µs | 831 µs | 2 | tessellation only |
| W5-hover | mre-cpu | 628 µs | 4500 µs | 600 | damage = two rows |
| W5-hover | egui | 487 µs | 803 µs | 2 | repaints everything |

### What this run establishes

1. **Idle is now a measured win, not a promise.** MRE's idle frame does no
   rasterisation at all (0 draw calls) and still pays its ~310 µs of
   build+layout; egui pays 440 µs to re-tessellate a table nothing changed.
   The retained claim survived its first apples-to-apples test.
2. **Interaction frames cost what they change.** A hover repaints two rows
   (~90k pixels) instead of the window (1.3M pixels). Without damage tracking
   this was the same cost as a scroll frame.
3. **Scroll and fling remain an honest loss.** Rasterising 27 rows of text on
   the CPU is ~10x the cost of tessellating them. The structural fixes are
   known and scheduled: a GPU backend moves rasterisation off the CPU, and a
   scroll-blit (translate the previous frame's pixels, redraw the exposed
   band) would make scrolling O(exposed band) instead of O(window).

### What the W5 row costs that egui's does not

MRE's hover frame still runs the full O(tree) build + layout + signature diff
(~320 µs) even though the pointer moved and nothing structural changed. The
known fix is per-node memoisation (fine-grained reactivity over the layout
tree); it is next on the roadmap and will cut that overhead.

### Protocol note

The mre-cpu rows in this run include the damage-tracker contract: a frame
whose damage is empty paints nothing and the host copies nothing. egui has no
equivalent (it rebuilds every frame by design), so idle and hover comparisons
measure the architecture, not just the code. This is the asymmetry the
"Known unfairnesses" section asks to disclose, and it is disclosed per-row by
the `draw calls` column.

---

## Scroll blit — 2026-11-11, later the same day

The damage tracker now detects pure translations between layouts (the same
content, moved by one axis-aligned integral offset, nothing else changed) and
**copies the previous frame's pixels instead of re-rasterising them**,
repainting only the vacated and exposed bands. The detection is conservative:
gaps between the moved nodes, another node crossing the region, a
non-integral offset at this density, or a node that overlaps the region all
fall back to repainting, which is always correct. A runtime test proves the
blitted frame is pixel-identical to a full repaint.

| workload | engine | p50 (median of 3 interleaved) | structural facts |
|---|---|---|---|
| W1-idle | mre-cpu | 390 µs | 0 draw calls, 600/600 frames paint nothing |
| W1-idle | egui | 457 µs | re-tessellates every frame |
| W2-scroll | mre-cpu | 2.1–5.0 ms (load-dependent) | 45 draw calls/frame, 326 glyph blits/frame |
| W2-scroll | egui | 484 µs | tessellation only |
| W3-fling | mre-cpu | 5.9 ms | 68 calls/frame, 557 glyph blits/frame |
| W3-fling | egui | 563 µs | tessellation only |
| W5-hover | mre-cpu | 811 µs | damage = two rows |
| W5-hover | egui | 556 µs | repaints everything |

### Reading this honestly

* The scroll rows moved from ~10.3 ms to a median of ~5.0 ms under this
  session's load, with quiet-machine runs at 2.1 ms. The blit made the frame
  memory-bandwidth-bound (a ~5 MB pixel copy), which is why background load
  hits it harder than it hits egui — a fairness note, not an excuse.
* Idle is a stable win across every run: an unchanged frame does no
  rasterisation at all and still beats egui's full re-tessellation.
* Hover is within 1.5x of egui under load; the remaining cost is MRE's
  per-frame build + layout + signature diff (~0.3–0.4 ms), which
  fine-grained reactivity will remove.
* The engine rows are still the only ones that rasterise pixels in the
  measured window; egui defers rasterisation to the GPU. On a GPU backend the
  mre-cpu rows move to the same class as egui's — that is the next
  architectural step, not an optimisation of this one.
