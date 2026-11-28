-- YouTube Music-style clone for MRE — pure Lua, no network, no Rust rebuild.
-- Screens are composed from mre.* nodes; navigation is engine-owned via mre.nav.

mre.set_theme("dark")

-- Palette. The GPU backend linearises sRGB at upload, so these render as authored.
local BG        = "#0B0B0D"
local SURFACE   = "#17171A"
local ELEVATED  = "#24242A"
local CHIP      = "#2C2C33"
local LINE      = "#34343B"
local RED       = "#FF3B5C"
local PINK      = "#FF7A90"
local TEXT      = "#F6F6F8"
local MUTED     = "#A0A0AA"
local GREEN     = "#3DDC84"

-- ---------------------------------------------------------------------------
-- Mock catalog. Everything is local and deterministic.
-- ---------------------------------------------------------------------------

local artists = {
  { id = "ar1", name = "Arctic Monkeys", meta = "12.4M monthly listeners", color = "#3E5C76", genre = "Indie rock" },
  { id = "ar2", name = "Dua Lipa",       meta = "68.1M monthly listeners", color = "#9B3E6E", genre = "Pop" },
  { id = "ar3", name = "The Weeknd",     meta = "91.7M monthly listeners", color = "#7A2E2E", genre = "R&B" },
  { id = "ar4", name = "Billie Eilish",  meta = "73.2M monthly listeners", color = "#2F6B62", genre = "Alt pop" },
  { id = "ar5", name = "Tame Impala",    meta = "18.9M monthly listeners", color = "#6A4E8C", genre = "Psych" },
}

local albums = {
  { id = "al1", title = "AM",              artist = "Arctic Monkeys", year = "2013", color = "#3E5C76", songs = { "s1", "s2", "s3" } },
  { id = "al2", title = "Future Nostalgia", artist = "Dua Lipa",       year = "2020", color = "#9B3E6E", songs = { "s4", "s5", "s6" } },
  { id = "al3", title = "After Hours",      artist = "The Weeknd",     year = "2020", color = "#7A2E2E", songs = { "s7", "s8", "s9" } },
  { id = "al4", title = "Happier Than Ever", artist = "Billie Eilish", year = "2021", color = "#2F6B62", songs = { "s10", "s11" } },
  { id = "al5", title = "Currents",         artist = "Tame Impala",    year = "2015", color = "#6A4E8C", songs = { "s12", "s13" } },
}

local songs = {
  { id = "s1",  title = "Do I Wanna Know?",   artist = "Arctic Monkeys", album = "AM",               duration = 272, color = "#3E5C76", plays = "1.4B" },
  { id = "s2",  title = "Why'd You Only Call Me When You're High?", artist = "Arctic Monkeys", album = "AM", duration = 202, color = "#456A85", plays = "812M" },
  { id = "s3",  title = "R U Mine?",          artist = "Arctic Monkeys", album = "AM",               duration = 202, color = "#2F4C63", plays = "640M" },
  { id = "s4",  title = "Levitating",         artist = "Dua Lipa",       album = "Future Nostalgia", duration = 203, color = "#B44E80", plays = "2.1B" },
  { id = "s5",  title = "Don't Start Now",    artist = "Dua Lipa",       album = "Future Nostalgia", duration = 183, color = "#8E3963", plays = "1.7B" },
  { id = "s6",  title = "Physical",           artist = "Dua Lipa",       album = "Future Nostalgia", duration = 194, color = "#C65B86", plays = "946M" },
  { id = "s7",  title = "Blinding Lights",    artist = "The Weeknd",     album = "After Hours",      duration = 200, color = "#8C3535", plays = "3.6B" },
  { id = "s8",  title = "Save Your Tears",    artist = "The Weeknd",     album = "After Hours",      duration = 215, color = "#6F2727", plays = "2.2B" },
  { id = "s9",  title = "In Your Eyes",       artist = "The Weeknd",     album = "After Hours",      duration = 237, color = "#A04444", plays = "1.1B" },
  { id = "s10", title = "Bad Guy",            artist = "Billie Eilish",  album = "Happier Than Ever",duration = 194, color = "#397A70", plays = "2.9B" },
  { id = "s11", title = "Happier Than Ever",  artist = "Billie Eilish",  album = "Happier Than Ever",duration = 298, color = "#2C5D55", plays = "721M" },
  { id = "s12", title = "The Less I Know The Better", artist = "Tame Impala", album = "Currents",    duration = 216, color = "#7758A0", plays = "1.3B" },
  { id = "s13", title = "Let It Happen",      artist = "Tame Impala",    album = "Currents",         duration = 467, color = "#5C4380", plays = "692M" },
}

