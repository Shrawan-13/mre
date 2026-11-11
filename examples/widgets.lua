-- The widget library, in one screen.
--
-- Everything here is composed from containers, text, icons and callbacks. The
-- engine has no special case for any of it, which is why a new widget is a
-- function rather than a change to the engine.

local tab = mre.signal(1)
local density = mre.signal("Comfortable")
local open = mre.signal(false)
local notes = mre.signal("")
local over_save = mre.hover("save")

mre.app {
  view = function()
    local pages = {
      mre.column {
        gap = 12,
        mre.text { "Orders", size = 18, weight = 600 },
        mre.text { "Nothing here yet.", color = "#6B7280" },
      },
      mre.column {
        gap = 12,
        mre.text { "Inventory", size = 18, weight = 600 },
        mre.card {
          mre.row {
            align = "center",
            justify = "between",
            mre.text { "12 items low" },
            mre.badge { text = "3" },
          },
        },
      },
      mre.column {
        gap = 12,
        mre.text { "Settings", size = 18, weight = 600 },
        mre.select {
          label = "Density",
          options = { "Comfortable", "Compact", "Roomy" },
          value = density(),
          open = false,
          on_toggle = function() end,
        },
      },
    }

    return mre.column {
      padding = 24,
      gap = 20,
      mre.tabs {
        items = { "Orders", "Inventory", "Settings" },
        active = tab(),
        on_change = function(index) tab(index) end,
      },
      pages[tab()],
      mre.row {
        gap = 12,
        align = "center",
        mre.button {
          "Open dialog",
          on_click = function() open(true) end,
        },
        mre.column {
          gap = 0,
          mre.button {
            key = "save",
            "Save notes",
            on_click = function() notes("Saved") end,
          },
          over_save() and mre.tooltip {
            text = "Saves the document",
            absolute = true,
            top = 38,
            left = 0,
          } or mre.spacer { size = 0 },
        },
        mre.icon_button { kind = "close", label = "Close", on_click = function() end },
      },
      mre.dialog {
        open = open(),
        on_dismiss = function() open(false) end,
        mre.text { "Discard the draft?", size = 16, weight = 600 },
        mre.text {
          "This cannot be undone.",
          size = 13,
          color = "#6B7280",
        },
        mre.row {
          gap = 8,
          justify = "end",
          mre.button { "Cancel", on_click = function() open(false) end },
          mre.button { "Discard", background = "#DC2626", color = "#FFFFFF", on_click = function() open(false) end },
        },
      },
    }
  end,
}
