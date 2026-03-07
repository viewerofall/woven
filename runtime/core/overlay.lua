-- core/overlay.lua
-- Controls overlay show/hide/toggle state.
-- Also owns the main run loop that keeps woven alive.

local workspace = require("core.workspace")
local metrics   = require("core.metrics")

local M = { _visible = false }

function M.init()
woven.log.info("overlay: ready")
-- state polling is handled by the Rust poller in main.rs
-- Lua only manages show/hide/toggle and config hooks
M._loop()
end

function M._push_state()
-- no-op: state is now pushed from Rust directly
end

function M._loop()
woven.log.info("overlay: entering run loop")
while true do
    -- Rust handles all state polling.
    -- Lua just keeps the process alive for config hooks.
    woven.sleep(1000)
    end
    end

    function M.show()
    M._visible = true
    woven.overlay.show()
    woven.log.info("overlay: shown")
    end

    function M.hide()
    M._visible = false
    woven.overlay.hide()
    woven.log.info("overlay: hidden")
    end

    function M.toggle()
    if M._visible then M.hide() else M.show() end
        end

        function M.is_visible()
        return M._visible
        end

        return M
