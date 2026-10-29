# Architecture

## The pipeline

```
  set_state ──> layout ──> paint ──> tessellate ──> submit
                  │          │            │
                  │          │            └── parallel, work-stealing
                  │          └── display list (flat, reusable)
                  └── dirty: LAYOUT          dirty: PAINT / TEXT
```

Each arrow is a place where work can be skipped. Skipping is the entire
performance strategy: a retained toolkit that does not skip is just an
immediate-mode toolkit with extra memory.

## Layering

| Layer | Crate (now → later) | Owns |
|---|---|---|
| Primitives | `mre-core` | ids, arena, geometry, colour |
| Instrumentation | `mre-perf` | budget, allocation counting, percentiles |
| Scene | `mre-scene` | retained tree, dirty flags, display lists |
| Layout | `mre-layout` *(next)* | flex/grid, constraint solver, measure cache |
| Text | `mre-text` *(next)* | shaping, glyph atlas, run cache |
| Paint | `mre-paint` *(next)* | batching, clip stack, layer cache |
| GPU | `mre-gpu` *(next)* | backend trait + wgpu implementation |
| Window | `mre-window` *(next)* | winit / raw handles, event loop |
| Widgets | `mre-widgets` *(next)* | buttons, lists, text input, scroll |
| Bindings | `mre-capi` *(next)* | C ABI for Swift / Kotlin / C# / Python |

Layering rule: **dependencies point down only.** `mre-core` knows nothing about
scenes; `mre-scene` knows nothing about GPUs. This is what makes it possible to
compile and test the interesting parts in under a second.

## Why a generational arena

The textbook UI tree is `Rc<RefCell<Node>>`. It is also the single biggest
reason hobby engines stutter:

* One heap allocation *per node*, so a 50k-row list means 50k allocations.
* One pointer indirection *per traversal step*, so every step is a cache miss.
* 16 bytes of refcount overhead per node on top of the payload.
* Node addresses are scattered, defeating prefetching entirely.
* `RefCell` borrow checks on the hot path.
* Cycle leaks, requiring manual `Weak`.

The arena replaces all of that with a `Vec<Entry<T>>` plus a free list:

* Nodes are contiguous, so a full traversal is a linear scan.
* Insert, remove and lookup are O(1).
* Ids are `(index, generation)`; a removed slot's generation is bumped, so a
  stale id can never resolve to the slot's next occupant.
* `generation` is a `NonZeroU32`, which makes `Option<Id>` **8 bytes** instead of
  12. That niche is load-bearing: it is why a node with five links fits in 64
  bytes.

## Why a 64-byte node

```rust
pub struct Node {
    kind: NodeKind,             // 1   \
    dirty: Dirty,               // 1    | 4
    parent: Option<Id>,         // 8   \
    first_child: Option<Id>,    // 8    |
    last_child: Option<Id>,     // 8    | 40
    next_sibling: Option<Id>,   // 8    |
    prev_sibling: Option<Id>,   // 8   /
    bounds: Rect,               // 16
    payload: u32,               // 4
}                               // == 64 bytes
```

Exactly one cache line, asserted by a test. Doubly-linked child lists give O(1)
append *and* O(1) unlink without a per-parent `Vec` that could reallocate mid-frame.

## Dirty flags

```rust
Dirty::NONE | Dirty::LAYOUT | Dirty::PAINT | Dirty::TEXT
```

* `LAYOUT` — bounds or children changed.
* `PAINT` — visual output changed.
* `TEXT` — shaped-run caches must be invalidated.

The invariant: **a stage performs no work for a node whose relevant flag is
clear.** The reference workload demonstrates the end state — an idle frame
produces `damage == EMPTY` and performs zero paint work.

## Display lists

Paint emits a flat `Vec<Command>`, not a visitor callback over the tree:

* it can be cleared and reused, so steady-state painting allocates nothing;
* it can be retained per layer and structurally shared;
* it can be diffed;
* it can be sorted into batches by a GPU backend without touching the scene.

A `Command` is 24 bytes, so two fit per cache line. Anything fat (glyph runs,
images, paint servers) is an index into a side table.

## Batching

Draw calls come from batching, never from widgets. `Command::batch_key()` is the
first axis: `(kind, colour)` today, extended with texture, clip and pipeline as
those land. The gate is `<= 64` draw calls for the reference scene.

## Threading model

Three clocks, one submit:

```
  UI thread        paint thread         GPU
  input + state    display-list build   queue
  layout           tessellation         present
```

* Layout is single-threaded (it is latency-bound, not throughput-bound).
* Tessellation may be parallel behind a task graph; it is throughput-bound.
* Exactly one submit per frame. Never block the GPU on the CPU.

## What makes this fast *permanently*

Not the code above. The gates:

* `tests/gate_alloc.rs` — zero allocations in idle, scroll, fling and reverse.
* `tests/gate_structure.rs` — node size, per-node bytes, pool sizing, draw calls.
* `tests/gate_scene_shape.rs` — the scene's shape is independent of the data set.
* `perf/budget.toml` — compiled-in budgets, checked by the bench.

Code gets refactored. Gates do not care.
