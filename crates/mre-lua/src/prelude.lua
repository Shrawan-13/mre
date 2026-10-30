-- MRE Lua prelude.
--
-- Everything here is defined in Lua rather than Rust because it is pure
-- composition: node constructors, the mount helper, and sugar. Only the
-- reactive primitive graph (signals, effects, flush) lives in Rust, because that
-- is the part that must not be able to allocate per frame or be bypassed.

local mre = ...

mre.version = "0.1.0"

-- ---------------------------------------------------------------------------
-- Vnodes
-- ---------------------------------------------------------------------------
--
-- A vnode is a plain table with a `__mre` kind tag. The engine reads these
-- tables and never mutates them, so they are cheap to construct and easy to
-- inspect from Lua (and from tests).

local function node(kind, props)
    if props == nil then
        props = {}
    elseif type(props) ~= "table" then
        -- `mre.text "hello"` => { "hello" }
        props = { props }
    end

    local out = { __mre = kind }

    -- Copy declared properties (named keys).
    for key, value in pairs(props) do
        out[key] = value
    end

    -- Normalise the array part into `children`, so every container has the
    -- same shape regardless of how it was written.
    local count = #props
    if count > 0 then
        local children = table.new and table.new(count, 0) or {}
        for i = 1, count do
            children[i] = props[i]
        end
        out.children = children
    end

    return out
end

mre.column = function(props) return node("column", props) end
mre.row    = function(props) return node("row", props) end
mre.stack  = function(props) return node("stack", props) end
mre.grid   = function(props) return node("grid", props) end
mre.scroll = function(props) return node("scroll", props) end
mre.layer  = function(props) return node("layer", props) end
mre.button = function(props) return node("button", props) end
mre.image  = function(props) return node("image", props) end
mre.canvas = function(props) return node("canvas", props) end

-- A virtualised list. `render(index)` is called only for the rows inside the
-- viewport, so a list of fifty thousand rows builds about as many nodes as a
-- list of thirty. `offset` is in pixels and `height` is the visible height.
mre.list = function(props) return node("list", props) end

-- Vector icons. Kept as line segments so they are sharp at every density and
-- cost nothing to add to the binary.
mre.icon = function(props) return node("icon", props) end

-- A single-line text field. The engine owns editing state (caret, insertion,
-- deletion) for the focused field, so typing does not depend on the view being
-- re-rendered between keystrokes. `on_change` receives the new value.
mre.text_input = function(props) return node("text_input", props) end

-- Text is special-cased so `mre.text "hi"` and `mre.text { "hi", size = 20 }`
-- both produce something the engine can read without guessing.
mre.text = function(props)
    if type(props) == "string" or type(props) == "number" then
        return { __mre = "text", text = tostring(props) }
    end
    local self = node("text", props)
    if self.text == nil then
        local first = self.children and self.children[1]
        if type(first) == "string" or type(first) == "number" then
            self.text = tostring(first)
            -- A single literal child is not a rich-text run list; hoist it and
            -- drop the array so the node has exactly one representation.
            if #self.children == 1 then
                self.children = nil
            end
        end
    end
    return self
end

-- A separator / spacer, because they come up constantly.
mre.spacer = function(size) return { __mre = "spacer", size = size } end

-- ---------------------------------------------------------------------------
-- Widgets
-- ---------------------------------------------------------------------------
--
-- Composition, not special cases. A checkbox is a row with a box and a label;
-- a slider is a stack of three rounded rects. They cost the same as any other
-- subtree, they theme themselves, and they are easy to copy into an app and
-- change when the design calls for something the library did not anticipate.

function mre.checkbox(props)
    local source = props.value or props.checked
    local read = type(source) == "function" and source or function() return source end
    local checked = read()

    local box_children = {}
    if checked then
        box_children[1] = mre.icon { kind = "check", size = 12, color = "#FFFFFF" }
    end

    return mre.row {
        key = props.key,
        gap = 8,
        align = "center",
        height = props.height or 24,
        interactive = true,
        on_click = props.on_change and function() props.on_change(not checked) end or nil,
        mre.row {
            width = 18, height = 18, radius = 4,
            align = "center", justify = "center",
            background = checked and "#2563EB" or "#FFFFFF",
            border = checked and "#2563EB" or "#9CA3AF",
            table.unpack(box_children),
        },
        mre.text { props.label or "", size = props.size or 14 },
    }
end

