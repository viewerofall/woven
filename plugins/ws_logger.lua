-- woven plugin api: 1.0
-- plugins/ws_logger.lua
-- Workspace focus logger + error handler example.
--
-- Demonstrates:
--   woven.on()       — reacting to compositor events
--   woven.on_error() — custom config error handler
--
-- Usage (in woven.lua):
--   require("plugins.ws_logger").setup()

local M = {}

function M.setup()
    -- Custom error handler: called IN ADDITION to the overlay toast + notify-send
    -- when the config file has a Lua error. Use it to add your own handling.
    woven.on_error(function(msg)
        woven.log.error("[error-handler] Config problem: " .. msg)
        -- You could write to a file, trigger a sound, etc.
    end)

    -- Log every workspace focus event.
    woven.on("workspace_focus", function(data)
        woven.log.info(string.format("[ws_logger] focused workspace %d", data.id))
    end)

    -- Log every new window.
    woven.on("window_open", function(data)
        woven.log.info(string.format("[ws_logger] opened %s — %s (ws %d)",
            data.class or "?", data.title or "?", data.workspace or 0))
    end)

    -- Log closed windows.
    woven.on("window_close", function(data)
        woven.log.info(string.format("[ws_logger] closed %s", data.id))
    end)

    -- Log windows that move between workspaces.
    woven.on("window_move", function(data)
        woven.log.info(string.format("[ws_logger] window %s moved to workspace %d",
            data.id, data.workspace))
    end)
end

return M
