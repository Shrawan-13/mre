-- A todo list: type, add, complete, delete. Signals in, pixels out.

-- Signals hold scalars, so the list itself lives in a plain table and a
-- version signal marks it dirty whenever it changes.
local todos = {}
local version = mre.signal(0)
local draft = mre.signal("")
local next_id = 1

local function touch()
  version(version() + 1)
end

local function add(text)
  text = (text or draft()):match("^%s*(.-)%s*$")
  if text == "" then
    return
  end
  todos[#todos + 1] = { id = next_id, text = text, done = false }
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

local function remaining()
  local count = 0
  for _, todo in ipairs(todos) do
    if not todo.done then
      count = count + 1
    end
  end
  return count
end

mre.app {
  view = function()
    version() -- the list is untracked; this is what says it changed
    local rows = {}
    for index, todo in ipairs(todos) do
      rows[#rows + 1] = mre.row {
        height = 44,
        padding = { left = 12, right = 12 },
        gap = 12,
        background = index % 2 == 0 and "#F8FAFC" or "#FFFFFF",
        align = "center",
        mre.icon_button {
          kind = "check",
          key = "todo-" .. todo.id,
          background = todo.done and "#DCFCE7" or "#EEF1F5",
          color = todo.done and "#15803D" or "#475569",
          on_click = function() toggle(todo.id) end,
        },
        mre.column {
          grow = 1,
          mre.text {
            todo.text,
            size = 14,
            color = todo.done and "#94A3B8" or "#0F172A",
          },
        },
        mre.icon_button {
          kind = "cross",
          key = "remove-" .. todo.id,
          background = "#FEE2E2",
          color = "#B91C1C",
          on_click = function() remove(todo.id) end,
        },
      }
    end

    local body
    if #rows == 0 then
      body = mre.column {
        grow = 1,
        align = "center",
        justify = "center",
        mre.text { "Nothing yet — type below", size = 14, color = "#94A3B8" },
      }
    else
      body = mre.scroll { grow = 1, table.unpack(rows) }
    end

    return mre.column {
      padding = 16,
      gap = 12,
      background = "#FFFFFF",
      mre.row {
        height = 48,
        align = "center",
        mre.column {
          grow = 1,
          mre.text { "Today", size = 22, weight = 600, color = "#0F172A" },
        },
        mre.badge {
          text = remaining() .. " left",
          background = remaining() == 0 and "#DCFCE7" or "#E0E7FF",
          color = remaining() == 0 and "#15803D" or "#3730A3",
        },
      },
      body,
      mre.row {
        height = 44,
        gap = 8,
        mre.textfield {
          key = "draft",
          value = draft(),
          placeholder = "What needs doing?",
          width = 320,
          on_change = function(text) draft(text) end,
          on_submit = add,
        },
        mre.button {
          "Add",
          background = "#2563EB",
          color = "#FFFFFF",
          on_click = add,
        },
      },
    }
  end,
}
