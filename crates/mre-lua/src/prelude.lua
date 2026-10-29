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

-- Reads a signal without subscribing the current computation.
function mre.peek(read)
    local value
    mre.untracked(function()
        value = read()
    end)
    return value
end
