# Lua API specification

MRE applications are written in Lua 5.4 (embedded via `mlua`, LuaJIT optional).
This document is the contract between the scripting layer and the engine.

Design rules:

1. **The frame is the engine's, not Lua's.** Lua never runs per frame unless
   something changed. Scroll, hover and animation touch no Lua at all.
2. **Everything is a table literal.** UI is described declaratively; the engine
   owns the instance.
3. **No hidden globals.** The `mre` module is the only entry point; app code is
   sandboxed.
4. **Errors are contained.** A component that throws is replaced by an error
   boundary, not a crashed app.

---

## 1. Reactivity

### Signals

```lua
local count = mre.signal(0)     -- create
count()                         -- read   (tracks the current computation)
count(5)                        -- write  (invalidates readers)
count(function(v) return v + 1 end) -- update
```

Semantics (Solid-style):

- A **computation** is any component body, `effect`, or `bind` callback.
- Reading a signal inside a computation subscribes that computation to it.
- Writing a signal marks subscribers dirty. Nothing runs immediately.
- Dirty computations are flushed **once per frame**, in dependency order.
- Writes during a flush are batched into the next flush (no cascades within a
  frame), with a max depth of 8 to catch cycles.

### Derived values

```lua
local total = mre.memo(function() return count() * price() end)
```

A memo is a computation whose value is cached until its dependencies change.
`memo` is lazy: it recomputes on read, not on write.

### Effects

```lua
mre.effect(function()
  print("count is now", count())
end)
```

Effects run after the frame's render pass, never during it, so an effect cannot
mutate the UI mid-layout.

### Property bindings (tier 2)

```lua
mre.text { text = function() return "Clicked " .. count() end }
```

Any property may be given a function instead of a value. The engine evaluates it
once, then re-evaluates it only when its dependencies change — **without
re-rendering the component**. This is the path for hover, opacity, colour and
transforms.

### Escape hatch

```lua
mre.untracked(function() return some_signal() end)  -- read without subscribing
mre.batch(function() a(1); b(2) end)                -- one flush
```

---

## 2. Components

```lua
local Card = mre.component(function(props)
  return mre.column {
    padding = 16,
    mre.text { props.title, size = 18 },
    props.children,
  }
end)

mre.mount(function()
  return Card { title = "Hello", mre.text { "body" } }
end)
```

- A component is a function returning a **vnode**.
- Props are a plain table; `children` (from array part) is a vnode list.
- Identity: a component instance is keyed by `(parent, index)` unless a `key` is
  given. Keys control reconciliation, exactly as expected.
- Re-render is scoped: only components that read a changed signal re-run.

---

## 3. Vnodes

| Constructor | Produces |
|---|---|
| `mre.column { gap, padding, align, justify, children }` | flex column |
| `mre.row { ... }` | flex row |
| `mre.grid { columns, ... }` | grid |
| `mre.stack { ... }` | z-stack |
| `mre.scroll { axis, ... }` | scroll container (virtualising when given `rows`) |
| `mre.text { "str", size, weight, color, align, wrap }` | text |
| `mre.image { src, fit }` | image |
| `mre.canvas { on_paint }` | custom painter |
| `mre.layer { ... }` | composited subtree / cached texture |

All accept `style`, `transform`, `clip`, `opacity`, `visible`, `id`, `key`,
`on_*` handlers, and `animate_*` hints.

Layout properties are CSS-shaped but intentionally a subset:
`width/height/min/max` accept numbers (px), `"50%"`, `"auto"`, or `{ fr = 1 }`.

---

## 4. Events

```lua
mre.button {
  label = "Delete",
  on_click   = function(e) ... end,
  on_double  = function(e) ... end,
  on_long    = function(e) ... end,
  on_hover   = function(e) ... end,
  on_pointer = function(e) ... end,   -- raw pointer, with pressure/tilt
  on_key     = function(e) ... end,
  on_focus   = function(e) ... end,
}
```

Event object:

```lua
{
  kind   = "tap" | "pointer" | "scroll" | "key" | "text" | "drop" | "gesture",
  target = <vnode id>,
  x, y   = pointer position, logical px
  dx, dy = delta
  button = "primary" | "secondary" | "middle"
  pressure, tilt_x, tilt_y,
  key, code, mods = { shift, ctrl, alt, meta },
  text,
  velocity = { x, y },
}
```

Handlers may return `true` to stop propagation.

---

## 5. Animation

```lua
local s = mre.spring(0, { stiffness = 240, damping = 22, mass = 1 })
s.target = 100          -- retarget mid-flight, velocity preserved

local t = mre.tween(0, { duration = 300, easing = "ease-out-cubic" })
mre.animate(t, 1)

local tl = mre.timeline {
  { at = 0,   { opacity = 0, y = 12 } },
  { at = 250, { opacity = 1, y = 0 }, easing = "spring" },
}
tl:play()

mre.sequence {
  t:wait(250),
  function() count(1) end,
  t:parallel({ tl:play(), s.target = 0 }),
}
```

Rules:

- Springs are integrated at a fixed timestep; results are deterministic and
  golden-testable.
- An animation writes **property bindings**, never re-renders a component.
- `prefers-reduced-motion` is respected automatically; `mre.motion.reduced()`
  reports it.
- The scheduler wakes the app only while something is animating.

---

## 6. Async and time

```lua
mre.task(function()
  local data = mre.await(http.get("https://api.example.com"))
  rows(data)
end)

mre.after(500, function() toast("saved") end)
mre.every(1000, function() tick() end)
```

Tasks are coroutines on the engine's task queue, resumed between frames. No
Lua-level threads, no blocking of the render thread.

---

## 7. Windowing and app lifecycle

```lua
mre.app {
  title = "Console",
  size  = { 1280, 800 },
  min_size = { 640, 400 },
  theme = mre.theme.dark,
  on_start = function() end,
  on_close = function() return true end,   -- false vetoes
  view = function() return Shell() end,
}
```

Multi-window: `mre.window.open { ... }`. Routes: `mre.router { "/" = Home, "/settings" = Settings }`.
Persistence: `mre.store.get/set` backed by a platform config path.

---

## 8. Errors, debugging and hot reload

- **Error boundaries**: `mre.boundary { fallback = ErrorCard, view = Risky }`.
- `mre.inspect(node)` prints a scene subtree; `mre.stats()` returns node counts,
  re-render counts and per-stage timings for the last frame.
- **Hot reload**: `mre watch app.lua` swaps module code while preserving signal
  state for components whose identity is unchanged.
- Re-render count per interaction is a **gated metric**. A scroll that re-renders
  a component fails CI.

---

## 9. Sandbox and capabilities

App code runs with no filesystem, network or process access by default. Access is
granted explicitly in the app manifest:

```lua
mre.capabilities { fs = { "~/Documents" }, net = { "api.example.com" }, clipboard = true }
```

This matters because MRE apps will be distributed as `.mre` bundles and opened by
users; a scripting engine without a capability model is a security hole.

---

## 10. Performance rules for app authors

1. Never read a signal in a component that should not re-render — use a `bind`.
2. Virtualise long lists with `mre.scroll { rows = n, row = function(i) ... end }`.
3. Give list items stable `key`s.
4. Prefer `memo` over recomputation in render bodies.
5. Do not create signals inside a component body without `mre.persist` — they
   reset on re-render by design.

The engine surfaces violations in `mre.stats()` rather than silently eating them.
