-- core/init.lua
-- Normal operation entry point.
-- Wires up ui modules, then starts the run loop.

local workspace = require("core.workspace")
local hooks     = require("core.hooks")
local metrics   = require("core.metrics")
local overlay   = require("core.overlay")
local theme     = require("ui.theme")
local animation = require("ui.animation")
local layout    = require("ui.layout")

local M = {}

function M.start()
woven.log.info("core: initializing")

-- expose woven.settings() so user config can call it
-- routes into layout and animation modules
woven.settings = function(values)
layout.apply_settings(values)
woven.log.info("settings: applied")
end

-- expose woven.workspaces() config call
woven.workspaces = function(values)
layout.apply_workspace(values)
layout.apply_popout(values)
end

-- expose woven.animations() config call
woven.animations = function(values)
animation.apply(values)
end

-- save the raw Rust binding BEFORE we override woven.theme
-- theme.apply() calls this directly to push values to Rust
-- without going through our override (which would recurse forever)
local _rust_theme_push = woven.theme

-- override woven.theme() to go through ui.theme module for merge logic
woven.theme = function(values)
theme.apply(values, _rust_theme_push)
end

-- fetch initial compositor state
workspace.refresh()

-- start metric polling
metrics.start_polling(2000)

-- init hook system (installs woven.on / woven.bind)
hooks.init()

woven.log.info("core: ready — overlay waiting for toggle")

-- init overlay (non-blocking, waits for key events from woven-render)
overlay.init()
end

return M
