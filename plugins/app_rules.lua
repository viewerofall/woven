-- woven plugin api: 1.0
-- plugins/app_rules.lua
-- Per-app accent color rules.  Maps window class names to hex colors.
-- These override the default class-hash colors in the overlay cards.
--
-- Usage (in woven.lua):
--   require("plugins.app_rules").setup()
--
-- Or with custom overrides:
--   require("plugins.app_rules").setup({
--       ["myapp"] = "#ff6c6b",
--   })

local M = {}

-- Sensible defaults matching popular app palettes.
local DEFAULTS = {
    -- Terminals
    ["kitty"]       = "#89b4fa",   -- blue
    ["alacritty"]   = "#89dceb",   -- sky
    ["foot"]        = "#94e2d5",   -- teal
    ["wezterm"]     = "#cba6f7",   -- mauve

    -- Browsers
    ["firefox"]     = "#fab387",   -- peach
    ["chromium"]    = "#89b4fa",   -- blue
    ["google-chrome"] = "#89b4fa",

    -- Editors / IDEs
    ["code"]        = "#89b4fa",   -- blue (VS Code)
    ["code-oss"]    = "#89b4fa",
    ["neovide"]     = "#a6e3a1",   -- green
    ["jetbrains-idea"] = "#f38ba8",

    -- File managers
    ["thunar"]      = "#f9e2af",   -- yellow
    ["nautilus"]    = "#89b4fa",
    ["dolphin"]     = "#89b4fa",

    -- Media
    ["mpv"]         = "#a6e3a1",   -- green
    ["vlc"]         = "#fab387",   -- peach
    ["spotify"]     = "#a6e3a1",

    -- Communication
    ["discord"]     = "#cba6f7",   -- mauve
    ["slack"]       = "#89dceb",
    ["telegram-desktop"] = "#89b4fa",

    -- Misc
    ["obsidian"]    = "#cba6f7",
    ["steam"]       = "#89b4fa",
    ["gimp"]        = "#f9e2af",
}

function M.setup(extra)
    -- Merge defaults with any user overrides
    local rules = {}
    for k, v in pairs(DEFAULTS) do rules[k] = v end
    if extra then
        for k, v in pairs(extra) do rules[k] = v end
    end
    woven.rules(rules)
end

return M
