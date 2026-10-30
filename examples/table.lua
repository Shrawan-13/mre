-- Fifty thousand rows, and only the visible ones exist.
--
-- The list is virtualised: Lua is asked for the rows inside the viewport plus a
-- one-row overscan, so scrolling costs the same whether the data set has fifty
-- rows or fifty thousand. Scrolling writes a signal, which retargets the window;
-- nothing walks the full data set.
--
-- This is the shape of workload the performance contract is written against.

local COUNT = 50000
local ROW_HEIGHT = 28
local VIEWPORT = 600
local MAX_OFFSET = COUNT * ROW_HEIGHT - VIEWPORT

local offset = mre.signal(0)
local sorted = mre.signal(false)

local function row(index)
  local value = (index * 37) % 1000
  local change = ((index * 7919) % 2001 - 1000) / 100
  local tint = index % 2 == 0 and "#FFFFFF" or "#F7F8FA"
  local change_color = change >= 0 and "#15803D" or "#B91C1C"

  return mre.row {
    height = ROW_HEIGHT,
    padding = { left = 16, right = 16 },
    gap = 12,
    background = tint,
    mre.text { string.format("%05d", index), size = 13, width = 80, color = "#6B7280" },
    mre.text { "Instrument " .. string.format("%05d", index), size = 13, width = 240 },
    mre.text {
      string.format("%.2f", value / 10),
      size = 13, width = 100, text_align = "end",
    },
    mre.text {
      string.format("%+.2f%%", change),
      size = 13, width = 100, text_align = "end", color = change_color,
    },
    mre.text {
      string.format("%d", (index * 104729) % 900000 + 100),
      size = 13, width = 120, text_align = "end", color = "#6B7280",
    },
  }
end

mre.app {
  view = function()
    local from = math.floor(offset() / ROW_HEIGHT)
    local to = math.min(COUNT, from + math.ceil(VIEWPORT / ROW_HEIGHT))

    return mre.column {
      background = "#FFFFFF",
      mre.row {
        height = 56,
        padding = { left = 16, right = 16 },
        align = "center",
        mre.column {
          grow = 1,
          mre.text { "All instruments", size = 20, weight = 600 },
          mre.text {
            string.format(
              "%s rows, showing %d–%d",
              sorted() and "sorted" or "50,000",
              from + 1,
              to
            ),
            size = 12,
            color = "#6B7280",
          },
        },
        mre.row {
          gap = 8,
          mre.button {
            sorted() and "Unsort" or "Sort",
            background = "#E5E7EB",
            color = "#111827",
            on_click = function() sorted(not sorted()) end,
          },
          mre.button { "Export" },
        },
      },
      mre.row {
        height = 32,
        padding = { left = 16, right = 16 },
        gap = 12,
        background = "#EEF1F5",
        mre.text { "ID", size = 12, weight = 600, width = 80, color = "#6B7280" },
        mre.text { "INSTRUMENT", size = 12, weight = 600, width = 240, color = "#6B7280" },
        mre.text { "PRICE", size = 12, weight = 600, width = 100, text_align = "end", color = "#6B7280" },
        mre.text { "CHANGE", size = 12, weight = 600, width = 100, text_align = "end", color = "#6B7280" },
        mre.text { "VOLUME", size = 12, weight = 600, width = 120, text_align = "end", color = "#6B7280" },
      },
      mre.scroll {
        height = VIEWPORT,
        on_scroll = function(_, dy)
          offset(math.max(0, math.min(MAX_OFFSET, offset() + dy)))
        end,
        mre.list {
          count = COUNT,
          row_height = ROW_HEIGHT,
          height = VIEWPORT,
          offset = offset(),
          render = row,
        },
      },
    }
  end,
}
