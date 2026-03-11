-- boot.lua
-- Rust hands execution here after sandbox + API setup.

local log = woven.log
local fs  = woven.fs

log.info("boot.lua started")

-- Save raw Rust bindings before the buffer shims overwrite them.
local _rust_theme      = woven.theme
local _rust_workspaces = woven.workspaces

-- Buffer shims so user config can call woven.theme() etc. before core.start().
local _buffered_theme    = {}
local _buffered_settings = {}
local _buffered_ws       = {}
local _buffered_anim     = {}

woven.theme      = function(v) _buffered_theme    = v or {} end
woven.settings   = function(v) _buffered_settings = v or {} end
woven.workspaces = function(v) _buffered_ws       = v or {} end
woven.animations = function(v) _buffered_anim     = v or {} end

-- ── First-time setup ──────────────────────────────────────────────────────────
-- If no config exists, launch woven-ctrl --setup (a proper GUI wizard)
-- and wait up to 120 seconds for the user to finish and write the config.
if not fs.exists(fs.config_path()) then
    log.warn("No config found at: " .. fs.config_path())
    log.info("Launching woven-ctrl --setup for first-time configuration...")

    woven.process.spawn("woven-ctrl", {"--setup"})

    -- Poll until config appears (user finishes wizard) or timeout
    local waited = 0
    while not fs.exists(fs.config_path()) and waited < 120 do
        woven.process.sleep(1000)
        waited = waited + 1
    end

    if not fs.exists(fs.config_path()) then
        log.warn("Setup timed out or was cancelled — using built-in defaults.")
        -- write a default config so we don't loop forever
        local default = require("fallback.defaults")
        fs.write(fs.config_path(), default.render(default.values))
    else
        log.info("Config written by setup wizard, continuing...")
    end
end

-- ── Load user config ──────────────────────────────────────────────────────────
local ok, err = pcall(function()
    local code = fs.read(fs.config_path())
    local chunk, load_err = load(code, "woven.lua")
    if not chunk then
        error("Parse error: " .. tostring(load_err))
    end
    chunk()
end)

if not ok then
    log.error("Config error: " .. tostring(err))
    log.warn("Continuing with built-in defaults.")
    -- don't block startup on a bad config
end

-- ── Start core and replay buffered config ─────────────────────────────────────
local core = require("core.init")
core.start(_rust_theme, _rust_workspaces)

woven.theme(_buffered_theme)
woven.settings(_buffered_settings)
woven.workspaces(_buffered_ws)
woven.animations(_buffered_anim)

log.info("boot: complete")
