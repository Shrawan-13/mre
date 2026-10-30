-- A data-dense market dashboard, written entirely in Lua.
--
-- This is the reference application shape: a sticky header, a virtualised list
-- in the real engine, aligned numeric columns and conditional colour. It also
-- exercises reactivity: the Refresh button writes a signal and only the header
-- is affected.

local ticks = mre.signal(0)
local progress = mre.signal(0)
local animate_progress = mre.animate(progress, { stiffness = 220, damping = 24 })

local data = {
  { "NVDA", 121.44, 2.31, "412.8M", "2.99T" },
  { "AAPL", 231.87, -0.42, "38.1M", "3.52T" },
  { "MSFT", 428.15, 0.87, "21.6M", "2.48T" },
  { "GOOGL", 172.63, 1.12, "24.9M", "2.11T" },
  { "AMZN", 201.09, -1.03, "33.4M", "3.18T" },
  { "META", 587.31, 3.44, "12.2M", "1.49T" },
  { "TSLA", 248.50, -2.18, "88.7M", "793B" },
  { "AVGO", 182.76, 1.90, "9.4M", "851B" },
  { "BRK.B", 468.02, 0.21, "2.8M", "1.01T" },
  { "JPM", 221.45, 0.66, "7.1M", "635B" },
  { "LLY", 812.20, -0.35, "1.9M", "772B" },
  { "V", 289.77, 0.44, "5.3M", "572B" },
  { "UNH", 568.10, 1.07, "3.2M", "523B" },
  { "XOM", 118.32, -0.88, "14.6M", "521B" },
  { "WMT", 81.44, 0.92, "18.3M", "655B" },
  { "MA", 492.61, 0.38, "2.4M", "455B" },
  { "ORCL", 173.90, 2.05, "8.8M", "478B" },
  { "COST", 902.33, -0.17, "1.7M", "400B" },
  { "HD", 402.18, 0.73, "3.1M", "399B" },
  { "PG", 171.05, 0.29, "5.9M", "403B" },
}

local COLUMNS = {
  { title = "SYMBOL", width = 110, align = "start" },
  { title = "PRICE", width = 100, align = "end" },
  { title = "CHANGE", width = 90, align = "end" },
  { title = "VOLUME", width = 110, align = "end" },
  { title = "MARKET CAP", width = 120, align = "end" },
}

local function cell(text, width, align, color, weight)
  return mre.text {
    tostring(text),
    size = 13,
    width = width,
    text_align = align,
    color = color,
    weight = weight,
  }
end

local function header()
  local children = {}
  for index, column in ipairs(COLUMNS) do
    children[index] = mre.text {
      column.title,
      size = 12,
      weight = 600,
      width = column.width,
      text_align = column.align,
      color = "#6B7280",
    }
  end
  return mre.row {
    height = 36,
    padding = { left = 12, right = 12 },
    gap = 12,
    background = "#EEF1F5",
    table.unpack(children),
  }
end

local function row(entry, index)
  local tint = index % 2 == 0 and "#FFFFFF" or "#F7F8FA"
  local change = entry[3]
  local change_color = change >= 0 and "#15803D" or "#B91C1C"
  local change_text = string.format("%+.2f%%", change)

  return mre.row {
    height = 30,
    padding = { left = 12, right = 12 },
    gap = 12,
    background = tint,
    cell(entry[1], 110, "start", "#111827", 500),
    cell(string.format("$%.2f", entry[2]), 100, "end", "#111827"),
    cell(change_text, 90, "end", change_color),
    cell(entry[4], 110, "end", "#6B7280"),
    cell(entry[5], 120, "end", "#6B7280"),
  }
end

mre.app {
  view = function()
    local count = ticks()
    local rows = {}
    for index, entry in ipairs(data) do
      rows[index] = row(entry, index)
    end

    return mre.column {
      background = "#FFFFFF",
      mre.row {
        height = 64,
        padding = { left = 16, right = 16 },
        align = "center",
        mre.column {
          grow = 1,
          mre.text { "Markets", size = 22, weight = 600 },
          mre.text {
            count == 0 and "Live · 20 instruments · USD"
              or string.format("Refreshed %d time%s", count, count == 1 and "" or "s"),
            size = 12,
            color = "#6B7280",
          },
          -- An engine-driven spring: retargeting mid-flight keeps velocity.
          mre.column {
            width = 160, height = 3, radius = 2, background = "#E5E7EB",
            mre.column {
              width = progress() * 160, height = 3, radius = 2,
              background = "#2563EB",
            },
          },
        },
        mre.button {
          "Refresh",
          on_click = function()
            ticks(count + 1)
            animate_progress(math.min(1.0, (count + 1) / 10))
          end,
        },
      },
      header(),
      table.unpack(rows),
    }
  end,
}
