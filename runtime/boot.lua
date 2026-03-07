-- boot.lua
-- Rust hands execution here after sandbox + API setup.
-- This is the real main(). Lua owns everything from this point.

local log      = woven.log
local fs       = woven.fs

log.info("boot.lua started")

-- Pre-wire woven.theme / woven.settings / woven.workspaces / woven.animations
-- as no-ops that buffer their values, so user config can call them safely
-- before core.start() upgrades them to the real implementations.
-- Without this, calling woven.theme({}) in config would hit the raw Rust
-- binding which doesn't do the Lua-side merge logic yet.
local _buffered_theme    = {}
local _buffered_settings = {}
local _buffered_ws       = {}
local _buffered_anim     = {}

woven.theme      = function(v) _buffered_theme    = v or {} end
woven.settings   = function(v) _buffered_settings = v or {} end
woven.workspaces = function(v) _buffered_ws       = v or {} end
woven.animations = function(v) _buffered_anim     = v or {} end

-- check for user config
if not fs.exists(fs.config_path()) then
    log.warn("No config found at: " .. fs.config_path())
    log.info("Entering setup guide...")
    local fallback = require("fallback.init")
    fallback.start()
    return
end

-- load user config inside pcall
-- this executes woven.theme(), woven.settings() etc. into the buffers above
local ok, err = pcall(function()
    local code  = fs.read(fs.config_path())
    local chunk, load_err = load(code, "woven.lua")
    if not chunk then
        error("Parse error: " .. tostring(load_err))
    end
    chunk()
end)

if not ok then
    log.error("Config error: " .. tostring(err))
    log.info("Falling back to setup guide...")
    local fallback = require("fallback.init")
    fallback.start()
    return
end

-- start core — this upgrades the woven.* shims to real implementations
-- then replays the buffered values through them
local core = require("core.init")
core.start()

-- replay buffered config calls through the now-real implementations
woven.theme(_buffered_theme)
woven.settings(_buffered_settings)
woven.workspaces(_buffered_ws)
woven.animations(_buffered_anim)

log.info("boot: complete")
