-- MRE navigation — runtime-loaded, portable, Lua-only.
--
-- This module owns screens, stacks, tabs and modals. It knows nothing about
-- Android, Windows, iOS or the browser: the host maps its native back event to
-- `App::back()`, which asks Lua through `mre.__back()`. Navigation state is an
-- encoded string in `mre.persist`, so it survives hot reload and app restarts.
--
--   local nav = mre.nav {
--     routes = { home = Home, detail = { view = Detail, title = "Detail" } },
--     initial = "home",
--   }
--   mre.app { view = function() return nav:view() end }
--
-- A screen receives `{ nav, params, route, id }`. Route keys are derived from a
-- persisted id, so pop-back restores scroll offsets and editor state for free.

local mre = ...
local NAV_VERSION = 1
local BLOB_PREFIX = "MRE-NAV-1;"
local MAX_DEPTH = 16
local MAX_COUNT = 4096
local MAX_BLOB = 1024 * 1024

local function fail(message)
    error(message, 0)
end

-- ---------------------------------------------------------------------------
-- Deterministic, bounded value codec.
--
-- Values are tagged and length-prefixed, so strings may contain any byte and
-- maps are sorted before encoding. Only scalars and tables are supported,
-- which matches the scalar-only contract of `mre.persist`.
-- ---------------------------------------------------------------------------

