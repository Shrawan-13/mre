-- MRE framework overlay — loaded at runtime, not compiled into the host.
--
-- Edit this file (or `mre.load` another module) and rerun the app: colours,
-- spacing, radii and widget composition change with no Rust rebuild.

local tokens = {}
local rules = {}

function mre.tokens(values)
    for key, value in pairs(values) do
        tokens[key] = value
    end
end

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
            -- State selectors are opt-in hooks; they never block a rule.
        elseif part ~= kind then
            return false
        end
    end
    return true
end

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

-- Add your own theme or widget overrides below. `mre dev` reloads this file
-- without rebuilding the APK.

return true
