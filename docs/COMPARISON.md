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
