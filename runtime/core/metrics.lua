local hooks = require("core.hooks")
local M = { _cache = {}, _interval = 2000 }
function M.start_polling(ms)
    M._interval = ms or 2000
    woven.log.info("metrics: polling every " .. M._interval .. "ms")
end
function M.poll(workspace_id)
    local data = woven.metrics.workspace(workspace_id)
    if data then
        local prev = M._cache[workspace_id]
        M._cache[workspace_id] = data
        if data.cpu_total > 80 and (not prev or prev.cpu_total <= 80) then
            hooks.fire("cpu_high", { workspace_id = workspace_id, cpu = data.cpu_total })
        end
    end
    return M._cache[workspace_id]
end
function M.get(id) return M._cache[id] end
return M
