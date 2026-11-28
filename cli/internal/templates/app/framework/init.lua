-- MRE framework overlay — loaded at runtime, not compiled into the host.
--
-- Edit this file (or `mre.load` another module) and rerun the app: colours,
-- spacing, radii and widget composition change with no Rust rebuild.
--
-- A tiny CSS-like layer:
--
--   mre.tokens { accent = "#7C3AED", radius = 12 }
--   mre.sheet {
--     ["button"]         = { radius = "var(radius)", background = "var(accent)" },
--     [".danger"]        = { background = "#DC2626", color = "#FFFFFF" },
--     ["#save"]          = { width = 160 },
--   }
--   mre.button { "Save", class = "danger", key = "save" }

local tokens = {}
local rules = {}

function mre.tokens(values)
    for key, value in pairs(values) do
        tokens[key] = value
    end
end

-- `mre.use` is the friendly alias for a whole theme.
mre.use = mre.tokens

function mre.sheet(spec)
    for selector, props in pairs(spec) do
        rules[#rules + 1] = { selector = selector, props = props }
    end
end

local function resolve(value)
    if type(value) ~= "string" then return value end
    local name = value:match("^var%(([%w_%-]+)%)$")
    if name then return tokens[name] end
    return value
end

local function matches(selector, kind, node)
    for part in selector:gmatch("[^%s]+") do
        if part:sub(1, 1) == "." then
            local wanted = part:sub(2)
            local found = false
            for class in tostring(node.class or ""):gmatch("[^%s]+") do
                if class == wanted then found = true end
            end
            if not found then return false end
        elseif part:sub(1, 1) == "#" then
            if node.key ~= part:sub(2) then return false end
        elseif part:sub(1, 1) == ":" then
            -- State selectors are opt-in hooks for apps that publish hover
            -- signals; they never block a rule by themselves.
        elseif part ~= kind then
            return false
        end
    end
    return true
end

-- Called by `node()` in the embedded prelude for every vnode.
function mre.__apply_style(kind, node)
    if #rules == 0 then return nil end
    local out = {}
    for key, value in pairs(node) do out[key] = value end
    for _, rule in ipairs(rules) do
        if matches(rule.selector, kind, node) then
            for key, value in pairs(rule.props) do
                if out[key] == nil then out[key] = resolve(value) end
            end
        end
    end
    return out
end

-- Navigation is part of the runtime framework, not the app bundle. Loading it
-- here means every app gets `mre.nav` / `mre.router` without a Rust rebuild.
mre.load("framework/nav")

return true
