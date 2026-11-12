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
local scroll_node = function(props) return node("scroll", props) end
mre.scroll = function(props)
    -- A plain scroll container: clip, and route the wheel to `on_scroll`. The
    -- caller owns the offset because only it knows how tall the content is.
    if props.content == nil or props.offset == nil then
        return scroll_node(props)
    end

    -- A scroll view with a visible thumb. `content` is the total content height
    -- and `offset` is the current scroll position, which is what lets the thumb
    -- be drawn in the same frame as the content it describes.
    local height = props.height or 400
    local content = props.content
    if type(content) == "function" then content = content() end
    local offset = props.offset
    if type(offset) == "function" then offset = offset() end

    local max_offset = math.max(0, content - height)
    local t = max_offset > 0 and math.min(1, math.max(0, offset / max_offset)) or 0
    local track = math.max(0, height - 8)
    local thumb = math.min(math.max(28, height * height / math.max(content, 1)), track)
    local thumb_y = 4 + t * math.max(0, track - thumb)

    local children = {}
    for index, child in ipairs(props) do children[index] = child end
    local count = #children
    if max_offset > 0 then
        children[count + 1] = mre.row {
            absolute = true, top = 4, right = 6,
            width = 4, height = track, radius = 2,
            background = props.track_color or "#E5E7EB",
        }
        children[count + 2] = mre.row {
            absolute = true, top = thumb_y, right = 6,
            width = 4, height = thumb, radius = 2,
            background = props.thumb_color or "#9CA3AF",
        }
    end

    local forwarded = {}
    for key, value in pairs(props) do forwarded[key] = value end
    forwarded[1] = nil
    forwarded.content = nil
    forwarded.offset = nil
    forwarded.track_color = nil
    forwarded.thumb_color = nil
    for index = 1, #children do forwarded[index] = children[index] end

    return scroll_node(forwarded)
