-- Animation as a property, not a re-render.
--
-- Both springs below are bound straight to paint properties. The view function
-- runs once; every frame after that is the engine reading two numbers and
-- shifting a rectangle. A list of ten thousand rows could animate this way
-- without touching a single one of them.

local fade = mre.spring(0)
local slide = mre.spring(40)

fade(1)
slide(0)

mre.app {
  view = function()
    return mre.column {
      padding = 24,
      gap = 16,
      background = "#FFFFFF",
      mre.text { "Bound properties", size = 22, weight = 600 },
      mre.text {
        "The card below fades and slides. The view ran once.",
        size = 12,
        color = "#6B7280",
      },
      mre.column {
        key = "card",
        width = 300,
        height = 120,
        radius = 12,
        background = "#2563EB",
        animate = { opacity = fade, y = slide },
        mre.text {
          "No rebuild",
          size = 20,
          weight = 600,
          color = "#FFFFFF",
          padding = 20,
        },
      },
      mre.button {
        "Replay",
        on_click = function()
          fade(0)
          slide(40)
          fade(1)
          slide(0)
        end,
      },
    }
  end,
}
