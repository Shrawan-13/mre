# The performance contract

`perf/budget.toml` is the single source of truth. It is parsed at build time by
`mre-perf/build.rs` and compiled into constants, so a budget is part of the
program rather than a number in a document that drifts.

## Rules

1. **Tightening a number requires no justification.** Do it.
2. **Loosening a number requires a written justification** in the change log
   below, in the same commit.
3. **Deleting a gate requires approval.** The gates are the product.
4. **A gate that flakes gets fixed, not muted.** A muted gate is a deleted gate.

## The gates, and why each exists

### Zero allocations per frame

`crates/mre-bench/tests/gate_alloc.rs`

The strongest gate in the repository. Frame time can be explained away; an
allocation cannot. Every allocation is an unbounded latency source, and it is how
GC-shaped stutter returns to a language that has no GC.

Covered windows: `idle`, `scroll`, `fling`, `reverse`, each 500-1000 frames.

The file also contains a canary that *allows* one allocation. If the canary fails,
the instrument is broken — which is the failure mode that would otherwise let the
gate pass silently.

### Structural budgets

`crates/mre-bench/tests/gate_structure.rs`

Deterministic, no timing, safe on shared CI runners:

| Gate | Value | Why |
|---|---|---|
| `size_of::<Node>()` | 64 | one cache line per node |
| `size_of::<Command>()` | <= 32 | stream two commands per cache line |
| arena bytes per node | <= 96 | bookkeeping must stay small |
| scene memory, 50k rows | <= 64 KiB | virtualisation still works |
| draw calls, reference scene | <= 64 | batching still works |

### Scene shape

`crates/mre-bench/tests/gate_scene_shape.rs`

The failure mode this catches is silent un-virtualisation: the table looks
correct, but a 10M-row data set now materialises 10M nodes. `node_count()` must
be identical for 1k and 10M rows.

### Frame time and stability

`perf/budget.toml` `[frame]`, `[stability]`, `[weak_device]`

Timing is *reported* by `mre-bench` and *gated* only where the runner is
trustworthy. Wall-clock assertions on shared CI are flaky, and flaky gates get
muted, so timing gates run on a dedicated machine.

The contract is on percentiles, never the mean:

* `p99 <= 1.0x` frame budget — no jank at 120 Hz
* `p999 <= 2.0x` frame budget — no visible hitch every ~8 s

A mean can be 4 ms while every 40th frame takes 30 ms. The mean is reported and
distrusted.

## Allocation of the frame budget (120 Hz)

| Stage | Budget | Note |
|---|---|---|
| input + events | 0.30 ms | |
| state update | 0.50 ms | |
| layout | 2.00 ms | the real cost in retained toolkits |
| paint | 1.00 ms | display-list build |
| tessellation | 1.50 ms | parallelisable |
| submit | 1.00 ms | |
| **headroom** | **2.00 ms** | 24% — a machine with zero headroom has negative headroom on a real one |

`budget::frame` tests assert the stages sum to no more than the total.

## Measurement scope

Today `mre-bench` measures **layout + paint + display-list build on the CPU**. No
GPU submit, no text shaping, no rasterisation.

That scope is printed with every run and embedded in every JSON result, because
a number without a scope is a marketing claim. As stages land, the scope string
changes and historical results are marked incomparable.

## Change log

| Date | Change | Reason |
|---|---|---|
| 2026-10-30 | initial contract v1 | baseline: 120 Hz budget, 64-byte nodes, zero allocations |

## Open gates

Defined but not yet enforceable, because the subsystems do not exist:

* `[startup] first_frame_ns` — needs the real engine boot path.
* `[idle] frames_per_idle_second = 0` — needs the event loop.
* `[batching] max_pipeline_switches` — needs a GPU backend.
* `[memory] gpu_resident_bytes` — needs the layer/atlas cache.

They are in the contract now so they cannot be forgotten later.