end
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
        role = "checkbox",
        label = props.label,
        gap = 8,
        align = "center",
        height = props.height or 24,
        interactive = true,
        focusable = true,
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
        role = "slider",
        label = props.label,
        width = width, height = height,
        interactive = true,
        focusable = false,
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

    -- A percentage width is filled as a percentage of the track, so a
    -- progress bar can span a card without knowing its width in pixels.
    local fill_width = width
    if type(width) == "string" then
        fill_width = string.format("%.4f%%", t * 100)
    else
        fill_width = t * width
    end

    return mre.stack {
        role = "progress",
        label = props.label,
        width = width, height = height, radius = height / 2,
        background = props.track or "#E5E7EB",
        mre.row {
            absolute = true, top = 0, left = 0,
            width = fill_width, height = height, radius = height / 2,
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
        role = "switch",
        label = props.label,
        width = 36, height = 20, radius = 10,
        interactive = true,
        focusable = true,
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
        role = "textfield",
        label = props.label,
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
        -- The colour of the text, and of the caret, which is drawn in it.
        -- Dropping it left the caret in the theme's text colour, which is
        -- invisible on a dark field.
        color = props.color,
        grow = props.grow,
        align = props.align,
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

--- A surface: padded, rounded, optionally bordered.
--
-- `mre.card { mre.text { "Title" }, mre.text { "Body" } }`
function mre.card(props)
    props = props or {}
    local children = {}
    for index, child in ipairs(props) do children[index] = child end
    return mre.column {
        key = props.key,
        role = props.role,
        label = props.label,
        width = props.width,
        height = props.height,
        padding = props.padding or { left = 16, right = 16, top = 16, bottom = 16 },
        gap = props.gap or 0,
        radius = props.radius or 10,
        background = props.background or "#FFFFFF",
        border = props.border or "#E5E7EB",
        table.unpack(children),
    }
end

--- A small pill, for counts and status.
function mre.badge(props)
    props = props or {}
    local text = props.text or props.label or ""
    return mre.row {
        key = props.key,
        role = "text",
        label = props.label or text,
        padding = { left = 8, right = 8, top = 2, bottom = 2 },
        radius = 999,
        background = props.background or "#E0ECFF",
        mre.text { text, size = props.size or 12, color = props.color or "#1D4ED8" },
    }
end

--- An icon-only button.
function mre.icon_button(props)
    props = props or {}
    return mre.row {
        key = props.key,
        role = "button",
        label = props.label,
        width = props.size or 30,
        height = props.size or 30,
        radius = props.radius or 6,
        align = "center",
        justify = "center",
        interactive = true,
        background = props.background or "transparent",
        on_click = props.on_click,
        mre.icon {
            kind = props.kind or "chevron-right",
            size = (props.size or 30) - 12,
            color = props.color or "#374151",
        },
    }
end

--- A strip of tabs. `items` is a list of strings, or of tables with a `title`.
--
-- The active tab is highlighted; `on_change(index, item)` is called on click.
function mre.tabs(props)
    props = props or {}
    local items = props.items or {}
    local active = props.active or 1
    local on_change = props.on_change
    local children = {}
    for index, item in ipairs(items) do
        local title = item
        if type(item) == "table" then title = item.title or item.label or "" end
        local selected = index == active
        children[index] = mre.button {
            title,
            role = "tab",
            padding = { left = 12, right = 12, top = 8, bottom = 8 },
            radius = props.radius or 6,
            background = selected and (props.selected_background or "#E0ECFF")
                or "transparent",
            color = selected and (props.selected_color or "#1D4ED8")
                or (props.color or "#4B5563"),
            on_click = on_change and function() on_change(index, item) end or nil,
        }
    end
    return mre.row {
        key = props.key,
        role = "tablist",
        gap = props.gap or 4,
        table.unpack(children),
    }
end

--- A combo box. The menu expands in place rather than floating, so it never
--- needs an overlay and is never clipped by a scroll view.
--
-- `options` is a list of values, or of tables with `value` and `label`.
function mre.select(props)
    props = props or {}
    local options = props.options or {}
    local value = props.value
    if type(value) == "function" then value = value() end
    local open = props.open
    if type(open) == "function" then open = open() end
    local on_change = props.on_change
    local on_toggle = props.on_toggle

    local function option_parts(option)
        if type(option) == "table" then
            return option.value, option.label or option.title or tostring(option.value)
        end
        return option, tostring(option)
    end

    local label = props.placeholder or "Select"
    for _, option in ipairs(options) do
        local option_value, option_label = option_parts(option)
        if option_value == value then label = option_label end
    end

    local children = {}
    children[1] = mre.row {
        height = props.height or 34,
        padding = { left = 10, right = 10 },
        gap = 8,
        align = "center",
        radius = 6,
        background = props.background or "#FFFFFF",
        border = props.border or "#D1D5DB",
        interactive = true,
        on_click = on_toggle and function() on_toggle(not open) end or nil,
        mre.text { label, size = props.size or 14, grow = 1 },
        mre.icon { kind = "chevron-down", size = 14, color = "#6B7280" },
    }

    if open then
        local items = {}
        for index, option in ipairs(options) do
            local option_value, option_label = option_parts(option)
            local selected = option_value == value
            items[index] = mre.button {
                option_label,
                height = 30,
                padding = { left = 10, right = 10 },
                background = selected and "#EFF6FF" or "transparent",
                color = selected and "#1D4ED8" or "#111827",
                on_click = on_change and function()
                    on_change(option_value, option)
                    if on_toggle then on_toggle(false) end
                end or nil,
            }
        end
        children[2] = mre.column {
            width = "100%",
            margin = { top = 4 },
            radius = 6,
            background = props.background or "#FFFFFF",
            border = props.border or "#D1D5DB",
            table.unpack(items),
        }
    end

    return mre.column {
        key = props.key,
        role = "combobox",
        label = props.label,
        width = props.width or 200,
        table.unpack(children),
    }
end

--- A modal dialog: a scrim over the app, with a card in the middle.
--
-- Put the dialog last in the root, so it paints over everything else:
--
--   return mre.column { app_content, mre.dialog { open = show(), ... } }
function mre.dialog(props)
    props = props or {}
    if not props.open then
        -- A zero-sized node: it costs a slot in the tree and nothing else.
        return mre.column { width = 0, height = 0 }
    end
    local children = {}
    for index, child in ipairs(props) do children[index] = child end
    return mre.column {
        key = props.key,
        role = "dialog",
        label = props.label,
        absolute = true, top = 0, right = 0, bottom = 0, left = 0,
        align = "center",
        justify = "center",
        interactive = true,
        tint = false,
        background = props.scrim or "#00000066",
        on_click = props.on_dismiss and function() props.on_dismiss() end or nil,
        mre.column {
            width = props.width or 420,
            padding = { left = 20, right = 20, top = 20, bottom = 20 },
            gap = props.gap or 12,
            radius = 12,
            background = props.background or "#FFFFFF",
            border = props.border or "#E5E7EB",
            -- Swallow clicks so that pressing inside the card does not
            -- dismiss the dialog.
            on_click = function() end,
            table.unpack(children),
        },
    }
end

--- A tooltip. Position it with `absolute` inside the anchor's parent.
--
--   local over = mre.hover("save")
--   mre.column {
--     mre.button { key = "save", "Save" },
--     over() and mre.tooltip { text = "Saves the document", absolute = true, top = 36 } or mre.spacer { size = 0 },
--   }
function mre.tooltip(props)
    props = props or {}
    local text = props.text or props.label or ""
    return mre.row {
        key = props.key,
        role = "tooltip",
        label = text,
        absolute = props.absolute,
        top = props.top,
        right = props.right,
        bottom = props.bottom,
        left = props.left,
        padding = { left = 8, right = 8, top = 4, bottom = 4 },
        radius = 6,
        background = props.background or "#111827",
        mre.text { text, size = props.size or 12, color = props.color or "#FFFFFF", nowrap = true },
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

-- A spring that is not attached to a signal, for driving a property directly:
--
--   local fade = mre.spring(0)
--   fade(1)
--   mre.card { key = "card", animate = { opacity = fade } }
--
-- The runtime reads the spring and writes the property, so the view function
-- does not run again while it animates.
function mre.spring(source, config)
    return mre.__spring(source, config)
end

-- Reads a signal without subscribing the current computation.
function mre.peek(read)
    local value
    mre.untracked(function()
        value = read()
    end)
    return value
end

-- ---------------------------------------------------------------------------
-- Viewport
-- ---------------------------------------------------------------------------
--
-- The size of the surface, in logical pixels. Reading it inside a view
-- subscribes that view to it, so a resize or a rotation rebuilds the view:
-- an app can change its structure, not only its layout.
--
--   local size = mre.viewport()
--   if size.width < 640 then ... end
function mre.viewport()
  return mre.__viewport()
end

-- ---------------------------------------------------------------------------
-- Hover
-- ---------------------------------------------------------------------------
-- The engine knows which node the pointer is over. `mre.hover(key)` returns a
-- reader for that state, so a control can change itself when the pointer
-- arrives:
--
--   local over = mre.hover("save")
--   mre.button { "Save", background = over() and "#1D4ED8" or "#2563EB" }
--
-- The value is a signal, so reading it inside the view subscribes the view:
-- the frame after the pointer arrives rebuilds, and no other frame does.

local hover_readers = {}

function mre.hover(key)
    assert(type(key) == "string", "mre.hover expects a node key")
    local reader = hover_readers[key]
    if reader == nil then
        reader = mre.__hover(key)
        hover_readers[key] = reader
    end
    return reader
end
