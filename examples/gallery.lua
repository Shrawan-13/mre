-- The widget gallery: everything here is composed from containers, text,
-- rounded rects and pointer callbacks. There are no special-case widget render
-- paths, so a widget behaves exactly like any other subtree.

local show_grid = mre.signal(true)
local enabled = mre.signal(false)
local volume = mre.signal(0.4)
local quality = mre.signal(2)
local saves = mre.signal(0)

local function percent(value)
  return string.format("%d%%", math.floor(value * 100 + 0.5))
end

mre.app {
  view = function()
    return mre.column {
      padding = 24,
      gap = 18,
      background = "#FFFFFF",
      mre.text { "Controls", size = 22, weight = 600 },
      mre.text {
        "Composed widgets, live state, engine-driven hover.",
        size = 13,
        color = "#6B7280",
      },

      mre.checkbox {
        key = "show-grid",
        label = "Show grid",
        checked = show_grid(),
        on_change = function(value) show_grid(value) end,
      },

      mre.row {
        gap = 12,
        align = "center",
        mre.toggle {
          key = "toggle",
          value = enabled(),
          on_change = function(value) enabled(value) end,
        },
        mre.text {
          enabled() and "Rendering enabled" or "Rendering paused",
          size = 14,
          color = "#374151",
        },
      },

      mre.column {
        gap = 6,
        mre.row {
          gap = 8,
          mre.text { "Volume", size = 13, color = "#6B7280", grow = 1 },
          mre.text { percent(volume()), size = 13, weight = 600 },
        },
        mre.slider {
          key = "volume",
          value = volume(),
          width = 320,
          on_change = function(value) volume(value) end,
        },
      },

      mre.column {
        gap = 6,
        mre.row {
          gap = 8,
          mre.text { "Quality", size = 13, color = "#6B7280", grow = 1 },
          mre.text { tostring(quality()), size = 13, weight = 600 },
        },
        mre.slider {
          key = "quality",
          value = quality(),
          min = 0,
          max = 4,
          width = 320,
          on_change = function(value) quality(math.floor(value + 0.5)) end,
        },
      },

      mre.progress { value = volume(), width = 320 },
      mre.divider {},

      mre.row {
        gap = 8,
        align = "center",
        mre.button {
          key = "save",
          "Save",
          on_click = function() saves(saves() + 1) end,
        },
        mre.text {
          saves() == 0 and "No changes saved"
            or string.format("Saved %d time%s", saves(), saves() == 1 and "" or "s"),
          size = 13,
          color = "#6B7280",
        },
      },
    }
  end,
}
