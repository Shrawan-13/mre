-- MRE Mobile Showcase — the Lua program shipped in the Android APK.
--
-- Everything here is composition, style and behaviour. The engine never sees
-- this file: the host loads it as assets/main.lua. Change this one file and the
-- APK's whole UI, theme, motion and navigation change without a Rust rebuild.
--
-- The design goal is a production-shaped mobile app: a dark default palette,
-- a real menu, a theme switcher, deterministic motion, and pages that exercise
-- every primitive the framework exposes.

-- Default to dark. The host applies this before the first view is built.
mre.set_theme("dark")

-- A small design system, entirely in Lua. Named colours resolve against the
-- active Rust theme, so switching themes recolours every card, label and border.
mre.tokens {
  radius = 16,
  space = 12,
  title = 24,
  body = 14,
}

mre.sheet {
  ["text"] = { color = "text", size = 14 },
  ["button"] = { radius = 12 },
  ["text_input"] = { radius = 12 },
}

-- ---------------------------------------------------------------------------
-- Application state. Every signal is a durable piece of app state; the view
-- is a pure function of these values plus the viewport and safe area.
-- ---------------------------------------------------------------------------

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
local motion = mre.signal(0)
local pulse = mre.signal(0.2)
local show_details = mre.signal(true)
local offset = mre.signal(0)

local motion_spring = mre.animate(motion, { stiffness = 210, damping = 20 })
local pulse_spring = mre.animate(pulse, { stiffness = 260, damping = 22 })
local bounce = mre.spring(0.2)

local function clamp(value, low, high)
  return math.max(low, math.min(high, value))
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

local function theme_toggle()
  mre.set_theme(mre.theme() == "dark" and "light" or "dark")
end

-- A card that always follows the active palette. Cards are columns: the style
-- layer cannot reach a composed widget's explicit props, so the palette is
-- passed directly here and remains theme-dependent.
local function card(props)
  props = props or {}
  local children = {}
  for index, child in ipairs(props) do children[index] = child end
  return mre.column {
    key = props.key,
    class = props.class,
    width = props.width or "100%",
    padding = props.padding or 16,
    gap = props.gap or 10,
    radius = 16,
    background = "surface",
    border = "border",
    table.unpack(children),
  }
end

local function section_title(text, trailing)
  return mre.row {
    gap = 8,
    align = "center",
    mre.text { text, size = 15, weight = 700, grow = 1 },
    trailing or mre.column { width = 0, height = 0 },
  }
end

-- ---------------------------------------------------------------------------
-- Page: Controls — everyday widgets and the IME text field.
-- ---------------------------------------------------------------------------

