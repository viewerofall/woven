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

function M.start(rust_theme_push, rust_workspaces_push)
woven.log.info("core: initializing")

-- wire woven.settings() — pure Lua layout/animation settings
woven.settings = function(values)
layout.apply_settings(values)
animation.apply(values)
woven.log.info("settings: applied")
end

-- wire woven.workspaces() — Lua layout side + Rust render side
woven.workspaces = function(values)
layout.apply_workspace(values)
layout.apply_popout(values)
if rust_workspaces_push then rust_workspaces_push(values) end
    end

    -- wire woven.animations() — pure Lua
    woven.animations = function(values)
    animation.apply(values)
    end

    -- wire woven.theme() through ui.theme merge logic, then push to Rust
    woven.theme = function(values)
    theme.apply(values, rust_theme_push)
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
