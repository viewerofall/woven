-- fallback/guide.lua
-- Interactive setup wizard. Runs when no config exists.
-- Lua asks questions; Rust provides input/output via woven.guide.*
-- Sandbox prevents any writes outside config dir.

local M = {}

function M.run(defaults)
    local values = {}
    for k, v in pairs(defaults.values) do
        values[k] = v
    end

    woven.guide.print("╔═══════════════════════════════╗")
    woven.guide.print("║   woven — first time setup    ║")
    woven.guide.print("╚═══════════════════════════════╝")
    woven.guide.print("")

    -- compositor is auto-detected by Rust on startup
    local comp = woven.compositor.detect()
    woven.guide.print("Detected compositor: " .. comp)
    woven.guide.print("")

    -- toggle key
    woven.guide.print("Toggle key (default: SUPER+TAB, press Enter to keep):")
    local key = woven.guide.input()
    if key ~= "" then values.toggle_key = key end

    -- blur
    woven.guide.print("Enable blur? (y/n, default: y):")
    local blur_in = woven.guide.input()
    if blur_in == "n" then values.blur = false end

    -- color scheme quick pick
    woven.guide.print("Color scheme? (1=catppuccin 2=gruvbox 3=nord 4=keep defaults):")
    local scheme = woven.guide.input()
    if scheme == "1" then
        values.background = "#1e1e2e"
        values.border     = "#cba6f7"
        values.text       = "#cdd6f4"
        values.accent     = "#89b4fa"
    elseif scheme == "2" then
        values.background = "#282828"
        values.border     = "#d79921"
        values.text       = "#ebdbb2"
        values.accent     = "#458588"
    elseif scheme == "3" then
        values.background = "#2e3440"
        values.border     = "#88c0d0"
        values.text       = "#eceff4"
        values.accent     = "#81a1c1"
    end

    -- write config
    local config_str = defaults.render(values)
    local path       = woven.fs.config_path()

    woven.guide.print("")
    woven.guide.print("Writing config to: " .. path)
    woven.fs.write(path, config_str)
    woven.guide.print("Done! Restart woven to apply.")
end

return M