local function controls()
  return mre.column {
    gap = 12,

    card {
      section_title("Counters", mre.icon { kind = "check", size = 16, color = "accent" }),
      mre.text { "Tapped " .. taps() .. " times", size = 20, weight = 700 },
      mre.row {
        gap = 8,
        mre.button { key = "tap", "Tap me", grow = 1, on_click = function() taps(taps() + 1) end },
        mre.button { key = "reset", "Reset", grow = 1, on_click = function() taps(0) end },
      },
    },

    card {
      section_title("Switches"),
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

    card {
      section_title("Volume", mre.text { math.floor(volume() * 100 + 0.5) .. "%", color = "accent", weight = 700 }),
      mre.slider { key = "slider", value = volume(), grow = 1, on_change = function(value) volume(value) end },
      mre.progress { value = volume() },
    },

    card {
      section_title("Text input"),
      mre.textfield {
        key = "name",
        value = name(),
        placeholder = "Type here — Android IME works",
        on_change = function(value) name(value) end,
      },
      mre.text { "Hello, " .. (name() ~= "" and name() or "friend"), color = "text_muted" },
    },
  }
end

-- ---------------------------------------------------------------------------
-- Page: Layout — flex directions, grid, explicit display modes and overlays.
-- ---------------------------------------------------------------------------

local function layout_page()
  local size = mre.viewport()
  local columns = size.width >= 520 and { "1fr", "1fr", "1fr" } or { "1fr", "1fr" }
  return mre.column {
    gap = 12,

    card {
      section_title("Flex directions", mre.text { "deterministic", size = 11, color = "text_muted" }),
      mre.row {
        gap = 6,
        mre.text { "L", width = 28, height = 28, background = "accent", color = "on_accent", align = "center" },
        mre.text { "R", width = 28, height = 28, background = "surface_alt", align = "center" },
        mre.text { "T", width = 28, height = 28, background = "surface_alt", align = "center" },
      },
      mre.row {
        direction = "row-reverse",
        gap = 6,
        mre.text { "1", width = 28, height = 28, background = "surface_alt", align = "center" },
        mre.text { "2", width = 28, height = 28, background = "accent", color = "on_accent", align = "center" },
      },
      mre.row {
        gap = 6,
        mre.text { "hidden", display = "none", width = 50, height = 20 },
        mre.text { "display:none removes this node entirely", size = 12, color = "text_muted" },
      },
    },

    card {
      section_title("Grid", mre.text { #columns .. " columns", size = 11, color = "text_muted" }),
      mre.grid {
        columns = columns,
        gap = 8,
        mre.text { "A", height = 40, background = "surface_alt", align = "center" },
        mre.text { "B", height = 40, background = "surface_alt", align = "center" },
        mre.text { "C", height = 40, background = "surface_alt", align = "center" },
        mre.text { "D", height = 40, background = "surface_alt", align = "center" },
      },
    },

    card {
      section_title("Overlay anchoring"),
      mre.text {
        "Dialogs and scrims anchor to the full window rect, not to the safe area.",
        size = 12,
        color = "text_muted",
      },
      mre.button { key = "layout-dialog", "Open a real overlay", on_click = function() dialog(true) end },
    },

    card {
      section_title("Viewport"),
      mre.text {
        math.floor(size.width) .. " x " .. math.floor(size.height) .. " dp @ " .. string.format("%.2f", size.scale) .. "x",
        color = "accent",
        weight = 600,
      },
      mre.text { "Reading mre.viewport() makes this page rebuild on rotation.", size = 12, color = "text_muted" },
    },
  }
end

-- ---------------------------------------------------------------------------
-- Page: Motion — springs, tweens, timers and deterministic time.
-- ---------------------------------------------------------------------------

local function motion_page()
  return mre.column {
    gap = 12,

    card {
      section_title("Spring", mre.text { "velocity-preserving", size = 11, color = "text_muted" }),
      mre.row {
        height = 56,
        background = "surface_alt",
        radius = 12,
        clip = true,
        mre.column {
          width = 56,
          height = 40,
          radius = 10,
          background = "accent",
          animate = { x = motion_spring },
        },
      },
      mre.button {
        key = "motion-kick",
        "Move the square",
        on_click = function() motion(motion() > 100 and 0 or 260) end,
      },
    },

    card {
      section_title("Tween + sequence", mre.text { "engine clock", size = 11, color = "text_muted" }),
      mre.row {
        height = 48,
        gap = 6,
        align = "center",
        mre.column {
          width = 28, height = 28, radius = 14, background = "accent",
          animate = { opacity = pulse_spring },
        },
        mre.column {
          width = 28, height = 28, radius = 14, background = "accent",
          animate = { opacity = bounce },
        },
        mre.text { "No wall-clock reads: mre.now() advances only on tick.", size = 12, color = "text_muted", grow = 1 },
      },
      mre.button {
        key = "motion-pulse",
        "Pulse",
        on_click = function()
          pulse(pulse() > 0.5 and 0.2 or 1)
          bounce(1)
          mre.after(180, function() bounce(0.2) end)
        end,
      },
    },

    card {
      section_title("Determinism"),
      mre.text {
        "Same events + same viewport = same tree. No random, no OS time, no hidden host state in the view.",
        size = 12,
        color = "text_muted",
      },
      mre.text { "clock: " .. string.format("%.2f", mre.now()) .. "s", color = "accent" },
    },
  }
end

-- ---------------------------------------------------------------------------
-- Page: Widgets — composed controls, select, tabs, dialogs and error boundary.
-- ---------------------------------------------------------------------------

local function widgets()
  local pages = {
    mre.column { gap = 4, mre.text { "First tab", size = 13 } },
    mre.column { gap = 4, mre.text { "Second tab", size = 13 } },
  }
  return mre.column {
    gap = 12,

    card {
      section_title("Tabs"),
      mre.tabs {
        items = { "One", "Two" },
        active = tab(),
        on_change = function(index) tab(index) end,
      },
      pages[tab()],
    },

    card {
      section_title("Select and badges"),
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
        mre.badge { text = "GPU" },
      },
    },

    card {
      section_title("Overlays"),
      mre.text { "The scrim covers every inset band because overlays use window coordinates.", size = 12, color = "text_muted" },
      mre.button { key = "open-dialog", "Open dialog", on_click = function() dialog(true) end },
    },

    mre.boundary {
      view = function()
        return card {
          section_title("Error boundary"),
          mre.text { "A broken subtree falls back instead of killing the frame.", size = 12, color = "text_muted" },
          mre.button {
            key = "show-details",
            show_details() and "Hide details" or "Show details",
            on_click = function() show_details(not show_details()) end,
          },
          show_details() and mre.text { "Details are just another signal-driven node.", size = 12 } or mre.column { width = 0, height = 0 },
        }
      end,
      fallback = function() return card { mre.text { "Recovered." } } end,
    },
  }
end

-- ---------------------------------------------------------------------------
-- Page: Checks — the same click-to-run assertions the CI uses.
-- ---------------------------------------------------------------------------

local function checks()
  return mre.column {
    gap = 12,
    card {
      section_title("Click-to-run checks"),
      mre.text { "These assertions run in Lua when you tap the button.", size = 12, color = "text_muted" },
      mre.button { key = "run-checks", "Run checks", on_click = run_checks },
      mre.badge { text = result() },
    },
    card {
      section_title("What the button verifies"),
      mre.text { "1. a signal can be read", size = 12 },
      mre.text { "2. a toggle holds a boolean", size = 12 },
      mre.text { "3. a slider stays in range", size = 12 },
      mre.text { "4. a text field holds a string", size = 12 },
      mre.text { "5. the current page is known", size = 12 },
    },
  }
end

-- ---------------------------------------------------------------------------
-- Page: About — environment, safe area and live diagnostics.
-- ---------------------------------------------------------------------------

local function about()
  local safe = mre.safe_area()
  local size = mre.viewport()
  local diagnostics = mre.diagnostics()
  return mre.column {
    gap = 12,
    card {
      section_title("MRE on Android"),
      mre.text {
        "Rust engine, Lua framework, GPU renderer, deterministic layout. "
        .. "Every colour and widget on this screen comes from Lua.",
        size = 13,
        color = "text_muted",
      },
    },
    card {
      section_title("Environment"),
      mre.text {
        string.format("Safe area dp: top %d right %d bottom %d left %d",
          math.floor(safe.top + 0.5), math.floor(safe.right + 0.5),
          math.floor(safe.bottom + 0.5), math.floor(safe.left + 0.5)),
        size = 12,
      },
      mre.text {
        string.format("Viewport: %d x %d dp @ %.2fx", math.floor(size.width), math.floor(size.height), size.scale),
        size = 12,
      },
      mre.text { "Theme: " .. mre.theme(), size = 12, color = "accent" },
      mre.text { "Diagnostics: " .. #diagnostics .. " issue(s)", size = 12, color = "text_muted" },
    },
    card {
      section_title("Appearance"),
      mre.text { "Switch the palette at runtime. No Rust rebuild, no reload.", size = 12, color = "text_muted" },
      mre.button { key = "toggle-theme", "Switch to " .. (mre.theme() == "dark" and "light" or "dark") .. " theme", on_click = theme_toggle },
    },
  }
end

local function current_page()
  if page() == "Layout" then return layout_page() end
  if page() == "Motion" then return motion_page() end
  if page() == "Widgets" then return widgets() end
  if page() == "Checks" then return checks() end
  if page() == "About" then return about() end
  return controls()
end

-- ---------------------------------------------------------------------------
-- Root: a top app bar, the page body, a bottom menu, and a window-level dialog.
-- ---------------------------------------------------------------------------

mre.app {
  view = function()
    return mre.column {
      width = "100%",
      height = "100%",
      background = "background",

      -- App bar. The engine reserves the safe area for us; no manual insets.
      mre.row {
        padding = { left = 16, right = 16, top = 12, bottom = 12 },
        gap = 10,
        align = "center",
        background = "surface",
        mre.column {
          grow = 1,
          mre.text { "MRE for Android", size = 20, weight = 700 },
          mre.text { "mobile framework showcase", size = 11, color = "text_muted" },
        },
        mre.button { key = "toggle-theme-bar", "Theme", on_click = theme_toggle },
      },

      -- Page body.
      mre.column {
        grow = 1,
        padding = { left = 14, right = 14, top = 14, bottom = 14 },
        clip = true,
        current_page(),
      },

      -- Bottom menu: six stable destinations in a compact grid.
      mre.grid {
        columns = { "1fr", "1fr", "1fr" },
        gap = 6,
        padding = { left = 12, right = 12, top = 10, bottom = 12 },
        background = "surface",
        mre.button { key = "page-controls", "Controls", on_click = function() page("Controls") end },
        mre.button { key = "page-layout", "Layout", on_click = function() page("Layout") end },
        mre.button { key = "page-motion", "Motion", on_click = function() page("Motion") end },
        mre.button { key = "page-widgets", "Widgets", on_click = function() page("Widgets") end },
        mre.button { key = "page-checks", "Checks", on_click = function() page("Checks") end },
        mre.button { key = "page-about", "About", on_click = function() page("About") end },
      },

      -- A dialog is last: it is an overlay anchored to the whole window.
      mre.dialog {
        open = dialog(),
        on_dismiss = function() dialog(false) end,
        mre.text { "Window-level overlay", size = 18, weight = 700 },
        mre.text { "The scrim uses the full window rect, including safe-area bands.", size = 13, color = "text_muted" },
        mre.row {
          gap = 8,
          justify = "end",
          mre.button { "Close", on_click = function() dialog(false) end },
        },
      },
    }
  end,
}
