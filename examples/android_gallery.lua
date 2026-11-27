-- MRE for Android — an interactive gallery with click-to-run checks.
--
-- This is the Lua program packaged as assets/main.lua in the APK. Every page
-- is composed from the same widgets as the desktop examples; nothing here is
-- Android-specific. Buttons carry stable `key`s so the Rust test can click
-- them through the same hit-testing a finger uses.

local page = mre.signal("Controls")
local taps = mre.signal(0)
local dialog = mre.signal(false)
local density = mre.signal("Comfortable")
local tab = mre.signal(1)
local enabled = mre.signal(false)
local agreed = mre.signal(false)
local volume = mre.signal(0.4)
local name = mre.signal("")
local result = mre.signal("not run yet")

local function percent(value)
  return string.format("%d%%", math.floor(value * 100 + 0.5))
end

local function run_checks()
  local passed = 0
  if taps() >= 0 then passed = passed + 1 end
  if type(enabled()) == "boolean" then passed = passed + 1 end
  if volume() >= 0.0 and volume() <= 1.0 then passed = passed + 1 end
  if type(name()) == "string" then passed = passed + 1 end
  if page() == "Checks" then passed = passed + 1 end
  result(passed .. "/5 passed")
end

local function controls()
  return mre.column {
    gap = 10,
    mre.card {
      mre.column {
        gap = 6,
        mre.text { "Counters", size = 16, weight = 600 },
        mre.text { "Tapped " .. taps() .. " times", key = "tap-count", size = 14 },
        mre.row {
          gap = 8,
          mre.button {
            key = "tap",
            "Tap me",
            on_click = function() taps(taps() + 1) end,
          },
          mre.button {
            key = "reset",
            "Reset",
            on_click = function() taps(0) end,
          },
        },
      },
    },
    mre.card {
      mre.column {
        gap = 6,
        mre.text { "Switches", size = 16, weight = 600 },
        mre.toggle {
          key = "toggle",
          label = "Rendering enabled",
          value = enabled(),
          on_change = function(value) enabled(value) end,
        },
        mre.checkbox {
          key = "checkbox",
          label = "I read the docs",
          checked = agreed(),
          on_change = function(value) agreed(value) end,
        },
      },
    },
    mre.card {
      mre.column {
        gap = 6,
        mre.row {
          gap = 8,
          mre.text { "Volume", size = 13, color = "#6B7280", grow = 1 },
          mre.text { percent(volume()), size = 13, weight = 600 },
        },
        mre.slider {
          key = "slider",
          value = volume(),
          on_change = function(value) volume(value) end,
        },
        mre.progress { value = volume() },
      },
    },
    mre.card {
      mre.column {
        gap = 6,
        mre.textfield {
          key = "name",
          value = name(),
          placeholder = "Type here",
          on_change = function(value) name(value) end,
        },
        mre.text { "Hello, " .. (name() ~= "" and name() or "friend"), size = 14 },
      },
    },
  }
end

local function checks()
  return mre.column {
    gap = 10,
    mre.card {
      mre.column {
        gap = 6,
        mre.text { "Click-to-run checks", size = 16, weight = 600 },
        mre.text {
          "These assertions run in Lua when you tap the button.",
          size = 13,
          color = "#6B7280",
        },
        mre.button { key = "run-checks", "Run checks", on_click = run_checks },
        mre.badge { text = result() },
      },
    },
    mre.card {
      mre.column {
        gap = 4,
        mre.text { "What the button verifies", size = 15, weight = 600 },
        mre.text { "1. a signal can be read", size = 13 },
        mre.text { "2. a toggle holds a boolean", size = 13 },
        mre.text { "3. a slider stays in range", size = 13 },
        mre.text { "4. a text field holds a string", size = 13 },
        mre.text { "5. the current page is known", size = 13 },
      },
    },
  }
end

local function widgets()
  local pages = {
    mre.column { gap = 4, mre.text { "First tab", size = 13 } },
    mre.column { gap = 4, mre.text { "Second tab", size = 13 } },
  }
  return mre.column {
    gap = 10,
    mre.card {
      mre.column {
        gap = 6,
        mre.text { "Tabs", size = 16, weight = 600 },
        mre.tabs {
          items = { "One", "Two" },
          active = tab(),
          on_change = function(index) tab(index) end,
        },
        pages[tab()],
      },
    },
    mre.card {
      mre.column {
        gap = 6,
        mre.text { "Select and badges", size = 16, weight = 600 },
        mre.select {
          key = "density-select",
          label = "Density",
          options = { "Comfortable", "Compact", "Roomy" },
          value = density(),
        },
        mre.row {
          gap = 8,
          mre.badge { text = "new" },
          mre.badge { text = "Lua" },
        },
      },
    },
    mre.button {
      key = "open-dialog",
      "Open dialog",
      on_click = function() dialog(true) end,
    },
  }
end

local function about()
  local safe = mre.safe_area()
  return mre.card {
    mre.column {
      gap = 6,
      mre.text { "MRE on Android", size = 16, weight = 600 },
      mre.text {
        string.format("Safe area (dp): top %d, right %d, bottom %d, left %d",
          math.floor(safe.top + 0.5), math.floor(safe.right + 0.5),
          math.floor(safe.bottom + 0.5), math.floor(safe.left + 0.5)),
        size = 12,
        color = "#6B7280",
      },
      mre.text {
        "A NativeActivity host, wgpu renderer, Lua apps and the same "
        .. "performance gates as the desktop engine.",
        size = 13,
      },
      mre.text { "Tap the buttons above to try the widgets.", size = 13, color = "#6B7280" },
    },
  }
end

mre.app {
  view = function()
    local safe = mre.safe_area()
    return mre.column {
      padding = {
        top = 16 + safe.top,
        right = 16 + safe.right,
        bottom = 16 + safe.bottom,
        left = 16 + safe.left,
      },
      gap = 12,
      background = "#FFFFFF",
      mre.text { "MRE for Android", size = 22, weight = 700 },
      mre.row {
        gap = 8,
        mre.button {
          key = "page-controls",
          "Controls",
          on_click = function() page("Controls") end,
        },
        mre.button {
          key = "page-checks",
          "Checks",
          on_click = function() page("Checks") end,
        },
        mre.button {
          key = "page-widgets",
          "Widgets",
          on_click = function() page("Widgets") end,
        },
        mre.button {
          key = "page-about",
          "About",
          on_click = function() page("About") end,
        },
      },
      page() == "Checks" and checks()
        or (page() == "Widgets" and widgets()
          or (page() == "About" and about() or controls())),
      -- A dialog is a scrim over the app, so it is the last child of the root.
      mre.dialog {
        open = dialog(),
        on_dismiss = function() dialog(false) end,
        mre.text { "More widgets, same engine.", size = 16, weight = 600 },
        mre.row {
          gap = 8,
          justify = "end",
          mre.button { "Close", on_click = function() dialog(false) end },
        },
      },
    }
  end,
}