function mre.slider(props)
    local min = props.min or 0
    local max = props.max or 1
    local width = props.width or 200
    local height = props.height or 24
    local value = props.value
    local read = type(value) == "function" and value or function() return value or min end

    local span = math.max(1e-6, max - min)
    local t = math.max(0, math.min(1, (read() - min) / span))

    local function set(local_x)
        local next_t = math.max(0, math.min(1, local_x / width))
        if props.on_change then
            props.on_change(min + next_t * span)
        end
    end

    return mre.stack {
        key = props.key,
        width = width, height = height,
        interactive = true,
        on_click = function(x) set(x) end,
        on_drag = function(x) set(x) end,
        mre.row {
            absolute = true, top = height / 2 - 2, left = 0,
            width = width, height = 4, radius = 2, background = "#E5E7EB",
        },
        mre.row {
            absolute = true, top = height / 2 - 2, left = 0,
            width = t * width, height = 4, radius = 2, background = "#2563EB",
        },
        mre.row {
            absolute = true, top = height / 2 - 8,
            left = math.max(0, t * width - 8),
            width = 16, height = 16, radius = 8,
            background = "#FFFFFF", border = "#2563EB",
        },
    }
end

function mre.progress(props)
    local width = props.width or 160
    local height = props.height or 6
    local value = props.value
    if type(value) == "function" then value = value() end
    local t = math.max(0, math.min(1, value or 0))

    return mre.stack {
        width = width, height = height, radius = height / 2,
        background = props.track or "#E5E7EB",
        mre.row {
            absolute = true, top = 0, left = 0,
            width = t * width, height = height, radius = height / 2,
            background = props.color or "#2563EB",
        },
    }
end

function mre.toggle(props)
    local source = props.value
    local read = type(source) == "function" and source or function() return source end
    local on = read() and true or false

    return mre.stack {
        key = props.key,
        width = 36, height = 20, radius = 10,
        interactive = true,
        background = on and "#2563EB" or "#CBD5E1",
        on_click = props.on_change and function() props.on_change(not on) end or nil,
        mre.row {
            absolute = true, top = 2, left = on and 18 or 2,
            width = 16, height = 16, radius = 8, background = "#FFFFFF",
        },
    }
end

function mre.textfield(props)
    local value = props.value
    if type(value) == "function" then value = value() end

    return mre.text_input {
        key = props.key,
        value = value or "",
        placeholder = props.placeholder or "",
        password = props.password or false,
        size = props.size or 14,
        width = props.width or 200,
        height = props.height or 34,
        padding = { left = 10, right = 10 },
        radius = 6,
        background = props.background or "#FFFFFF",
        border = props.border or "#D1D5DB",
        on_change = props.on_change,
        on_submit = props.on_submit,
    }
end

function mre.divider(props)
    return mre.row {
        width = (props and props.width) or "100%",
        height = 1,
        background = (props and props.color) or "#E5E7EB",
    }
end

-- ---------------------------------------------------------------------------
-- Derived state
-- ---------------------------------------------------------------------------
--
-- A memo is a computation whose result is cached in a signal. The effect
-- recomputes it when its dependencies change; readers subscribe to the cache
-- signal, so they are invalidated only if the value actually changed.
--
-- This is eager rather than lazy: the value is computed on write, not on read.
-- That is the right trade for UI, where a memo is almost always read in the same
-- frame it is invalidated.

function mre.memo(fn)
    assert(type(fn) == "function", "mre.memo expects a function")
    local cached = mre.signal(nil)
    cached(fn()) -- seed immediately so the first read is already valid
    mre.effect(function()
        cached(fn())
    end)
    return function()
        return cached()
    end
end

mre.derived = mre.memo

-- ---------------------------------------------------------------------------
-- Mounting
-- ---------------------------------------------------------------------------
--
-- `mount` is an effect, which is what makes the whole thing reactive: the body
-- is tracked, so writing any signal it read schedules exactly one re-run on the
-- next flush. Nothing re-renders while idle.

mre.mount = function(view)
    assert(type(view) == "function", "mre.mount expects a function")
    mre.__tree = nil
    mre.effect(function()
        mre.__tree = view()
    end)
    return mre.__tree
end

-- Alias used by multi-window apps: the view is the app.
mre.app = function(config)
    local view = config and config.view
    assert(type(view) == "function", "mre.app requires a view function")
    mre.__app = config
    return mre.mount(view)
end

-- ---------------------------------------------------------------------------
-- Ergonomics
-- ---------------------------------------------------------------------------

-- ---------------------------------------------------------------------------
-- Animation
-- ---------------------------------------------------------------------------
--
-- A spring, not a duration. `mre.animate(signal)` returns a callable handle:
--
--   local open = mre.signal(0)
--   local animate = mre.animate(open)
--   animate(1)          -- spring towards 1
--   animate.to(0.5)     -- retarget mid-flight, keeping velocity
--
-- Springs are stepped in Rust, so a frame with no running animation does no
-- work at all.

function mre.animate(signal, config)
    assert(type(signal) == "function", "mre.animate expects a signal")
    return mre.__spring(signal, config)
end

-- Reads a signal without subscribing the current computation.
function mre.peek(read)
    local value
    mre.untracked(function()
        value = read()
    end)
    return value
end
