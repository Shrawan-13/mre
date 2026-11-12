-- Focus — a task list, and a tour of what the engine can draw.
--
-- Everything here is Lua: signals, springs, hover tracking, dialogs, and a
-- viewport reader that rebuilds the view when the window changes. The engine
-- contributes layout, painting, text editing, animation and damage tracking.
--
--   cargo run -p mre-window --bin mre -- examples/todo.lua

-- The list lives in a plain table (signals hold scalars) and a version signal
-- marks it dirty whenever it changes.
local todos = {}
local version = mre.signal(0)
local draft = mre.signal("")
local filter = mre.signal("all")
local confirm_clear = mre.signal(false)
local next_id = 1

local function touch()
  version(version() + 1)
end

-- One palette, so the whole app can be re-skinned by editing eight lines.
local C = {
  bg = "#0A0F1E",
  surface = "#121A2E",
  raised = "#182240",
  border = "#24304D",
  track = "#1E2A47",
  text = "#E6ECFA",
  muted = "#8494B8",
  faint = "#5D6D92",
  accent = "#7C8CFF",
  accent_dim = "#1D2A4E",
  success = "#3DDC97",
  success_dim = "#11322A",
  danger = "#FF6B81",
}

-- ---------------------------------------------------------------------------
-- State
-- ---------------------------------------------------------------------------

