-- Text input: engine-owned editing state, Lua-owned value.
--
-- The field keeps its own caret and buffer while focused, so a keystroke does
-- not depend on the view being rebuilt between characters. `on_change` publishes
-- the value back into a signal; the view reads that signal when it renders
-- everything else.

local name = mre.signal("")
local email = mre.signal("")
local password = mre.signal("")
local status = mre.signal("Nothing submitted yet")

local function submit()
  if name() == "" then
    status("A name is required")
    return
  end
  status(string.format("Submitted %s <%s>", name(), email()))
end

mre.app {
  view = function()
    return mre.column {
      padding = 24,
      gap = 14,
      background = "#FFFFFF",
      mre.text { "Create account", size = 22, weight = 600 },
      mre.text {
        "Click a field and type. Tab moves between fields; Enter submits.",
        size = 12,
        color = "#6B7280",
      },

      mre.textfield {
        key = "name",
        value = name(),
        placeholder = "Full name",
        width = 320,
        on_change = function(value) name(value) end,
        on_submit = submit,
      },
      mre.textfield {
        key = "email",
        value = email(),
        placeholder = "you@example.com",
        width = 320,
        on_change = function(value) email(value) end,
        on_submit = submit,
      },
      mre.textfield {
        key = "password",
        value = password(),
        placeholder = "Password",
        password = true,
        width = 320,
        on_change = function(value) password(value) end,
        on_submit = submit,
      },

      mre.row {
        gap = 10,
        align = "center",
        mre.button { "Create", on_click = submit },
        mre.text { status(), size = 12, color = "#6B7280" },
      },
    }
  end,
}