local playlists = {
  { id = "pl1", title = "Daily Mix 1",       subtitle = "Arctic Monkeys, Tame Impala and more", color = "#B5394F", songs = { "s1", "s12", "s3", "s13" } },
  { id = "pl2", title = "Discover Weekly",   subtitle = "Your weekly mixtape of fresh music",  color = "#3E8E7E", songs = { "s10", "s4", "s6", "s11" } },
  { id = "pl3", title = "Release Radar",     subtitle = "Catch all the latest music",          color = "#6E4C9E", songs = { "s7", "s5", "s2", "s9" } },
  { id = "pl4", title = "Liked Songs",       subtitle = "Your collection",                     color = "#3D5BA8", songs = { "s1", "s4", "s7", "s10" } },
  { id = "pl5", title = "Chill Hits",        subtitle = "Kick back to the best new music",      color = "#B07A3A", songs = { "s8", "s11", "s13", "s3" } },
  { id = "pl6", title = "Pop Right Now",     subtitle = "The biggest pop songs",               color = "#A63E78", songs = { "s4", "s5", "s6", "s10" } },
}

local function song_by_id(id)
  for _, item in ipairs(songs) do
    if item.id == id then return item end
  end
  return songs[1]
end

local function album_by_id(id)
  for _, item in ipairs(albums) do if item.id == id then return item end end
  return albums[1]
end

local function artist_by_id(id)
  for _, item in ipairs(artists) do if item.id == id then return item end end
  return artists[1]
end

local function playlist_by_id(id)
  for _, item in ipairs(playlists) do if item.id == id then return item end end
  return playlists[1]
end

local function clock(seconds)
  return string.format("%d:%02d", math.floor(seconds / 60), math.floor(seconds % 60))
end

-- ---------------------------------------------------------------------------
-- Persisted player state. Only scalars go through mre.persist.
-- ---------------------------------------------------------------------------

local current  = mre.persist("ytm.current", "s1")
local liked_ids = mre.persist("ytm.liked", "s1,s4,s7")
local shuffle  = mre.persist("ytm.shuffle", false)
local repeat_one = mre.persist("ytm.repeat", false)
local playing  = mre.signal(false)
local progress = mre.signal(0)
local query    = mre.signal("")
local library_tab = mre.signal(1)

local function is_liked(id)
  return ("," .. liked_ids() .. ","):find("," .. id .. ",", 1, true) ~= nil
end

