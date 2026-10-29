-- A counter app written entirely in Lua.
--
-- Run it headless with:
--     cargo run -p mre-lua --example headless -- examples/counter.lua
--
-- The point of the example is not the counter. It is that the three functions
-- below (`increment`, `reset`, `report`) are the whole application, and the
-- engine re-runs only the view when `count` changes.

local count = mre.signal(0)

-- Derived state: the label is computed once and cached until `count` changes.
local label = mre.memo(function()
    local n = count()
    if n == 0 then
        return "Not clicked yet"
    elseif n == 1 then
        return "Clicked once"
    end
    return "Clicked " .. n .. " times"
end)

-- An effect is side-effectful work that tracks what it reads. This one is only
-- here to prove the engine re-runs it exactly once per change.
mre.__effect_runs = 0
mre.effect(function()
    label()
    mre.__effect_runs = mre.__effect_runs + 1
end)

-- Actions are plain Lua functions; the widget surface just calls them.
mre.__increment = function() count(count() + 1) end
mre.__reset = function() count(0) end

mre.mount(function()
    return mre.column {
        padding = 24,
        gap = 12,

        mre.text { "MRE counter", size = 22, weight = "bold" },
        mre.text { label() },
        mre.row {
            gap = 8,
            mre.button { label = "Increment", on_click = mre.__increment },
            mre.button { label = "Reset", on_click = mre.__reset },
        },
    }
end)
