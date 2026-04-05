# Contributing Plugins

Want your plugin in the official woven repository? Here's how.

## Requirements

1. **Single `.lua` file** — one file per plugin, placed in `plugins/`
2. **Version header** — first line must be `-- woven plugin api: 1.0`
3. **Module pattern** — export a table with a `setup(opts)` function
4. **Sensible defaults** — `setup()` with no args must work out of the box
5. **No external dependencies** — only use `woven.*` APIs (see `PLUGIN_API.md`)
6. **No filesystem writes outside store** — use `woven.store` for persistence, not `woven.fs.write`

## Plugin structure

```lua
-- woven plugin api: 1.0
-- plugins/yourplugin.lua
-- Short description of what it does.
--
-- Usage:
--   require("plugins.yourplugin").setup()

local M = {}

function M.setup(opts)
    opts = opts or {}

    local handle = woven.plugin.register({
        name = "yourplugin",
        type = "bar_widget",
    })

    handle.widget({
        slot     = opts.slot     or "top",      -- default slot
        height   = opts.height   or 56,         -- default height
        interval = opts.interval or 10,         -- refresh interval in seconds
        render   = function(ctx)
            -- drawing code here
        end,
    })
end

return M
```

## Slots

Pick the slot that makes sense for your plugin:

| Slot | Width | Use case |
|------|-------|----------|
| `top` | 40px canvas | Compact info (clock, date, battery) |
| `bottom` | 40px canvas | Compact info (sysinfo, now playing) |
| `panel` | 272px canvas | Expanded content (cava, detailed stats) |
| `overlay` | 252px canvas | Widget strip items (greeting, network, launchers) |

## Submitting

1. Fork `viewerofall/woven`
2. Add your `.lua` file to `plugins/`
3. Test it locally: copy to `~/.config/woven/plugins/`, add the require to your `woven.lua`, confirm it renders
4. Open a PR with:
   - The plugin file
   - A short description of what it does
   - A screenshot if it has visual output

## Guidelines

- Keep rendering efficient — avoid heavy computation in `render()` since it runs every `interval` seconds
- Use `woven.store` for state that should persist across restarts
- Use `woven.http.get()` sparingly — it blocks the Lua thread, so set a reasonable `interval` (300+ seconds for API calls)
- Handle missing data gracefully — if a file or API isn't available, show a fallback, don't crash
- Respect the user's theme — use `opts.color` overrides where it makes sense so users can customize

## What gets accepted

- Useful general-purpose widgets (weather, system monitors, media, productivity)
- Well-tested, clean code
- Plugins that don't duplicate existing functionality

## What doesn't get accepted

- Plugins that shell out excessively or run arbitrary commands
- Plugins that write to the filesystem outside `woven.store`
- Joke/novelty plugins (keep those in your own config)
- Plugins with hardcoded paths or distro-specific assumptions