local function encode_scalar(value, out)
    local kind = type(value)
    if kind == "nil" then
        out[#out + 1] = "N;"
    elseif kind == "boolean" then
        out[#out + 1] = value and "T;" or "F;"
    elseif kind == "number" then
        if math.type(value) == "integer" then
            out[#out + 1] = "I" .. tostring(value) .. ";"
        elseif value ~= value then
            out[#out + 1] = "Dnan;"
        elseif value == math.huge then
            out[#out + 1] = "Dinf;"
        elseif value == -math.huge then
            out[#out + 1] = "D-inf;"
        else
            out[#out + 1] = "D" .. string.format("%.17g", value) .. ";"
        end
    elseif kind == "string" then
        out[#out + 1] = "S" .. #value .. ":" .. value
    else
        fail("nav state cannot store a " .. kind)
    end
end

local function encode_value(value, out, depth, seen)
    if depth > MAX_DEPTH then fail("nav state is too deep") end
    if type(value) ~= "table" then
        encode_scalar(value, out)
        return
    end
    if seen[value] then fail("nav state contains a cycle") end
    seen[value] = true

    local array = 0
    while value[array + 1] ~= nil do
        array = array + 1
        if array > MAX_COUNT then fail("nav array is too large") end
    end

    local keys = {}
    for key in pairs(value) do
        local kind = type(key)
        if kind ~= "string" and kind ~= "number" then
            fail("nav map keys must be strings or numbers")
        end
        local dense = kind == "number" and key >= 1 and key <= array
            and math.floor(key) == key
        if not dense then keys[#keys + 1] = key end
    end
    table.sort(keys, function(a, b)
        if type(a) ~= type(b) then return type(a) < type(b) end
        return tostring(a) < tostring(b)
    end)
    if #keys > MAX_COUNT then fail("nav map is too large") end

    out[#out + 1] = "A" .. array .. ";"
    for index = 1, array do
        encode_value(value[index], out, depth + 1, seen)
    end
    out[#out + 1] = "M" .. #keys .. ";"
    for _, key in ipairs(keys) do
        encode_scalar(key, out)
        encode_value(value[key], out, depth + 1, seen)
    end
    seen[value] = nil
end

local function encode(value)
    local out = { BLOB_PREFIX }
    encode_value(value, out, 0, {})
    local blob = table.concat(out)
    if #blob > MAX_BLOB then fail("nav state exceeds " .. MAX_BLOB .. " bytes") end
    return blob
end

local function read_to(blob, pos, needle)
    local at = blob:find(needle, pos, true)
    if not at then fail("truncated nav state") end
    return blob:sub(pos, at - 1), at + 1
end

local function read_count(blob, pos)
    local digits, next_pos = read_to(blob, pos, ";")
    local count = tonumber(digits)
    if not count or count < 0 or count ~= math.floor(count) then
        fail("invalid nav count")
    end
    return count, next_pos
end

local function decode_value(blob, pos, depth, allow_table)
    if depth > MAX_DEPTH then fail("nav state is too deep") end
    local tag = blob:sub(pos, pos)
    pos = pos + 1

    if tag == "N" then
        if blob:sub(pos, pos) ~= ";" then fail("bad nil") end
        return nil, pos + 1
    elseif tag == "T" or tag == "F" then
        if blob:sub(pos, pos) ~= ";" then fail("bad boolean") end
        return tag == "T", pos + 1
    elseif tag == "I" then
        local digits, next_pos = read_to(blob, pos, ";")
        local number = tonumber(digits)
        if not number or number ~= math.floor(number) then fail("bad integer") end
        return number, next_pos
    elseif tag == "D" then
        local digits, next_pos = read_to(blob, pos, ";")
        if digits == "nan" then return 0 / 0, next_pos end
        if digits == "inf" then return math.huge, next_pos end
        if digits == "-inf" then return -math.huge, next_pos end
        local number = tonumber(digits)
        if not number then fail("bad number") end
        return number, next_pos
    elseif tag == "S" then
        local digits, next_pos = read_to(blob, pos, ":")
        local length = tonumber(digits)
        if not length or length < 0 or length ~= math.floor(length) then
            fail("bad string length")
        end
        if length > MAX_BLOB then fail("nav string is too large") end
        local text = blob:sub(next_pos, next_pos + length - 1)
        if #text ~= length then fail("truncated string") end
        return text, next_pos + length
    elseif tag == "A" then
        -- A table is encoded as one A section followed by one M section. The
        -- decoder must consume both before returning, or the map part looks
        -- like trailing data at the top level.
        if not allow_table then fail("nav keys must be scalars") end
        local count, next_pos = read_count(blob, pos)
        local list = {}
        for index = 1, count do
            local value, after = decode_value(blob, next_pos, depth + 1, true)
            list[index] = value
            next_pos = after
        end

        if blob:sub(next_pos, next_pos) ~= "M" then fail("missing nav map section") end
        local map_count, after_map = read_count(blob, next_pos + 1)
        for _ = 1, map_count do
            local key, after_key = decode_value(blob, after_map, depth + 1, false)
            if key == nil then fail("nil nav key") end
            local value, after_value = decode_value(blob, after_key, depth + 1, true)
            list[key] = value
            after_map = after_value
        end
        return list, after_map
    end
    fail("unknown nav tag: " .. tag)
end

local function decode(blob)
    if type(blob) ~= "string" or blob:sub(1, #BLOB_PREFIX) ~= BLOB_PREFIX then
        return nil
    end
    local ok, value = pcall(function()
        local result, pos = decode_value(blob, #BLOB_PREFIX + 1, 0, true)
        if pos ~= #blob + 1 then fail("trailing nav data") end
        return result
    end)
    if ok then return value end
    mre.warn("nav decode failed: " .. tostring(value) .. " blob=" .. blob)
    return nil
end

local function clone(value)
    local ok, blob = pcall(encode, value)
    if not ok then return nil, blob end
    local decoded = decode(blob)
    if decoded == nil then return nil, "decode failed" end
    return decoded
end

-- ---------------------------------------------------------------------------
-- Route and state normalisation.
-- ---------------------------------------------------------------------------

local function sorted_keys(table_value)
    local keys = {}
    for key in pairs(table_value) do keys[#keys + 1] = key end
    table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
    return keys
end

local function normalise_routes(spec, transition)
    local routes = {}
    for name, definition in pairs(spec or {}) do
        if type(name) ~= "string" then fail("route names must be strings") end
        if type(definition) == "function" then
            routes[name] = {
                view = definition,
                title = name,
                transition = transition or "push",
            }
        elseif type(definition) == "table" and type(definition.view) == "function" then
            routes[name] = {
                view = definition.view,
                title = definition.title or name,
                transition = definition.transition or transition or "push",
                on_enter = definition.on_enter,
                on_exit = definition.on_exit,
            }
        else
            fail("route " .. name .. " needs a view function")
        end
    end
    return routes
end

local function normalise_entry(routes, spec, id)
    local name = type(spec) == "string" and spec or spec and spec.name
    if not routes[name] then fail("unknown route: " .. tostring(name)) end
    local params = type(spec) == "table" and (spec.params or {}) or {}
    local copied, problem = clone(params)
    if not copied then fail("bad route params: " .. tostring(problem)) end
    return { name = name, id = id, params = copied }
end

local function normalise_tabs(spec, initial)
    if spec == nil then
        return { { id = "root", title = "App", initial = initial } }
    end
    local tabs = {}
    for index, item in ipairs(spec) do
        local tab = type(item) == "string"
            and { id = item, title = item, initial = initial }
            or item
        if type(tab) ~= "table" then fail("bad tab at index " .. index) end
        tabs[index] = {
            id = tostring(tab.id or index),
            title = tab.title or tostring(tab.id or index),
            initial = tab.initial or initial,
        }
    end
    return tabs
end

local function valid_entry(entry, routes)
    return type(entry) == "table"
        and type(entry.name) == "string" and routes[entry.name] ~= nil
        and type(entry.id) == "number" and entry.id >= 1
        and type(entry.params) == "table"
end

local function valid_state(state, routes, tabs)
    if type(state) ~= "table" or state.v ~= NAV_VERSION then return false end
    if type(state.next) ~= "number" or state.next < 1 then return false end
    if type(state.tabs) ~= "table" or #state.tabs ~= #tabs then return false end
    if type(state.active) ~= "number" then return false end
    local active = math.floor(state.active)
    if active < 1 or active > #tabs then return false end
    for index, tab in ipairs(state.tabs) do
        if type(tab) ~= "table" or tab.id ~= tabs[index].id then return false end
        if type(tab.stack) ~= "table" or #tab.stack < 1 then return false end
        for _, entry in ipairs(tab.stack) do
            if not valid_entry(entry, routes) then return false end
        end
    end
    if type(state.modals) ~= "table" then return false end
    for _, entry in ipairs(state.modals) do
        if not valid_entry(entry, routes) then return false end
    end
    return true
end

local function build_initial(config, routes, tabs)
    local first = config.initial
    if not first then
        local names = sorted_keys(routes)
        first = names[1]
    end
    local state = {
        v = NAV_VERSION,
        next = 1,
        active = math.floor(config.active_tab or 1),
        tabs = {},
        modals = {},
    }
    for index, tab in ipairs(tabs) do
        state.tabs[index] = {
            id = tab.id,
            stack = { normalise_entry(routes, tab.initial or first, state.next) },
        }
        state.next = state.next + 1
    end
    if state.active < 1 or state.active > #tabs then state.active = 1 end
    return state
end

-- ---------------------------------------------------------------------------
-- Navigator.
-- ---------------------------------------------------------------------------

local Nav = {}
Nav.__index = Nav

local function context(nav, entry)
    return {
        nav = nav,
        params = clone(entry.params) or {},
        route = entry.name,
        id = entry.id,
    }
end

local function screen_key(entry)
    return "nav-screen-" .. tostring(entry.id)
end

local function render_entry(nav, entry, options)
    local definition = nav.routes[entry.name]
    local view = definition.view(context(nav, entry))
    local props = {
        key = options.key,
        grow = 1,
        opacity = options.opacity,
        animate = options.animate,
    }
    return mre.stack(props, view)
end

function Nav:save()
    local ok, blob = pcall(encode, self._state)
    if ok then
        self.saved(blob)
    else
        mre.warn("navigation state was not persisted: " .. tostring(blob))
    end
    self.revision(self.revision() + 1)
end

function Nav:transition(kind, outgoing, incoming)
    self.transitioning = nil
    if self.transition_ms <= 0 or not incoming then return end
    local token = self.next_token
    self.next_token = token + 1
    local spring = self.spring
    local fade = mre.spring(kind == "push" and 0 or 1, spring)
    local slide = mre.spring(kind == "push" and 24 or -12, spring)
    fade(1)
    slide(0)
    local out_fade, out_slide
    if outgoing then
        out_fade = mre.spring(1, spring)
        out_slide = mre.spring(0, spring)
        out_fade(0)
        out_slide(kind == "push" and -12 or 24)
    end
    self.transitioning = {
        token = token,
        incoming = incoming.id,
        outgoing = outgoing,
        fade = fade,
        slide = slide,
        out_fade = out_fade,
        out_slide = out_slide,
    }
    -- The springs are stepped by the engine. This single timer removes the
    -- outgoing screen afterwards; no Lua runs per frame.
    mre.after(self.transition_ms + 40, function()
        if self.transitioning and self.transitioning.token == token then
            self.transitioning = nil
            self.revision(self.revision() + 1)
        end
    end)
end

function Nav:active_stack()
    return self._state.tabs[self._state.active].stack
end

function Nav:clear_transition()
    self.transitioning = nil
end

function Nav:push(name, params)
    local entry = normalise_entry(self.routes, { name = name, params = params }, self._state.next)
    self._state.next = self._state.next + 1
    local stack = self:active_stack()
    local previous = stack[#stack]
    stack[#stack + 1] = entry
    local definition = self.routes[name]
    if definition.on_enter then
        local ok, problem = pcall(definition.on_enter, context(self, entry))
        if not ok then mre.warn("on_enter failed: " .. tostring(problem)) end
    end
    self:transition("push", nil, entry)
    self:save()
    return entry
end

function Nav:replace(name, params)
    local stack = self:active_stack()
    local previous = table.remove(stack)
    if previous then
        local definition = self.routes[previous.name]
        if definition.on_exit then
            local ok, problem = pcall(definition.on_exit, context(self, previous))
            if not ok then mre.warn("on_exit failed: " .. tostring(problem)) end
        end
    end
    local entry = normalise_entry(self.routes, { name = name, params = params }, self._state.next)
    self._state.next = self._state.next + 1
    stack[#stack + 1] = entry
    self:transition("push", previous, entry)
    self:save()
    return entry
end

function Nav:pop()
    local stack = self:active_stack()
    if #stack <= 1 then return false end
    local outgoing = table.remove(stack)
    local definition = self.routes[outgoing.name]
    if definition.on_exit then
        local ok, problem = pcall(definition.on_exit, context(self, outgoing))
        if not ok then mre.warn("on_exit failed: " .. tostring(problem)) end
    end
    self:transition("pop", outgoing, stack[#stack])
    self:save()
    return true
end

function Nav:pop_to(name)
    if not self.routes[name] then return false end
    local stack = self:active_stack()
    for index = #stack, 1, -1 do
        if stack[index].name == name then
            if index == #stack and #stack == 1 then return true end
            local outgoing = stack[#stack]
            while #stack > index do table.remove(stack) end
            self:transition("pop", outgoing, stack[#stack])
            self:save()
            return true
        end
    end
    return false
end

function Nav:reset(name, params)
    local stack = self:active_stack()
    local previous = stack[#stack]
    local fallback = stack[#stack] and stack[#stack].name or nil
    local entry = normalise_entry(self.routes, { name = name or fallback, params = params }, self._state.next)
    self._state.next = self._state.next + 1
    self._state.tabs[self._state.active].stack = { entry }
    self:transition("push", previous, entry)
    self:save()
    return entry
end

function Nav:open_modal(name, params)
    local entry = normalise_entry(self.routes, { name = name, params = params }, self._state.next)
    self._state.next = self._state.next + 1
    self._state.modals[#self._state.modals + 1] = entry
    self:save()
    return entry
end

function Nav:dismiss_modal()
    if #self._state.modals == 0 then return false end
    table.remove(self._state.modals)
    self:save()
    return true
end

function Nav:tab_index(id_or_index)
    if type(id_or_index) == "number" then
        local index = math.floor(id_or_index)
        return index >= 1 and index <= #self._state.tabs and index or nil
    end
    for index, tab in ipairs(self._state.tabs) do
        if tab.id == id_or_index then return index end
    end
    return nil
end

function Nav:set_tab(id_or_index)
    local index = self:tab_index(id_or_index)
    if not index then return false end
    self._state.active = index
    self:clear_transition()
    self:save()
    return true
end

function Nav:tab()
    return self._state.tabs[self._state.active].id
end

function Nav:open(path, params)
    local route = tostring(path or ""):gsub("^#/?", ""):gsub("^/", "")
    local tab_id, name = route:match("^([^/]+)/(.+)$")
    if tab_id and self:tab_index(tab_id) then
        self:set_tab(tab_id)
    else
        name = route
    end
    if not self.routes[name] then
        mre.warn("unknown navigation path: " .. tostring(path))
        return false
    end
    self:push(name, params)
    return true
end

function Nav:top()
    return self:active_stack()[#self:active_stack()]
end

function Nav:title()
    local entry = self:top()
    local definition = self.routes[entry.name]
    local title = definition.title
    if type(title) == "function" then title = title(context(self, entry)) end
    return title or entry.name
end

function Nav:state()
    return clone(self._state) or {}
end

function Nav:can_back()
    return #self._state.modals > 0 or #self:active_stack() > 1
end

function Nav:back()
    if #self._state.modals > 0 then return self:dismiss_modal() end
    return self:pop()
end

function Nav:back_button(props)
    local options = {}
    for key, value in pairs(props or {}) do options[key] = value end
    options[1] = options[1] or "Back"
    if self:can_back() then
        options.on_click = function() self:back() end
    else
        options.opacity = 0.35
        options.on_click = function() end
    end
    return mre.button(options)
end

function Nav:destroy()
    if self.off_back then self.off_back() end
end

function Nav:view()
    -- Both reads subscribe the mounted view to navigation changes. Writing
    -- either signal in `save` schedules exactly one rebuild per navigation.
    self.saved()
    self.revision()

    local children = {}
    local stack = self:active_stack()
    local active = self.transitioning

    for _, entry in ipairs(stack) do
        local options = {
            key = screen_key(entry),
            opacity = 1,
        }
        if entry ~= stack[#stack] then options.opacity = 0 end
        if active and entry.id == active.incoming then
            options.animate = { opacity = active.fade, x = active.slide }
        end
        children[#children + 1] = render_entry(self, entry, options)
    end

    if active and active.outgoing then
        children[#children + 1] = render_entry(self, active.outgoing, {
            key = screen_key(active.outgoing),
            opacity = 1,
            animate = { opacity = active.out_fade, x = active.out_slide },
        })
    end

    -- Overlays come last so a dialog covers safe-area bands, as the prelude
    -- documents. They are keyed by id, so dismissing one keeps the others.
    for _, entry in ipairs(self._state.modals) do
        children[#children + 1] = render_entry(self, entry, {
            key = "nav-modal-" .. tostring(entry.id),
            opacity = 1,
        })
    end

    return mre.stack({ key = "nav-root", grow = 1 }, table.unpack(children))
end

function mre.nav(config)
    config = config or {}
    local routes = normalise_routes(config.routes, config.transition)
    local tabs = normalise_tabs(config.tabs, config.initial)
    local initial = build_initial(config, routes, tabs)
    local blob = mre.persist(config.persist or "mre.nav", encode(initial))
    local restored = decode(blob())
    local state = valid_state(restored, routes, tabs) and restored or initial

    local nav = setmetatable({
        routes = routes,
        tabs = tabs,
        _state = state,
        saved = blob,
        revision = mre.signal(0),
        transition_ms = config.transition_ms or 240,
        spring = config.spring or { stiffness = 260, damping = 26 },
        transitioning = nil,
        next_token = 1,
    }, Nav)

    nav.off_back = mre.on_back(function() return nav:back() end, 20)
    return nav
end

mre.router = mre.nav

return true