local function add()
  local text = draft():match("^%s*(.-)%s*$")
  if text == "" then
    return
  end
  todos[#todos + 1] = { id = next_id, text = text, done = false, at = os.time() }
  next_id = next_id + 1
  draft("")
  touch()
end

local function toggle(id)
  for _, todo in ipairs(todos) do
    if todo.id == id then
      todo.done = not todo.done
    end
  end
  touch()
end

local function remove(id)
  for index = #todos, 1, -1 do
    if todos[index].id == id then
      table.remove(todos, index)
    end
  end
  touch()
end

local function clear_done()
  for index = #todos, 1, -1 do
    if todos[index].done then
      table.remove(todos, index)
    end
  end
  touch()
end

local function count_done()
  local count = 0
  for _, todo in ipairs(todos) do
    if todo.done then
      count = count + 1
    end
  end
  return count
end

local function count_active()
  return #todos - count_done()
end

local FILTERS = {
  { id = "all", title = "All" },
  { id = "active", title = "Active" },
  { id = "done", title = "Done" },
}

local function visible()
  local current = filter()
  local out = {}
  for _, todo in ipairs(todos) do
    if current == "all" or (current == "done" and todo.done) or (current == "active" and not todo.done) then
      out[#out + 1] = todo
    end
  end
  return out
end

local function filter_index()
  local current = filter()
  for index, item in ipairs(FILTERS) do
    if item.id == current then
      return index
    end
  end
  return 1
end

-- ---------------------------------------------------------------------------
-- Animation
-- ---------------------------------------------------------------------------
--
-- One spring per row, created once and reused: a view that allocated a spring
-- on every rebuild would restart the animation every time it ran. The spring
-- value is read by the engine, so no Lua runs while it moves.

local fades = {}

local function fade_for(id)
  local spring = fades[id]
  if spring == nil then
    spring = mre.spring(0.35, { stiffness = 260, damping = 24 })
    fades[id] = spring
  end
  return spring
end

-- ---------------------------------------------------------------------------
-- Widgets
-- ---------------------------------------------------------------------------

-- An icon button that can animate. The prelude's `mre.icon_button` composes
-- well, but it drops properties it does not know about, so this one is built
-- from the same parts with `animate` carried through.
local function icon_button(props)
  local size = props.size or 30
  return mre.row {
    key = props.key,
    role = "button",
    label = props.label,
    width = size,
    height = size,
    radius = props.radius or 999,
    align = "center",
    justify = "center",
    interactive = true,
    background = props.background or "transparent",
    animate = props.animate,
    on_click = props.on_click,
    mre.icon { kind = props.kind, size = size - 10, color = props.color or C.muted },
  }
end

local function stat(value, label, colour)
  return mre.column {
    gap = 2,
    mre.text { tostring(value), size = 24, weight = 700, color = colour },
    mre.text { label, size = 11, weight = 500, color = C.faint },
  }
end

local function todo_row(todo)
  local hovering = mre.hover("row-" .. todo.id)()
  local fade = fade_for(todo.id)
  fade(hovering and 1 or 0.35)

  return mre.row {
    key = "row-" .. todo.id,
    interactive = true,
    radius = 14,
    padding = { left = 14, right = 12, top = 13, bottom = 13 },
    gap = 14,
    align = "center",
    background = hovering and C.raised or C.surface,
    border = { color = hovering and C.accent_dim or C.border, width = 1 },
    -- The completion circle.
    mre.row {
      key = "todo-" .. todo.id,
      role = "button",
      width = 26,
      height = 26,
      radius = 999,
      align = "center",
      justify = "center",
      interactive = true,
      background = todo.done and C.success_dim or "transparent",
      border = { color = todo.done and C.success or C.faint, width = todo.done and 0 or 1 },
      on_click = function()
        toggle(todo.id)
      end,
      mre.icon { kind = "check", size = 14, color = todo.done and C.success or C.faint },
    },
    mre.text {
      todo.text,
      size = 15,
      weight = 500,
      grow = 1,
      color = todo.done and C.faint or C.text,
    },
    mre.text {
      os.date("%H:%M", todo.at),
      size = 12,
      color = C.faint,
    },
    icon_button {
      key = "remove-" .. todo.id,
      kind = "cross",
      size = 26,
      color = C.danger,
      animate = { opacity = fade },
      on_click = function()
        remove(todo.id)
      end,
    },
  }
end

local function empty_state()
  local current = filter()
  local title = "Nothing on your plate"
  local hint = "Add a task with the field below"
  if current == "active" then
    title = "Nothing active"
    hint = "Every task is done — nice work"
  elseif current == "done" then
    title = "Nothing completed yet"
    hint = "Finished tasks will collect here"
  end
  return mre.column {
    grow = 1,
    align = "center",
    justify = "center",
    gap = 8,
    mre.icon { kind = "check", size = 30, color = C.faint },
    mre.text { title, size = 16, weight = 600, color = C.muted },
    mre.text { hint, size = 13, color = C.faint },
  }
end

local function body()
  local items = visible()
  if #items == 0 then
    return empty_state()
  end
  local children = {}
  for index, todo in ipairs(items) do
    children[index] = todo_row(todo)
  end
  return mre.scroll {
    grow = 1,
    gap = 10,
    table.unpack(children),
  }
end

local function header(size)
  local active = count_active()
  return mre.row {
    justify = "between",
    align = "center",
    mre.column {
      gap = 3,
      mre.text { "Focus", size = 27, weight = 700, color = C.text, letter_spacing = -0.4 },
      mre.text {
        size.width > 560 and (os.date("%A, %d %B") .. "  ·  " .. active .. " active")
          or (active .. " active"),
        size = 12,
        color = C.faint,
      },
    },
    mre.badge {
      text = active .. " left",
      background = C.accent_dim,
      color = C.accent,
    },
  }
end

local function progress_card(size)
  local total = #todos
  local done = count_done()
  local ratio = total > 0 and done / total or 0
  local percent = math.floor(ratio * 100 + 0.5)

  return mre.card {
    radius = 16,
    background = C.surface,
    border = { color = C.border, width = 1 },
    padding = { left = 18, right = 18, top = 16, bottom = 16 },
    gap = 14,
    mre.row {
      justify = "between",
      align = "center",
      mre.text { "Progress", size = 13, weight = 600, color = C.muted },
      mre.text { percent .. "%", size = 13, weight = 700, color = C.accent },
    },
    mre.progress {
      width = "100%",
      value = ratio,
      height = 8,
      track = C.track,
      color = C.accent,
    },
    mre.row {
      gap = 30,
      stat(done, "done", C.success),
      stat(total - done, "active", C.accent),
      stat(total, "total", C.muted),
    },
  }
end

local function filter_row()
  return mre.row {
    justify = "between",
    align = "center",
    gap = 12,
    mre.tabs {
      items = FILTERS,
      active = filter_index(),
      on_change = function(_, item)
        filter(item.id)
      end,
      selected_background = C.accent_dim,
      selected_color = C.text,
      color = C.faint,
      gap = 4,
    },
    mre.button {
      "Clear completed",
      key = "clear",
      role = "button",
      padding = { left = 12, right = 12, top = 7, bottom = 7 },
      radius = 9,
      background = "transparent",
      color = C.faint,
      border = { color = C.border, width = 1 },
      on_click = function()
        confirm_clear(true)
      end,
    },
  }
end

local function composer(size)
  return mre.card {
    radius = 14,
    background = C.surface,
    border = { color = C.border, width = 1 },
    padding = { left = 8, right = 8, top = 8, bottom = 8 },
    mre.row {
      gap = 10,
      align = "center",
      mre.text_input {
        key = "draft",
        value = draft(),
        placeholder = "What needs doing?",
        size = 15,
        grow = 1,
        height = 42,
        radius = 10,
        background = "#0D1424",
        border = { color = C.border, width = 1 },
        color = C.text,
        on_change = function(text)
          draft(text)
        end,
        on_submit = add,
      },
      mre.button {
        "Add",
        role = "button",
        height = 42,
        padding = { left = 20, right = 20 },
        radius = 10,
        background = C.accent,
        color = C.bg,
        weight = 700,
        on_click = add,
      },
    },
  }
end

local function confirm_dialog()
  return mre.dialog {
    open = confirm_clear(),
    scrim = "#03060FCC",
    background = C.surface,
    border = { color = C.border, width = 1 },
    width = 380,
    mre.text { "Clear completed tasks?", size = 17, weight = 700, color = C.text },
    mre.text {
      "Every task that is already done will be removed. This cannot be undone.",
      size = 13,
      color = C.muted,
    },
    mre.row {
      gap = 10,
      justify = "end",
      mre.button {
        "Cancel",
        key = "cancel",
        role = "button",
        padding = { left = 16, right = 16, top = 9, bottom = 9 },
        radius = 9,
        background = "transparent",
        color = C.muted,
        border = { color = C.border, width = 1 },
        on_click = function()
          confirm_clear(false)
        end,
      },
      mre.button {
        "Clear them",
        role = "button",
        padding = { left = 16, right = 16, top = 9, bottom = 9 },
        radius = 9,
        background = C.danger,
        color = "#25060D",
        weight = 700,
        on_click = function()
          confirm_clear(false)
          clear_done()
        end,
      },
    },
  }
end

-- ---------------------------------------------------------------------------
-- The app
-- ---------------------------------------------------------------------------

mre.app {
  view = function()
    version() -- the list is untracked; this is what says it changed
    local size = mre.viewport()
    -- The root carries the background and nothing else, so an overlay
    -- anchored to it covers the whole window instead of the padded content.
    return mre.column {
      background = C.bg,
      mre.column {
        grow = 1,
        padding = { left = 30, right = 30, top = 26, bottom = 24 },
        gap = 18,
        header(size),
        progress_card(size),
        filter_row(),
        body(),
        composer(size),
      },
      -- The dialog paints over everything else, so it goes last.
      confirm_dialog(),
    }
  end,
}
