# Navigation and screens

Navigation is app composition, so it lives entirely in Lua. The host only maps
its native back event into `App::back()`; route definitions, stacks, tabs,
modals and transitions are byte-identical on Android, desktop, iOS and web.

The module is `framework/nav.lua`, loaded automatically by `framework/init.lua`.
It is available as `mre.nav` and its alias `mre.router`.

```lua
local nav = mre.nav {
  routes = {
    chats    = ChatsScreen,
    thread   = { view = ThreadScreen, title = "Thread" },
    settings = SettingsScreen,
  },
  initial = "chats",
  persist = "nav.stack",
}

mre.app {
  view = function()
    return nav:view()
  end,
}
```

A screen is a plain component. It receives one context table:

```lua
local function ThreadScreen(ctx)
  return mre.column {
    ctx.nav:back_button(),
    mre.text { "Thread " .. tostring(ctx.params.id) },
    mre.scroll {
      key = "thread-" .. tostring(ctx.params.id),
      -- ordinary flow children; the engine owns scrolling
    },
  }
end
```

`ctx` contains `nav`, `params`, `route` and `id`.

## Stack API

| Method | Meaning |
|---|---|
| `nav:push(name, params)` | Push a route. |
| `nav:replace(name, params)` | Replace the top route. |
| `nav:pop()` | Pop one route; never pops the root. |
| `nav:pop_to(name)` | Pop back to the nearest matching route. |
| `nav:reset(name, params)` | Clear the stack to one fresh route. |
| `nav:top()` | Current entry `{ name, id, params }`. |
| `nav:title()` | Current route title (string or function). |
| `nav:state()` | Read-only copy of the whole state. |
| `nav:can_back()` | Whether back is available. |
| `nav:back()` | Dismiss a modal or pop one route. |
| `nav:open(path, params)` | Deep link, e.g. `"chats/thread?id=4"` is parsed as `chats/thread`. |
| `nav:back_button(props)` | A composed button that hides itself when back is unavailable. |
| `nav:destroy()` | Unregister the back handler. |

## Tabs

Each tab owns an independent stack. Switching tabs preserves every stack.

```lua
local nav = mre.nav {
  routes = { chats = Chats, detail = Detail, settings = Settings },
  tabs = {
    { id = "chats", title = "Chats", initial = "chats" },
    { id = "settings", title = "Settings", initial = "settings" },
  },
  active_tab = 1,
}

nav:set_tab("settings")
nav:tab()  -- "settings"
```

## Modals

Modals are a separate overlay stack rendered after the main screen, so a
`mre.dialog` route covers safe-area bands correctly.

```lua
nav:open_modal("detail", { id = 7 })
nav:dismiss_modal()
```

Back always dismisses the topmost modal before touching a stack. The priority
chain is:

1. focused text field (dismiss keyboard),
2. topmost modal,
3. active stack (`pop`),
4. app handlers registered with `mre.on_back(fn, priority)`,
5. a built-in priority `-100` handler that calls `mre.exit()`.

Navigators register at priority 20; app handlers default to 0. A handler
returning `true` consumes the event, so an app can veto or replace the default
exit (for example to show a confirm dialog).

```lua
mre.on_back(function()
  if unsaved_changes() then
    confirm_exit(true)
    return true
  end
  return false -- allow platform exit
end)
```

`mre.exit()` asks the host to close the app: `Activity.finish()` on Android,
window exit on desktop, or history navigation on web.

## Persistence and identity

Navigation state is encoded with a bounded, length-prefixed Lua codec and kept
in `mre.persist(key)`. It survives hot reload and process restarts. Every entry
has a persisted numeric `id`; screen keys are derived from it, so:

- popping back restores that screen's scroll offset and editor state,
- pushing the same route again creates fresh state,
- `reset` allocates new ids and really clears state.

Route params must be serialisable scalars or tables of scalars. Cyclic tables,
functions and huge blobs are rejected with a warning instead of crashing a frame.

## Transitions

Transitions use `animate = { opacity = spring, x = spring }`. The engine steps
the springs and writes the properties directly, so no Lua runs per frame and the
full display refresh rate is preserved. Set `transition_ms = 0` to disable them.

```lua
local nav = mre.nav {
  routes = { home = Home, detail = Detail },
  initial = "home",
  transition_ms = 240,
  spring = { stiffness = 260, damping = 26 },
}
```

The outgoing screen stays keyed until a single `mre.after` timer removes it.
Interruptions cancel the previous transition and start from the current spring
value, which is the same model production navigators use.