local function toggle_like(id)
  local kept = {}
  local was = false
  for token in string.gmatch(liked_ids(), "[^,]+") do
    if token == id then was = true else kept[#kept + 1] = token end
  end
  if not was then kept[#kept + 1] = id end
  liked_ids(table.concat(kept, ","))
end

local function step(direction)
  local index = 1
  for position, item in ipairs(songs) do
    if item.id == current() then index = position break end
  end
  if shuffle() then
    index = ((index * 7 + 3) % #songs) + 1
  else
    index = ((index - 1 + direction) % #songs) + 1
  end
  current(songs[index].id)
  progress(0)
  playing(true)
end

local function play(ctx, id)
  if id then current(id) end
  progress(0)
  playing(true)
  ctx.nav:push("player", { id = current() })
end

local function advance()
  step(1)
end

local function tick()
  mre.after(1000, function()
    if playing() then
      local item = song_by_id(current())
      progress(progress() + 1)
      if progress() >= item.duration then
        if repeat_one() then
          progress(0)
        else
          advance()
        end
      end
    end
    tick()
  end)
end
tick()

-- ---------------------------------------------------------------------------
-- Small visual atoms.
-- ---------------------------------------------------------------------------

local function txt(value, size, color, weight)
  return mre.text {
    tostring(value),
    size = size or 13,
    color = color or TEXT,
    weight = weight or 500,
  }
end

local function initials(name)
  local out = ""
  for word in tostring(name):gmatch("%S+") do out = out .. word:sub(1, 1) end
  return out:sub(1, 2):upper()
end

local function art(color, name, size)
  return mre.stack {
    width = size,
    height = size,
    radius = 10,
    background = color,
    clip = true,
    align = "center",
    justify = "center",
    txt(initials(name), size * 0.38, "#FFFFFF", 700),
  }
end

local function big_art(item)
  return mre.stack {
    width = "100%",
    height = 260,
    radius = 18,
    background = item.color,
    clip = true,
    align = "center",
    justify = "center",
    txt(initials(item.title), 76, "#FFFFFF", 700),
  }
end

local function label_button(text, key, on_click, background, foreground, width, height)
  return mre.button {
    text,
    key = key,
    on_click = on_click,
    background = background or CHIP,
    color = foreground or TEXT,
    radius = 16,
    width = width or 64,
    height = height or 32,
  }
end

local function section(title, action, on_action)
  local children = {
    txt(title, 16, TEXT, 700),
  }
  if action then
    children[2] = mre.button {
      action,
      key = "more-" .. title,
      on_click = on_action or function() end,
      background = "transparent",
      color = PINK,
      size = 11,
    }
  end
  return mre.row({
    width = "100%",
    align = "center",
    justify = "between",
  }, table.unpack(children))
end

local function chip_row(labels, active_index)
  local children = {}
  for index, value in ipairs(labels) do
    children[index] = mre.button {
      value,
      key = "chip-" .. value,
      background = index == active_index and RED or CHIP,
      color = index == active_index and "#FFFFFF" or MUTED,
      radius = 16,
      height = 30,
      padding = { left = 12, right = 12 },
    }
  end
  return mre.row({ width = "100%", gap = 8, clip = true }, table.unpack(children))
end

local function tab_item(ctx, id, glyph, title, active)
  local color = active and RED or MUTED
  return mre.column {
    width = 86,
    gap = 4,
    align = "center",
    justify = "center",
    interactive = true,
    key = "tab-" .. id,
    on_click = function() ctx.nav:set_tab(id) end,
    mre.row {
      width = 30,
      height = 22,
      radius = 11,
      align = "center",
      justify = "center",
      background = active and "#381820" or "transparent",
      txt(glyph, 15, color, 700),
    },
    txt(title, 10, color, 600),
  }
end

local function bottom_nav(ctx, active)
  return mre.column {
    width = "100%",
    background = "#101012",
    mre.divider { color = LINE },
    mre.row {
      width = "100%",
      height = 58,
      justify = "space-around",
      align = "center",
      tab_item(ctx, "home", "▶", "Home", active == "home"),
      tab_item(ctx, "search", "?", "Search", active == "search"),
      tab_item(ctx, "library", "≡", "Library", active == "library"),
    },
  }
end

local function mini_player(ctx)
  local item = song_by_id(current())
  return mre.row {
    key = "mini-player",
    width = "100%",
    height = 56,
    gap = 10,
    padding = { left = 12, right = 12 },
    background = ELEVATED,
    interactive = true,
    on_click = function() ctx.nav:push("player", { id = item.id }) end,
    art(item.color, item.title, 38),
    mre.column {
      grow = 1,
      gap = 2,
      txt(item.title, 12, TEXT, 600),
      txt(item.artist, 10, MUTED),
    },
    mre.text {
      playing() and "PAUSE" or "PLAY",
      size = 10,
      weight = 700,
      color = PINK,
    },
  }
end

local function shell(ctx, active, content)
  return mre.column {
    width = "100%",
    height = "100%",
    background = BG,
    content,
    mini_player(ctx),
    bottom_nav(ctx, active),
  }
end

local function app_bar(ctx, title)
  return mre.row {
    width = "100%",
    height = 54,
    padding = { left = 16, right = 16 },
    align = "center",
    justify = "between",
    mre.row {
      gap = 8,
      align = "center",
      txt("♫", 20, RED, 700),
      txt("MUSIC", 15, TEXT, 700),
    },
    mre.button {
      "SEARCH",
      key = "app-search",
      on_click = function() ctx.nav:set_tab("search") end,
      background = "transparent",
      color = MUTED,
      size = 10,
    },
  }
end

local function card(ctx, kind, item, subtitle, color)
  local id = item.id
  local title = item.title or item.name
  return mre.column {
    key = "card-" .. kind .. "-" .. id,
    width = 136,
    gap = 7,
    interactive = true,
    on_click = function() ctx.nav:push(kind, { id = id }) end,
    art(color or item.color, title, 128),
    txt(title, 12, TEXT, 650),
    txt(subtitle or item.subtitle or item.artist or item.meta, 10, MUTED),
  }
end

local function shelf(ctx, title, kind, items, subtitle_for)
  local children = {}
  for index, item in ipairs(items) do
    children[index] = card(ctx, kind, item, subtitle_for and subtitle_for(item) or nil)
  end
  return mre.column {
    width = "100%",
    gap = 10,
    section(title),
    mre.row({ width = "100%", gap = 12, clip = true }, table.unpack(children)),
  }
end

local function song_row(ctx, item, index)
  return mre.row {
    key = "song-row-" .. item.id,
    width = "100%",
    height = 54,
    gap = 10,
    padding = { left = 14, right = 14 },
    background = item.id == current() and "#1D2733" or "transparent",
    interactive = true,
    on_click = function() play(ctx, item.id) end,
    txt(index, 11, MUTED, 600),
    art(item.color, item.title, 38),
    mre.column {
      grow = 1,
      gap = 2,
      txt(item.title, 12, TEXT, 600),
      txt(item.artist .. " • " .. item.album, 10, MUTED),
    },
    mre.text { is_liked(item.id) and "♥" or "♡", size = 14, color = is_liked(item.id) and RED or MUTED },
    txt(clock(item.duration), 10, MUTED),
  }
end

-- ---------------------------------------------------------------------------
-- Tab roots.
-- ---------------------------------------------------------------------------

local function Home(ctx)
  local chips = { "All", "Music", "Podcasts", "Live" }
  local hero = albums[1]
  return shell(ctx, "home", mre.scroll({
    key = "home-scroll",
    grow = 1,
    clip = true,
  },
    mre.column({ width = "100%", padding = { top = 8, bottom = 14 }, gap = 18 },
      app_bar(ctx),
      chip_row(chips, 1),
      mre.column {
        key = "hero",
        width = "100%",
        height = 150,
        radius = 16,
        background = hero.color,
        clip = true,
        justify = "end",
        padding = { left = 16, bottom = 14 },
        interactive = true,
        on_click = function() ctx.nav:push("album", { id = hero.id }) end,
        txt("LISTEN AGAIN", 11, "#F2D7DD", 700),
        txt(hero.title, 26, "#FFFFFF", 700),
        txt(hero.artist, 12, "#F2D7DD", 500),
      },
      shelf(ctx, "Your playlists", "playlist", playlists),
      shelf(ctx, "Listen again", "album", albums, function(item) return item.artist .. " • " .. item.year end),
      shelf(ctx, "Popular artists", "artist", artists, function(item) return item.genre end),
      shelf(ctx, "Made for you", "playlist", { playlists[2], playlists[5], playlists[6], playlists[3] }),
      mre.text { "End of your mix — enjoy the music.", size = 11, color = MUTED, align = "center", width = "100%" },
    )
  ))
end

local function Search(ctx)
  local term = query():lower()
  local found_songs, found_artists, found_albums = {}, {}, {}
  if term ~= "" then
    for _, item in ipairs(songs) do
      if (item.title .. " " .. item.artist):lower():find(term, 1, true) then found_songs[#found_songs + 1] = item end
    end
    for _, item in ipairs(artists) do
      if (item.name .. " " .. item.genre):lower():find(term, 1, true) then found_artists[#found_artists + 1] = item end
    end
    for _, item in ipairs(albums) do
      if (item.title .. " " .. item.artist):lower():find(term, 1, true) then found_albums[#found_albums + 1] = item end
    end
  end

  local body
  if term == "" then
    local genres = { "Pop", "Indie", "R&B", "Chill", "Focus", "Workout", "Sleep", "Party" }
    local genre_cards = {}
    for index, name in ipairs(genres) do
      genre_cards[index] = mre.column {
        key = "genre-" .. name,
        width = 96,
        height = 76,
        radius = 12,
        background = CHIP,
        justify = "end",
        padding = { left = 10, bottom = 10 },
        interactive = true,
        on_click = function() query(name) end,
        txt(name, 12, TEXT, 700),
      }
    end
    body = mre.column({ width = "100%", gap = 18 },
      section("Browse all"),
      mre.row({ width = "100%", gap = 10, clip = true }, table.unpack(genre_cards)),
      section("Recent searches"),
      mre.column({ width = "100%", gap = 2 },
        mre.row {
          key = "recent-ar1", width = "100%", height = 46, gap = 12, padding = { left = 14 },
          interactive = true, on_click = function() query("Arctic") end,
          txt("◷", 14, MUTED), txt("Arctic Monkeys", 12, TEXT, 600),
        },
        mre.row {
          key = "recent-pop", width = "100%", height = 46, gap = 12, padding = { left = 14 },
          interactive = true, on_click = function() query("Pop") end,
          txt("◷", 14, MUTED), txt("Pop Right Now", 12, TEXT, 600),
        }
      ),
      shelf(ctx, "Trending now", "album", albums)
    )
  else
    local results = {}
    results[1] = section("Top result")
    if found_songs[1] then
      results[#results + 1] = mre.row {
        key = "top-result", width = "100%", height = 76, gap = 12,
        padding = { left = 14, right = 14 }, background = ELEVATED,
        interactive = true, on_click = function() play(ctx, found_songs[1].id) end,
        art(found_songs[1].color, found_songs[1].title, 56),
        mre.column { grow = 1, gap = 3, txt(found_songs[1].title, 16, TEXT, 700), txt(found_songs[1].artist, 11, MUTED) },
        txt("PLAY", 10, PINK, 700),
      }
    end
    if #found_songs > 0 then
      results[#results + 1] = section("Songs")
      for index, item in ipairs(found_songs) do results[#results + 1] = song_row(ctx, item, index) end
    end
    if #found_artists > 0 then
      results[#results + 1] = shelf(ctx, "Artists", "artist", found_artists)
    end
    if #found_albums > 0 then
      results[#results + 1] = shelf(ctx, "Albums", "album", found_albums)
    end
    if #found_songs + #found_artists + #found_albums == 0 then
      results[#results + 1] = mre.text { "No results for “" .. query() .. "”", size = 13, color = MUTED, padding = { top = 20 } }
    end
    body = mre.column({ width = "100%", gap = 14 }, table.unpack(results))
  end

  return shell(ctx, "search", mre.scroll({ key = "search-scroll", grow = 1, clip = true },
    mre.column({ width = "100%", padding = { top = 8, bottom = 14 }, gap = 16 },
      app_bar(ctx),
      mre.textfield {
        key = "search-field",
        placeholder = "Songs, artists or albums",
        value = query(),
        on_change = function(value) query(value) end,
        width = "100%",
        grow = 1,
        background = ELEVATED,
        border = { color = LINE, width = 1 },
        color = TEXT,
      },
      body
    )
  ))
end

local function Library(ctx)
  local tabs = { "Playlists", "Songs", "Albums", "Artists" }
  local tab_children = {}
  for index, name in ipairs(tabs) do
    tab_children[index] = mre.button {
      name,
      key = "library-tab-" .. name,
      on_click = function() library_tab(index) end,
      background = library_tab() == index and RED or CHIP,
      color = library_tab() == index and "#FFFFFF" or MUTED,
      radius = 15,
      height = 30,
      padding = { left = 11, right = 11 },
    }
  end

  local list
  local active = library_tab()
  if active == 1 then
    local items = {}
    for index, item in ipairs(playlists) do
      items[index] = mre.row {
        key = "lib-playlist-" .. item.id, width = "100%", height = 62, gap = 12,
        padding = { left = 14, right = 14 }, interactive = true,
        on_click = function() ctx.nav:push("playlist", { id = item.id }) end,
        art(item.color, item.title, 44),
        mre.column { grow = 1, gap = 3, txt(item.title, 13, TEXT, 600), txt(item.subtitle, 10, MUTED) },
        txt("›", 16, MUTED),
      }
    end
    list = mre.column({ width = "100%", gap = 2 }, table.unpack(items))
  elseif active == 2 then
    local items = {}
    for index, item in ipairs(songs) do items[index] = song_row(ctx, item, index) end
    list = mre.column({ width = "100%", gap = 2 }, table.unpack(items))
  elseif active == 3 then
    local items = {}
    for index, item in ipairs(albums) do
      items[index] = mre.row {
        key = "lib-album-" .. item.id, width = "100%", height = 62, gap = 12,
        padding = { left = 14, right = 14 }, interactive = true,
        on_click = function() ctx.nav:push("album", { id = item.id }) end,
        art(item.color, item.title, 44),
        mre.column { grow = 1, gap = 3, txt(item.title, 13, TEXT, 600), txt(item.artist, 10, MUTED) },
        txt("›", 16, MUTED),
      }
    end
    list = mre.column({ width = "100%", gap = 2 }, table.unpack(items))
  else
    local items = {}
    for index, item in ipairs(artists) do
      items[index] = mre.row {
        key = "lib-artist-" .. item.id, width = "100%", height = 62, gap = 12,
        padding = { left = 14, right = 14 }, interactive = true,
        on_click = function() ctx.nav:push("artist", { id = item.id }) end,
        mre.stack {
          width = 44, height = 44, radius = 22, background = item.color,
          align = "center", justify = "center", txt(initials(item.name), 14, "#FFFFFF", 700),
        },
        mre.column { grow = 1, gap = 3, txt(item.name, 13, TEXT, 600), txt(item.meta, 10, MUTED) },
        txt("›", 16, MUTED),
      }
    end
    list = mre.column({ width = "100%", gap = 2 }, table.unpack(items))
  end

  return shell(ctx, "library", mre.scroll({ key = "library-scroll", grow = 1, clip = true },
    mre.column({ width = "100%", padding = { top = 8, bottom = 14 }, gap = 14 },
      app_bar(ctx),
      txt("Your library", 22, TEXT, 700),
      mre.row({ width = "100%", gap = 8, clip = true, padding = { left = 14, right = 14 } }, table.unpack(tab_children)),
      list
    )
  ))
end

-- ---------------------------------------------------------------------------
-- Pushed screens. They paint over the tab shell and use the navigator's back.
-- ---------------------------------------------------------------------------

local function header(ctx, title, subtitle)
  return mre.column {
    width = "100%",
    gap = 4,
    mre.row {
      width = "100%",
      align = "center",
      justify = "between",
      ctx.nav:back_button {
        key = "back",
        background = ELEVATED,
        color = TEXT,
        radius = 16,
        width = 56,
        height = 32,
      },
      txt(title, 14, TEXT, 700),
      mre.row { width = 56 },
    },
    subtitle and txt(subtitle, 11, MUTED) or nil,
  }
end

local function Player(ctx)
  local item = song_by_id(ctx.params.id or current())
  local percent = math.floor(math.max(0, math.min(1, progress() / item.duration)) * 100)
  return mre.column {
    width = "100%",
    height = "100%",
    background = "#101014",
    padding = { left = 18, right = 18, top = 14, bottom = 18 },
    gap = 18,
    header(ctx, "NOW PLAYING"),
    big_art(item),
    mre.row {
      width = "100%",
      align = "center",
      justify = "between",
      mre.column { gap = 3, txt(item.title, 21, TEXT, 700), txt(item.artist, 13, MUTED) },
      mre.button {
        is_liked(item.id) and "♥" or "♡",
        key = "like",
        on_click = function() toggle_like(item.id) end,
        background = is_liked(item.id) and "#3A1C25" or ELEVATED,
        color = is_liked(item.id) and RED or MUTED,
        radius = 20,
        width = 40,
        height = 40,
      },
    },
    mre.column { width = "100%", gap = 6,
      mre.row { width = "100%", height = 5, radius = 3, background = LINE,
        mre.row { width = percent .. "%", height = 5, radius = 3, background = RED },
      },
      mre.row { width = "100%", justify = "between", txt(clock(progress()), 10, MUTED), txt(clock(item.duration), 10, MUTED) },
    },
    mre.row({ width = "100%", justify = "space-around", align = "center" },
      label_button("SHUF", "shuffle", function() shuffle(not shuffle()) end, shuffle() and "#3A1C25" or ELEVATED, shuffle() and RED or MUTED),
      label_button("◀◀", "prev", function() step(-1) end, ELEVATED, TEXT, 52, 40),
      mre.button {
        playing() and "||" or "▶",
        key = "play-toggle",
        on_click = function() playing(not playing()) end,
        background = RED,
        color = "#FFFFFF",
        radius = 30,
        width = 60,
        height = 60,
        size = 22,
      },
      label_button("▶▶", "next", function() step(1) end, ELEVATED, TEXT, 52, 40),
      label_button("LOOP", "repeat", function() repeat_one(not repeat_one()) end, repeat_one() and "#3A1C25" or ELEVATED, repeat_one() and RED or MUTED)
    ),
    mre.button {
      "QUEUE  •  UP NEXT",
      key = "open-queue",
      on_click = function() ctx.nav:push("queue", {}) end,
      background = ELEVATED,
      color = TEXT,
      radius = 12,
      width = "100%",
      height = 42,
    },
    mre.text { "Mock playback — no network required.", size = 10, color = MUTED, width = "100%", align = "center" },
  }
end

local function CollectionScreen(ctx, kind)
  local id = ctx.params.id
  local item, title, subtitle, color, list
  if kind == "album" then
    item = album_by_id(id); title = item.title; subtitle = item.artist .. " • " .. item.year; color = item.color; list = item.songs
  elseif kind == "playlist" then
    item = playlist_by_id(id); title = item.title; subtitle = item.subtitle; color = item.color; list = item.songs
  else
    item = artist_by_id(id); title = item.name; subtitle = item.meta; color = item.color; list = {}
    for _, song in ipairs(songs) do if song.artist == item.name then list[#list + 1] = song.id end end
  end

  local rows = {}
  for index, song_id in ipairs(list) do
    rows[index] = song_row(ctx, song_by_id(song_id), index)
  end

  return mre.column {
    width = "100%",
    height = "100%",
    background = BG,
    padding = { left = 14, right = 14, top = 14, bottom = 14 },
    gap = 12,
    header(ctx, kind:upper()),
    mre.scroll({ key = "collection-" .. kind .. "-" .. tostring(id), grow = 1, clip = true },
      mre.column({ width = "100%", gap = 14 },
        big_art({ title = title, color = color }),
        mre.column { gap = 3, txt(title, 23, TEXT, 700), txt(subtitle, 12, MUTED) },
        mre.row { width = "100%", gap = 10,
          mre.button {
            "PLAY",
            key = "collection-play",
            on_click = function() play(ctx, list[1]) end,
            background = RED,
            color = "#FFFFFF",
            radius = 20,
            width = 100,
            height = 40,
          },
          label_button("SHUFFLE", "collection-shuffle", function() shuffle(true); play(ctx, list[1]) end, ELEVATED, MUTED, 92, 40),
        },
        mre.text { "SONGS", size = 11, color = MUTED, weight = 700 },
        mre.column({ width = "100%", gap = 2 }, table.unpack(rows)),
      )
    ),
  }
end

local function Queue(ctx)
  local rows = {}
  for index, item in ipairs(songs) do
    rows[index] = mre.row {
      key = "queue-" .. item.id,
      width = "100%",
      height = 50,
      gap = 10,
      padding = { left = 12, right = 12 },
      background = item.id == current() and "#1D2733" or "transparent",
      interactive = true,
      on_click = function() play(ctx, item.id) end,
      txt(index, 11, MUTED),
      art(item.color, item.title, 34),
      mre.column { grow = 1, gap = 2, txt(item.title, 12, TEXT, 600), txt(item.artist, 10, MUTED) },
      txt(clock(item.duration), 10, MUTED),
    }
  end
  return mre.column {
    width = "100%", height = "100%", background = BG, padding = { left = 12, right = 12, top = 14 }, gap = 12,
    header(ctx, "QUEUE", "Up next from your mock library"),
    mre.scroll({ key = "queue-scroll", grow = 1, clip = true },
      mre.column({ width = "100%", gap = 2 }, table.unpack(rows))
    ),
  }
end

-- ---------------------------------------------------------------------------
-- Navigation graph. Each tab owns its own stack; back pops within the tab.
-- ---------------------------------------------------------------------------

local nav = mre.nav {
  routes = {
    home = Home,
    search = Search,
    library = Library,
    player = Player,
    playlist = function(ctx) return CollectionScreen(ctx, "playlist") end,
    album = function(ctx) return CollectionScreen(ctx, "album") end,
    artist = function(ctx) return CollectionScreen(ctx, "artist") end,
    queue = Queue,
  },
  tabs = {
    { id = "home", title = "Home", initial = "home" },
    { id = "search", title = "Search", initial = "search" },
    { id = "library", title = "Library", initial = "library" },
  },
  initial = "home",
  persist = "ytm.nav",
  transition_ms = 260,
  spring = { stiffness = 300, damping = 30 },
}

mre.app {
  view = function()
    return nav:view()
  end,
}
